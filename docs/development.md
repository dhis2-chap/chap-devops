# Development

## Make targets

```sh
make test      # run the test suite
make check     # formatting and clippy, fixing nothing
make lint      # fix what check reports: clippy --fix, then format (writes files)
make build     # fast host-arch compile check
make release   # universal (arm64+x86_64) macOS binary at bin/chap-devops
make install   # copy bin/chap-devops to $PREFIX/bin, with the link cdx (PREFIX defaults to ~/.local)
make run       # build the release binary, then run it (ARGS="...")
make vendor    # refresh the embedded marketplace snapshot
make docs      # regenerate the reference, then build the book into site/
make docs-serve        # serve the book locally with live reload
make docs-reference    # regenerate docs/reference.md from the CLI help texts
make release-tag VERSION=x.y.z   # bump, check, commit and tag a release
make clean     # remove target/ and bin/
```

`make help` lists every target. The underlying commands are `cargo test`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt --all --check` and
`scripts/vendor-marketplace.sh`.

## Tests

```sh
cargo test
```

Unit tests live next to the code they cover, always in `<module>/tests.rs`
(`src/status.rs` and `src/status/tests.rs`), declared at the bottom of the
module as `#[cfg(test)] mod tests;`; there are no inline `mod tests { }`
blocks. A code file stays under about 600 lines: past that it becomes a folder
module split by concern, `foo.rs` with the module doc, the `mod` lines and the
re-exports, and `foo/<concern>.rs` beside it. Modules are `foo.rs` + `foo/`,
never `foo/mod.rs`. `tests/cli/` is the end-to-end suite: it runs the built binary
against temporary directories, and every run is `--offline` with its own cache
directory. That cache starts with the frozen marketplace data of
`tests/fixtures/marketplace/` (`tests/support/marketplace.rs`), so no test
touches the network or the developer's real cache. It is one test binary
with a module per concern (`init.rs`, `models.rs`, `dhis2.rs`, ...);
`common/` holds the sandbox every test runs in and the stand-in servers for
GitHub, ghcr, the marketplace and chap-core, and `dhis2_stand_in.rs` the one
for DHIS2. A new test goes in the module for its command, and a helper moves to
`common/` once a second module needs it. Keep a module under about 1500 lines;
past that, split it by concern.

`chap-devops models add` and `chap-devops update --chap-tag` are the commands that cannot
be tested that way: they resolve a repository, a release or a compose file over
HTTP. Those tests stand a local server in for everything they would reach -
GitHub's REST API, the raw file host, ghcr and the marketplace index - through
four hooks nothing on the command line can set:

| Variable | What it moves |
| --- | --- |
| `CHAP_DEVOPS_GITHUB_API` | The GitHub REST base URL, normally `https://api.github.com`: the repository lookups, chap-core's release feed, `chap-devops`'s own, and `/rate_limit`. |
| `CHAP_DEVOPS_GITHUB_RAW` | The raw file base URL, normally `https://raw.githubusercontent.com`: chap-core's `compose.ghcr.yml` at a ref. |
| `CHAP_DEVOPS_GHCR_URL` | The registry base URL, normally `https://ghcr.io`. |
| `CHAP_DEVOPS_OCI_URL` | The base URL of every other image registry, normally the host in the image name. |
| `CHAP_DEVOPS_NO_DOCKER_PROBE` | `1` turns off the uid probe, which would otherwise `docker pull` an image the test has not got. |

The registry itself moves with the ordinary `--registry-url` flag. The hooks
exist for the tests: an operator who wants another registry wants another image
reference, and one who wants another catalogue has the flag.

`GITHUB_TOKEN` and `GH_TOKEN` are not hooks but the real thing: the first of
the two with a value in it is sent as `Authorization: Bearer` on every
`api.github.com` request, which is 5000 requests an hour instead of 60. Both
are cleared for every end-to-end run, so a token in the developer's shell
cannot change what a test sees, and set deliberately in the one test where the
header itself is what is under test. Nothing on the command line sets a token
and nothing writes one down. See [Doctor](./doctor.md#raising-the-github-limit).

The tests that need Docker check for it first and skip with a note on stderr
when it is not there, so the suite is green on a machine without a daemon.

One test is `#[ignore]`d as well: `live_up_then_purge_removes_the_model_volume`
starts a real deployment, which pulls every image. Run it by hand on a
machine with Docker when the volume handling changes:

```sh
cargo test --test cli -- --ignored live_up_then_purge
```

### The end-to-end run

`scripts/e2e.sh` (or `make e2e`) runs everything for real against published
images, once per chap-core tag (`latest` and `master` by default) and DHIS2
version (2.41, 2.42, 2.43 and `dev` by default; `dev` is the unreleased next
one, `dhis2/core-dev:master`). Each pair gets one deployment with OCS, the
object store, DHIS2 and `chapkit_simple_multistep_model` running together, on
ports of its own. `--models all` runs every marketplace model instead, which
takes much longer and needs more disk.
DHIS2 2.42 is seeded from the Laos climate demo chap-devops uses by default, and the
other versions start empty: DHIS2's Sierra Leone demo exists for some of them,
but its `admin` may not create the route `dhis2 connect` writes. Then:

1. `chap-devops up`, and a wait until `chap-devops status` exits 0: every service up and
   every model registered;
2. `chap-devops doctor`;
3. OCS answering `/health` and listing `/stac/collections`, then ingesting
   three days of CHIRPS3 (public, no credentials) and publishing it;
4. `chap-devops dhis2 connect`: the route, the apps, analytics;
5. `chap-devops models test --all`: every model trains and predicts;
6. `chap-devops models test --all --backtest`: a dataset, a backtest and its scores
   through chap-core, the path the Modeling App takes;
7. `chap-devops chap eval` of the first model over the deployment's network, on
   chap-core's Laos example data, then `chap-devops chap plot-backtest`. Both files
   must be on the host.

```sh
scripts/e2e.sh                          # latest, then master
scripts/e2e.sh --models all             # every marketplace model
scripts/e2e.sh --dhis2 dev              # only the unreleased DHIS2
scripts/e2e.sh --dhis2 2.42             # one DHIS2 version
scripts/e2e.sh --tags v2.3.1 --keep     # one tag, left running afterwards
scripts/e2e.sh --no-dhis2 --work ./e2e  # no DHIS2, results under ./e2e
scripts/e2e.sh --chap-admin --tags master  # what chap-admin does, and a takeover
make e2e TAGS="latest"
```

`--chap-admin` runs what `chap-admin` does, with chap-core only, once per tag.
It installs `chapkit_simple_multistep_model` at 0.1.3 and updates it to the
stable version. It builds a model from `chap-devops models new` and adds the
image, then uninstalls the first model. Each step checks the row and the
registered version in `chap-devops status --json`. Then it starts chap-core's
own `compose.ghcr.yml`, stops it, and takes it over with
`chap-devops init --adopt`. [Taking over a chap-admin
deployment](./use-cases/from-chap-admin.md) maps each `chap-admin` command.

A failed step does not stop the run: its log is kept, the container logs are
collected, and the steps that do not depend on it still run. `report.md` in the
work directory has one row per tag and step with its result and duration, and
the script exits non-zero when any step failed. It needs Docker with room for
DHIS2 (about 8 GB) and the network, and it takes a while.

The `e2e` workflow (`.github/workflows/e2e.yml`) runs the script every Monday
at 03:00 UTC, with one job for each chap-core tag (`latest` and `master`) and
DHIS2 2.42. To run it at a different time, start it from the Actions tab. You
can then set the tags, the DHIS2 versions (`none` leaves DHIS2 out) and the
models. Each job puts `report.md` in the run summary and keeps the report and
the logs as an artifact for 14 days.

## Slides

The slide decks are [Marp](https://marp.app/) Markdown: one file for each deck
in `slides/decks/`, and one theme, `slides/theme/chap-devops.css`. A `---` line
starts a new slide.

```sh
make slides      # every deck, as HTML and as a 16:9 PDF, into site/slides/
```

`make slides` also writes an image of each deck's title slide, and copies
`slides/index.html` to `site/slides/index.html`. That page lists the decks, so
`/slides/` on Pages shows them and not a 404. A new deck needs a card in
`slides/index.html` and a row in `docs/slides.md`.

`make slides` runs a pinned Marp CLI through `bunx`, so it needs bun. The PDF
also needs Chrome or Chromium on the machine. The `docs` workflow builds the
slides after the book, so Pages serves them at `/slides/`, and
`docs/slides.md` links each one.

Run `make docs` first and `make slides` after it: `mdbook build` empties
`site/`, and that removes the slides.

Each command and each output on a slide must be one that chap-devops really runs
or prints. Copy it from a real run, or from the chapter that it comes from.

## The build script

`build.rs` produces four things the compiler cannot work out on its own.

- `$OUT_DIR/embedded_files.rs`, the `(relative path, contents)` table that
  `src/registry/embedded.rs` includes. It is generated from
  `vendor/marketplace/registry.yaml` and the model files that index lists, in
  index order, so there is no hand-written list to keep in step. A file the
  index names but the directory does not hold fails the build with a message
  naming it; a file on disk the index does not list is a `cargo:warning` and
  is left out.
- `TARGET`, the triple this binary is built for, which `chap-devops self update`
  reads to pick its release asset and `chap-devops self version` prints.
- `GIT_REVISION`, the short commit of the checkout, empty when there is no git
  or no repository, which `chap-devops self version` prints when it is there.
- `CHAP_DEVOPS_BUILD_CHANNEL`, `stable` or `dev`, read from the environment variable
  of the same name. Only the exact value `dev` selects the dev channel, so
  nothing an environment happens to hold can end up in the compiled string,
  and a local build is stable without anyone having to say so.
  `.github/workflows/release.yml` sets it for a branch build. See
  [The dev channel](#the-dev-channel).

The script names its own `rerun-if-changed` files, so it re-runs when the
vendor directory or `.git/HEAD` moves and not otherwise, plus a
`rerun-if-env-changed` for `CHAP_DEVOPS_BUILD_CHANNEL`, so flipping the channel
rebuilds rather than handing back a cached binary from the other one.

## Vendoring the marketplace snapshot

The catalogue compiled into the binary is the last fallback when the registry
can be neither fetched nor read from cache, so `chap-devops` works on a machine that
has never had network access.

```sh
make vendor
```

runs `scripts/vendor-marketplace.sh`, which refreshes the files under
`vendor/marketplace/` and records the upstream commit in `SNAPSHOT`. It
records no fetch date, so a run against an unchanged upstream leaves the
directory as it was. A model added or dropped upstream needs nothing else:
`build.rs` reads the index and embeds what it lists, and `cargo test registry`
is what checks the result. Refresh when the marketplace changes and commit
the result; a snapshot change is compiled in, so it is a code change.

The vendored layout is the same as the on-disk cache layout, so
`vendor/marketplace/` and a cache entry are interchangeable.

The tests do not use this snapshot. The unit tests embed
`tests/fixtures/marketplace/`, a frozen copy from before the marketplace
removed CHAP-EWARS, and the CLI tests start with it in their registry cache.
So a refresh changes no test result. Change the frozen copy only on purpose,
with the reason in the commit.

## The documentation

The book is mdbook. `book.toml` at the repository root points `src` at `docs/`
and `build-dir` at `site/`, which is ignored by git.

```sh
make docs-serve    # http://localhost:3000, rebuilds as you write
make docs          # regenerate the reference, then build site/
```

`docs/SUMMARY.md` is the table of contents; a chapter that is not listed there
is not built. Every other file in `docs/` is a hand-written chapter, except
`docs/reference.md`.

## How the command reference is regenerated

`docs/reference.md` is generated from the `--help` texts, so a flag cannot
change without the chapter changing with it.

```sh
make docs-reference     # cargo run -q -- docs-markdown > docs/reference.md
```

`chap-devops docs-markdown` is a hidden subcommand (`hide = true`) implemented in
`src/commands/docs.rs`. It walks the static `Cli::command()` tree through
clap's introspection API and prints one `##` section per command path, with the
about text, a fenced usage line from clap's `render_usage`, and a table of
arguments and options.

Two details matter. It renders from the **static** tree rather than the one
`main.rs` parsed, because `main.rs` hides the project-only commands when there
is no project and the reference has to list all of them. And it skips clap's
own `-h` and `-V` by their *action* rather than their name, because
`chap-devops models enable --version` is a flag of ours that must stay.

The generated file is committed, and `cargo test` compares
`chap-devops docs-markdown` to it byte for byte. When that test fails, run
`make docs-reference` and commit the result. `make docs` depends on
`docs-reference`, so a local book build can never be stale.

To document a new flag: add it to its argument struct in `src/cli/` (or to
`src/cli.rs` for a global option) with a doc comment, run
`make docs-reference`, and mention it in whichever hand-written chapter it
belongs to.

## Continuous integration

`.github/workflows/ci.yml` runs the same three checks as `make check` plus
`make test` on Linux and macOS, on every push to `main` and every pull
request, and a shellcheck job over `install.sh` (as POSIX `sh`) and
`scripts/*.sh` (as bash). Windows is not in the matrix: the release still
builds the two Windows targets, but nothing tests them, and Windows users are
pointed at WSL with the Linux binary.

`.github/workflows/release.yml` builds release binaries for six targets
(Linux, macOS and Windows, on x86_64 and aarch64) when a `vX.Y.Z` tag is
pushed, fuses the two macOS builds into a seventh universal archive, and
attaches them all to a GitHub release with their SHA-256 sums. Every target is
built natively, so there is no cross-compilation toolchain to maintain, the
freshly built binary generates its own completion scripts into the archive,
and the Linux builds are static musl so one binary runs on any distribution. A
push to `main` runs the same seven builds and publishes them as the rolling
`dev` pre-release; a `workflow_dispatch` run exercises the build matrix
without publishing anything.

`.github/workflows/docs.yml` publishes the book to GitHub Pages on every push
to `main`. It regenerates `docs/reference.md` and fails if the result differs
from the committed file, which is the same check `cargo test` makes, then runs
`mdbook build` and deploys `site/`.

## Releasing

### Prerequisites

The macOS archives are unsigned: the dhis2-chap organisation has no Apple
Developer ID. The workflow signs them with a Developer ID certificate and
notarizes them when six repository secrets are set, and the release notes
then say so. `scripts/github-secrets.sh` sets them, reading each value from
1Password and piping it into `gh secret set` on stdin, so no secret is written
to disk or shown in a process list. All six are read before any of them is
uploaded, and a read that fails or comes back empty aborts the run before it
has set anything.

```sh
OP_VAULT="Private" OP_ITEM="Apple Developer ID" scripts/github-secrets.sh
```

The six are `APPLE_CERTIFICATE` (base64 of the exported `.p12`),
`APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`,
`APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`.
`--scope org` sets them on the organisation with `--visibility selected`
instead of on this repository alone; `--dry-run` shows what would be set.

The publish job also refuses a tag that does not match the version in
`Cargo.toml`, and publishes a tag with a suffix (`v1.0.0-rc.1`) as a
pre-release that never becomes `releases/latest`.

### Preparing the Apple certificate

`APPLE_CERTIFICATE` is the base64 of a `.p12` archive holding the Developer ID
private key, the certificate downloaded from the developer portal and Apple's
intermediate, so the chain in the archive is complete:

```sh
# the Developer ID certificate from the portal, DER, next to its private key
openssl x509 -inform DER -in developer_id.cer -out developer_id.pem

# Apple's Developer ID G2 intermediate, which the portal does not include
curl -fsSL -o DeveloperIDG2CA.cer https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer
openssl x509 -inform DER -in DeveloperIDG2CA.cer -out DeveloperIDG2CA.pem

openssl pkcs12 -export -legacy -inkey developer_id.key -in developer_id.pem -certfile DeveloperIDG2CA.pem -name "Developer ID Application: <name> (<TEAMID>)" -out developer_id.p12
```

`-legacy` is not optional. OpenSSL 3 exports with AES-256-CBC, PBKDF2 and an
SHA-256 MAC by default, and the `security` tool on the GitHub macOS runner
cannot read that: the import fails with

```text
security: SecKeychainItemImport: MAC verification failed during PKCS12 import (wrong password?)
```

with a password that is perfectly correct. `-legacy` writes the older encoding
`security` accepts. Exporting from Keychain Access instead of OpenSSL is also
fine; it already writes the compatible encoding.

Verify the archive locally before uploading it, in a throwaway keychain rather
than the login one:

```sh
security create-keychain -p "" /tmp/verify.keychain
security import developer_id.p12 -k /tmp/verify.keychain -P '<password>' -T /usr/bin/codesign
security find-identity -v -p codesigning /tmp/verify.keychain
security delete-keychain /tmp/verify.keychain
```

`find-identity` has to print exactly one valid identity; that line is what
`APPLE_SIGNING_IDENTITY` has to match. If it prints none, the runner will fail
in the same way, whatever the password is.

The base64 of that file is `APPLE_CERTIFICATE` and the password chosen on
export is `APPLE_CERTIFICATE_PASSWORD`:

```sh
base64 -i developer_id.p12 | pbcopy
```

### Cutting a release

```sh
make release-tag VERSION=0.2.0
```

which runs `scripts/release.sh`. It refuses to do anything unless the tree is
clean and the branch is `main`, bumps `version` in `Cargo.toml` and
`Cargo.lock`, runs `make check test`, commits `chore: release v0.2.0`, creates
an annotated `v0.2.0` tag, and then prints the push command rather than
pushing. Nothing reaches GitHub until you run it:

```sh
git push --atomic origin main v0.2.0
```

`--atomic` lands the branch and the tag together or not at all, so a rejected
branch never leaves a tag building a release. With `--push` the script also
fetches `origin/main` first and stops if it holds commits the local branch
does not.

`make release-tag VERSION=0.2.0 PUSH=1`, or `scripts/release.sh 0.2.0 --push`,
pushes straight away.

### What the workflow produces

The tag starts `release.yml`, which attaches eight files to the release:

- `chap-devops-universal-apple-darwin.tar.gz`, both macOS slices in one binary,
  unsigned unless the Apple secrets are set;
- `chap-devops-aarch64-apple-darwin.tar.gz` and `chap-devops-x86_64-apple-darwin.tar.gz`,
  likewise;
- `chap-devops-x86_64-unknown-linux-musl.tar.gz` and
  `chap-devops-aarch64-unknown-linux-musl.tar.gz`, static;
- `chap-devops-x86_64-pc-windows-msvc.zip` and `chap-devops-aarch64-pc-windows-msvc.zip`,
  unsigned and untested;
- `SHA256SUMS`, covering all seven archives.

Each archive holds one directory, `chap-devops-<version>-<target>/`, with the binary,
`README.md`, `LICENSE` and `completions/` holding `chap-devops.bash`, `_chap-devops`,
`chap-devops.fish` and `_chap-devops.ps1`, generated by running `chap-devops completions` on the
binary that was just built.

No archive name carries the version: the tag is already in the download URL,
one name per platform is what `releases/latest/download/<name>` needs to keep
the tables in the README and in [Install](./install.md) from going stale, and
`SHA256SUMS` makes a per-archive checksum file redundant. The directory inside
the archive does keep the version, because that is what someone sees after
unpacking it.

### The release notes

`scripts/release-notes.sh` writes the body of the release, and the workflow
does nothing but redirect it into `notes.md`. It takes the tag, and optionally
the tag to compare against when the default is wrong:

```sh
scripts/release-notes.sh v0.2.3         # what that release said
scripts/release-notes.sh v0.2.4         # what the next one will say
scripts/release-notes.sh dev            # the rolling pre-release
```

Running it on a checkout is the way to read the notes before the tag is
pushed, and it is why the text is a script rather than a heredoc in the
workflow.

A tagged release leads with `## What's changed`: the commit subjects between
the previous version tag and this one, grouped by their Conventional Commit
type, with a scope kept as a bold prefix and the `chore: release` commit left
out. The groups print in this order, and one with nothing in it is not printed
at all:

| Heading | Types |
| --- | --- |
| Features | `feat` |
| Fixes | `fix` |
| Performance | `perf` |
| Reverts | `revert` |
| Documentation | `docs` |
| Internal changes | `refactor`, `style`, `test` |
| CI and tooling | `ci`, `build`, `chore` |

What changed for a user comes first. A refactor is an internal change rather
than tooling: it is usually the groundwork a later feature sits on, and filed
under CI and tooling a reader scanning the notes takes it for build noise. A
type no group claims, and a subject that is not a Conventional Commit at all,
lands under Other changes rather than being dropped. Then a compare link, the
install one-liner and the checksum and signature checks. The `dev` shape keeps
the unstable header, lists the same grouped commits under
`## Since <last tag>`, and installs with `--version dev`.

The previous tag is `git describe --tags --abbrev=0 --match 'v[0-9]*'`. The
match is not optional: `dev` is a tag too, it sits on a `main` commit, and
without the glob it is what `describe` finds.

GitHub's own `generate_release_notes` is off. It lists merged pull requests,
and everything here is pushed straight to `main`, so all it ever added was a
"Full changelog" link. Both publish jobs check out with `fetch-depth: 0`, since
the notes need the history and the tags.

### The dev channel

Every push to `main` publishes the same seven archives as a GitHub
pre-release under the single moving tag `dev`, named `dev (rolling build of
main)`. It is the same build the tag path produces; what differs is which release it lands on and what the binary says
about itself.

- `CHAP_DEVOPS_BUILD_CHANNEL` is set at the workflow level to `stable` for a
  `refs/tags/v*` run and `dev` for everything else, so a dispatch build is
  never mistaken for a release build either. `build.rs` compiles it in and
  `chap-devops self version` prints it as `channel`.
- The version does not change: a dev build reports the Cargo version, which
  is the last tag. The commit is what moves, so `chap-devops self update` on a dev
  build compares the commit the release notes record against the build's own
  `GIT_REVISION` rather than comparing versions, and the once-a-day notice
  does the same. The line the commit is read back from is written by
  `scripts/release-notes.sh`; `commit` and the sha have to stay together in
  it, and a unit test in `src/selfupdate/tests.rs` checks that they do.
- A stable build never drifts onto a dev one: it follows
  `releases/latest`, which GitHub documents as "the most recent
  non-prerelease, non-draft release", and the notice checks `prerelease`
  itself on top of that. Crossing over is `chap-devops self update --version dev`,
  and `install.sh --version dev` installs one from scratch.
- The tag is moved by the workflow itself, with `git tag -f dev` and a force
  push, before the release is written. GitHub's create-a-release and
  update-a-release APIs both document `target_commitish` as unused once the
  tag exists, so leaving it to the release action would place the tag on the
  first rolling build and never move it again.
- `softprops/action-gh-release` overwrites an asset of the same name
  (`overwrite_files` defaults to true) and reuses the release the tag already
  has, so the eight files are replaced in place. It does not delete assets it
  was not handed, so a final step removes anything attached to the release
  that this build did not produce - a target that was renamed or dropped -
  after the upload rather than before it, which keeps the release from
  briefly missing an archive.
- `concurrency: release-<ref>` keeps one run going and one waiting per ref,
  and a newer push replaces the waiting one, so a burst of pushes to `main`
  builds the first and the newest commit. A run is never cancelled once it
  has started: one cut off while `publish-dev` uploads would leave the `dev`
  release with archives and `SHA256SUMS` from different commits. Every
  third-party action is pinned to a commit SHA, with its version in a comment.

The two publish jobs are guarded against each other: `publish` runs only for
`refs/tags/v*` and `publish-dev` only for a push to `refs/heads/main`.

### The install script

`install.sh` at the repository root is what
`curl -fsSL .../install.sh | sh` runs. POSIX `sh` rather than bash, so it works
on a minimal container image, and shellcheck-clean under `-s sh` in CI.

It resolves the platform from `uname -s` and `uname -m`, resolves the version
from `CHAP_DEVOPS_VERSION`, then the GitHub API, then the redirect of the
`releases/latest` page, downloads the archive and `SHA256SUMS` into a `mktemp`
directory, verifies with whichever of `sha256sum` and `shasum` exists, and
installs with `install -m 0755`. Completion scripts are copied into the
standard user directories that already exist, and a failure there never fails
the install.

`--version dev` needs nothing special: the tag goes into the download URL like
any other, and the binary inside the archive is found by matching
`chap-devops-*-<target>/chap-devops` rather than by spelling the directory out, because a
rolling archive carries `dev-<short commit>` where a release carries the tag.

`--here`, which `CHAP_DEVOPS_INSTALL_DIR=.` and `--dir .` also select, stops after
`install -m 0755 ./chap-devops`: it prints the path and the version and returns
without touching completion directories or saying anything about `PATH`. It is
the shape a CI job or a one-off trial wants, and the one mode that writes
nothing outside the current directory.

`CHAP_DEVOPS_DOWNLOAD_BASE` overrides where the archive and `SHA256SUMS` come from.
It is there to test the script against a locally built release and an install
never needs it. `curl` reads `file://`, so the whole path can be exercised
without a release:

```sh
target="$(uname -m | sed 's/arm64/universal/')-apple-darwin"
name="chap-devops-v0.0.0-test-$target"
mkdir -p /tmp/rel/"$name"
cp target/debug/chap-devops README.md LICENSE /tmp/rel/"$name"/
tar -czf /tmp/rel/chap-devops-"$target".tar.gz -C /tmp/rel "$name"
(cd /tmp/rel && shasum -a 256 chap-devops-"$target".tar.gz > SHA256SUMS)

CHAP_DEVOPS_DOWNLOAD_BASE=file:///tmp/rel \
  sh install.sh --version v0.0.0-test --dir /tmp/bin
```

`--dry-run` prints the platform, the archive URL and the target path and stops
there, which is the quickest check that the detection is right on a machine.

The signing steps live in `.github/actions/sign-macos`, a composite action
used by both the per-architecture build and the universal one, so the two
cannot drift apart. It imports the certificate into a throwaway keychain,
signs with `--options runtime --timestamp`, submits to `notarytool --wait` and
fails the job on any status other than `Accepted`, printing `notarytool log`
first. The universal binary is signed again after `lipo`, because fusing the
two binaries invalidates the per-slice signatures. Nothing is stapled: a ticket
can only be stapled to a bundle, a disk image or an installer package, so
Gatekeeper fetches it from Apple instead. The keychain is deleted in an
`if: always()` step in the calling job.

### Verifying a download

```sh
shasum -a 256 -c SHA256SUMS --ignore-missing
codesign -dv --verbose=4 chap-devops
```

The second prints `Authority=Developer ID Application: ...` and a secure
timestamp for a macOS binary. `spctl --assess --type execute` is not a useful
check here: it assesses app bundles and reports a plain command-line
executable as rejected even when the signature and the notarization are both
good. The same `codesign` output is written to the workflow's job summary, so
a release can be audited without downloading anything.

### The docs deploy

Pushing the release commit to `main` also runs `docs.yml`, which rebuilds the
book and deploys it to <https://dhis2-chap.github.io/chap-devops/>. There is
nothing to do by hand; the version in the install commands is written out in
`docs/install.md` and is updated there when it matters.

### Repository setup

`scripts/bootstrap-repo.sh` is the one-time setup for a fresh GitHub
repository. It prints every command and executes nothing until you pass
`--yes`:

```sh
scripts/bootstrap-repo.sh          # print the commands
scripts/bootstrap-repo.sh --yes    # run them
```

It creates `dhis2-chap/chap-devops` as a public repository from this checkout,
pushes `main`, switches GitHub Pages to the Actions build source (which has to
happen before `docs.yml` can deploy), sets the About link to the documentation
site, and runs `scripts/github-secrets.sh`.
