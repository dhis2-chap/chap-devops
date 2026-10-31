# Install

## The one-liner

On macOS and Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/dhis2-chap/chap-devops/main/install.sh | sh
```

Or, to get the binary and nothing else - into this directory as `./chap-devops`, no
`PATH` advice, no completion scripts, nothing written anywhere outside it:

```sh
curl -fsSL https://raw.githubusercontent.com/dhis2-chap/chap-devops/main/install.sh | sh -s -- --here
```

If the script printed `... is not on your PATH`, add the `export PATH=...`
line it printed to your shell profile and open a new shell.

Then run `chap-devops doctor` (`./chap-devops doctor` after `--here`): it checks in one
pass that this machine has everything a deployment needs - Docker, Compose
2.24.4 or newer, disk, and a route to the hosts Chap pulls from - and says what
to do about anything it did not find. See [Doctor](./doctor.md).

The script prints the release, the platform, the archive URL and the install
path, then the steps. On macOS with `--here` it ends like this:

```text
downloading chap-devops-universal-apple-darwin.tar.gz
verifying the checksum
unpacking

installed /Users/you/chap-devops-test/chap-devops
chap-devops 0.100.1
```

The script works out the platform from `uname`, downloads the release archive
for it, checks the download against the release's `SHA256SUMS` and installs the
binary. It refuses to install anything it could not verify.

Next to `chap-devops`, it puts the link `cdx`, the short form of the command: `cdx up`
is the same as `chap-devops up`. `chap-devops self update` replaces the binary, and the
link then points at the new one. `--here` installs the binary alone, with no
link.

If the install folder already has a `cdx` that is not this link, the script
leaves it alone and says so. Two small tools also install a command named
`cdx`: the `cdx` crate (text tools, with `cargo install cdx`) and the `cdx`
package on PyPI (directory bookmarks). If you need one of them, delete the
link and use `chap-devops`.

It goes into `/usr/local/bin` when that directory is writable and
`~/.local/bin` otherwise. It makes the directory if it is not there. If the
directory is not on your `PATH`, it prints the `export PATH=...` line to add to
your shell profile. Pipe it through `sudo sh` to take the first branch on a
machine where `/usr/local/bin` needs root.

Without `--here`, the script also copies the completion scripts into the
directories of bash, zsh and fish that already exist in your home (see
[Shell completions](#shell-completions)). This is also true with `--dir`. It
prints a `completions` line for each file it wrote. Only `--here` writes
nothing outside the current directory.

| Option | Environment variable | What it does |
| --- | --- | --- |
| `--version TAG` | `CHAP_DEVOPS_VERSION` | Install that release instead of the newest. `dev` is the rolling build of `main` |
| `--dir DIR` | `CHAP_DEVOPS_INSTALL_DIR` | Install `chap-devops` and `cdx` into that directory |
| `--here` | `CHAP_DEVOPS_INSTALL_DIR=.` | Put `./chap-devops` in the current directory and do nothing else |
| `--dry-run` | | Print the release, the archive and the install path, and change nothing |
| `--help` | | Print the options |

```sh
curl -fsSL https://raw.githubusercontent.com/dhis2-chap/chap-devops/main/install.sh |
  CHAP_DEVOPS_INSTALL_DIR="$HOME/bin" CHAP_DEVOPS_VERSION=v0.100.0 sh
```

The oldest release that has chap-devops archives is `v0.100.0`. For an older tag,
the script stops with `could not download ...`, and `chap-devops self update
--version` stops with an error that names the archive it did not find:

```text
error: v0.2.1 has no chap-devops-universal-apple-darwin.tar.gz, so chap-devops cannot install it on aarch64-apple-darwin; pick a newer tag with --version, or run `chap-devops self update` for the newest
```

Use `v0.100.0` or newer. Without `--version`, the error ends in `download a
build from https://github.com/dhis2-chap/chap-devops/releases`.
`--here` and `--dir` together are an error; use one of them.

Without `--version` the newest release is looked up through the GitHub API, and
through the redirect of the `releases/latest` page when the API rate limit has
been reached. Unauthenticated API calls are limited to 60 an hour per address,
which a CI runner can exhaust.

## Windows

On Windows, chap-devops runs inside WSL. Install WSL 2 and Docker Desktop, turn on
Docker Desktop's WSL integration for your distribution (Settings, Resources,
WSL integration), then open the WSL shell and follow the Linux steps above:
the one-liner installs the Linux binary, and `chap-devops doctor` checks that docker
answers from inside WSL.

The release also carries native Windows builds (`chap-devops-*-pc-windows-msvc.zip`
in the table below). They are published untested: CI runs on Linux and macOS
only. WSL with the Linux binary is the supported way on Windows.

## Which version

There are two series, and a binary knows which one it belongs to.

**Stable** is a `vX.Y.Z` tag: a release that was cut on purpose, with notes,
and it never changes once published. It is what the one-liner installs, what
`chap-devops self update` follows, and what the download links point at.

**dev** is the rolling build of `main`, republished under the single moving
tag `dev` on every push. It is the same seven archives, built and signed the
same way, from a commit that has passed CI and nothing more: no release notes
worth the name, no promise that anything in it stays. It reports the version
of the last tag, so `chap-devops --version` says `chap-devops 0.100.1` on a dev build as
well; `chap-devops self version` is what tells the two apart:

```text
  version       v0.100.1
  revision      b95828a
  channel       dev
  target        aarch64-apple-darwin
  path          /usr/local/bin/chap-devops
  installed by  release archive
```

`target` is the slice that runs. A universal macOS binary on Apple silicon
shows `aarch64-apple-darwin`.

`channel` is always printed, `stable` or `dev`, and is in `--json` under the
same name. `path` is the chap-devops file itself, also when you run it through the
link `cdx`. A binary built anywhere else, `cargo install` included, is stable.

Picking one with the installer:

```sh
# the newest stable release
curl -fsSL https://raw.githubusercontent.com/dhis2-chap/chap-devops/main/install.sh | sh
# the rolling build of main
curl -fsSL https://raw.githubusercontent.com/dhis2-chap/chap-devops/main/install.sh | sh -s -- --version dev
# one named release
curl -fsSL https://raw.githubusercontent.com/dhis2-chap/chap-devops/main/install.sh | sh -s -- --version v0.100.0
```

and with an installed `chap-devops`:

```sh
chap-devops self update                      # the newest build of the channel this one is on
chap-devops self update --version dev        # cross over to the rolling build
chap-devops self update --version v0.100.1   # go back to a stable release
```

`chap-devops self update` on a stable build follows `releases/latest`, which
excludes pre-releases, so a stable install is never moved onto a dev build by
itself. On a dev build the same command follows the `dev` release, and
compares the commit it was built from rather than the version number, since
the version does not move between two rolling builds. `--version TAG` always
wins over both.

## Downloading an archive

Each release attaches an archive per platform. These links always point at the
newest release:

| Platform | Download | Notes |
| --- | --- | --- |
| Linux x86_64 | [`chap-devops-x86_64-unknown-linux-musl.tar.gz`](https://github.com/dhis2-chap/chap-devops/releases/latest/download/chap-devops-x86_64-unknown-linux-musl.tar.gz) | static, any distro |
| macOS | [`chap-devops-universal-apple-darwin.tar.gz`](https://github.com/dhis2-chap/chap-devops/releases/latest/download/chap-devops-universal-apple-darwin.tar.gz) | universal, signed and notarized |
| Windows x86_64 | [`chap-devops-x86_64-pc-windows-msvc.zip`](https://github.com/dhis2-chap/chap-devops/releases/latest/download/chap-devops-x86_64-pc-windows-msvc.zip) | unsigned, untested |

<details>
<summary>Other platforms</summary>

| Platform | Download | Notes |
| --- | --- | --- |
| Linux arm64 | [`chap-devops-aarch64-unknown-linux-musl.tar.gz`](https://github.com/dhis2-chap/chap-devops/releases/latest/download/chap-devops-aarch64-unknown-linux-musl.tar.gz) | static, any distro |
| macOS, Apple silicon | [`chap-devops-aarch64-apple-darwin.tar.gz`](https://github.com/dhis2-chap/chap-devops/releases/latest/download/chap-devops-aarch64-apple-darwin.tar.gz) | signed and notarized |
| macOS, Intel | [`chap-devops-x86_64-apple-darwin.tar.gz`](https://github.com/dhis2-chap/chap-devops/releases/latest/download/chap-devops-x86_64-apple-darwin.tar.gz) | signed and notarized |
| Windows arm64 | [`chap-devops-aarch64-pc-windows-msvc.zip`](https://github.com/dhis2-chap/chap-devops/releases/latest/download/chap-devops-aarch64-pc-windows-msvc.zip) | unsigned, untested |

</details>

An archive is named after the platform alone, with no version in it: the
release it belongs to is the one in the URL, so a link written down today
still resolves after the next release. A release attaches these seven archives
and one `SHA256SUMS` covering all of them, and nothing else.

An archive holds one directory, `chap-devops-<version>-<target>/`, containing the
`chap-devops` binary, `README.md`, `LICENSE` and a `completions/` directory. In a
dev archive that version is `dev-<short commit>`, which is the one place the
commit shows up without running the binary.

The Linux builds are static musl binaries, so one file runs on any
distribution with no shared library to match. Linux and macOS ship as
`tar.gz`, Windows as `zip`.

On macOS take the universal archive unless you have a reason to pick a single
architecture: it carries the Apple silicon and Intel slices in one binary and
runs on any Mac.

On macOS:

```sh
curl -fsSLO https://github.com/dhis2-chap/chap-devops/releases/latest/download/chap-devops-universal-apple-darwin.tar.gz
tar -xzf chap-devops-universal-apple-darwin.tar.gz
sudo install -m 0755 chap-devops-*-universal-apple-darwin/chap-devops /usr/local/bin/chap-devops
```

On a Linux server:

```sh
curl -fsSLO https://github.com/dhis2-chap/chap-devops/releases/latest/download/chap-devops-x86_64-unknown-linux-musl.tar.gz
tar -xzf chap-devops-x86_64-unknown-linux-musl.tar.gz
sudo install -m 0755 chap-devops-*-x86_64-unknown-linux-musl/chap-devops /usr/local/bin/chap-devops
```

On an arm64 server, write `aarch64` in place of `x86_64` in the three lines.
The commands do not add the link `cdx`. To add it, run
`sudo ln -s chap-devops /usr/local/bin/cdx`.

## Verifying a download

Every release carries a `SHA256SUMS` file covering every archive:

Download it into the directory that has the archive, then check:

```sh
curl -fsSLO https://github.com/dhis2-chap/chap-devops/releases/latest/download/SHA256SUMS
shasum -a 256 -c SHA256SUMS --ignore-missing
```

It prints one line for the archive that is there:

```text
chap-devops-universal-apple-darwin.tar.gz: OK
```

On Linux, use `sha256sum -c SHA256SUMS --ignore-missing`; many distributions
have no `shasum`. The `sha256sum` of BusyBox (Alpine, small containers) has no
`--ignore-missing`. There, check the one line of your archive:

```sh
grep ' chap-devops-x86_64-unknown-linux-musl.tar.gz$' SHA256SUMS | sha256sum -c -
```

The macOS binaries are signed with an Apple Developer ID certificate and
notarized by Apple, so Gatekeeper lets them run: no right-click to open, no
`xattr -d com.apple.quarantine`. To see who signed one:

```sh
codesign -dv --verbose=4 /usr/local/bin/chap-devops
```

The `Authority=Developer ID Application: ...` line names the signer. To see
that Apple notarized it:

```sh
spctl -a -vvv -t install /usr/local/bin/chap-devops
```

It prints `accepted` and `source=Notarized Developer ID`.

Apple serves the notarization ticket online rather than it being stapled to
the file, because a bare executable has nowhere to carry a ticket. The first
run on a machine therefore wants to reach Apple once; after that the verdict
is cached.

The native Windows binaries are untested and not signed, which would need an
Authenticode certificate. SmartScreen may warn the first time `chap-devops.exe` runs;
"More info" then "Run anyway" gets past it. There `SHA256SUMS` is the check
that matters. Under WSL none of this applies: it is the Linux binary.

## Keeping it up to date

```sh
chap-devops self update           # replace this binary with the newest release
chap-devops self update --check   # say whether there is one, change nothing
chap-devops self version          # version, revision, channel, target, path, install method
```

`chap-devops self update` looks up the release, downloads the archive built for this
target (on macOS the universal one, whichever slice you installed), checks it
against `SHA256SUMS` and renames the new binary over the running one. The
installed file is never written through, so a download that fails or does not
verify leaves what you have exactly as it was.

`--version TAG` installs a named release, going backwards included, and `dev`
is a tag like any other. `--yes` skips the question
`replace ... with chap-devops vX.Y.Z? [y/N]`. chap-devops does not ask under `--json`, or
when stdin or stdout is not a terminal. `--json` reports the result as a
document, with `channel` alongside `current` and `latest`. If the binary lives
somewhere you cannot write, `chap-devops` says so and suggests `sudo` or an install
directory of your own rather than half-replacing itself.

A successful update prints:

```text
updated chap-devops: v0.100.0 -> v0.100.1
```

With `-v`, a hint names the file: `hint: replaced /usr/local/bin/chap-devops`.

`--check` on a binary that is up to date prints
`chap-devops v0.100.1 is up to date`. With `--version dev` or another tag, `--check`
reports that release:

```text
chap-devops dev b95828a (2026-10-07) is available (you have v0.100.1)
hint: the download is chap-devops-universal-apple-darwin.tar.gz
hint: `chap-devops self update --version dev` installs it
```

The hints come with `-v`. They name the same `--version TAG` that was
checked, because a plain `chap-devops self update` follows the channel of this
build. The day of a dev build is the day it was built, from the release notes
or the newest asset. If chap-devops finds no date, the line says `chap-devops dev b95828a`
with no day.

Without `--version`, an update follows the channel this build is on: the
newest tag for a stable build, the newest build of `main` for a dev one. See
[Which version](#which-version).

Once a day, after a command that succeeded, `chap-devops` asks the release feed
whether there is a newer version and prints one dimmed line on stderr if there
is:

```text
chap-devops v0.100.1 is available (you have v0.100.0): run `chap-devops self update`
```

On a dev build the same notice compares commits, because the version number
does not move between two rolling builds:

```text
a newer chap-devops dev build is available (commit 9f3a2c1, you have bff294c): run `chap-devops self update`
```

It stays quiet unless it can see that the commit moved, so a build with no
recorded revision is never nagged about nothing.

The check has a two-second timeout, its answer is cached in
`self-update-check.json` in the cache directory (`$CHAP_DEVOPS_CACHE_DIR`, else
`$XDG_CACHE_HOME/chap-devops`, else `~/.cache/chap-devops`, also on macOS), and any failure
is ignored: it can never
change what a command did or what it exited with. It is skipped under `--json`,
under `--offline`, when stdout is not a terminal, and for `chap-devops self` itself.
`CHAP_DEVOPS_NO_UPDATE_CHECK=1` turns it off entirely.

A `chap-devops` installed with `cargo install` is replaced the same way, but
`cargo install --path .` from an updated checkout is the more honest way to
move that one on. `installed by` in `chap-devops self version` tells which one you
have:

- `release archive` for a binary that the release workflow built, wherever it
  is now.
- `cargo install` for a local build in a `.cargo/bin` directory.
- `cargo build` for a local build in a `target/release` or `target/debug`
  directory.
- `local build` for a local build at any other path, also for `make install`
  and for `cargo install --root DIR`.

## Shell completions

Every release archive ships the scripts for bash, zsh, fish and PowerShell
under `completions/` (`chap-devops.bash`, `_chap-devops`, `chap-devops.fish`, `_chap-devops.ps1`). The
install script copies the first three into place when the directory a shell
reads already exists:

- `~/.local/share/bash-completion/completions/chap-devops` (or under
  `$XDG_DATA_HOME`)
- `~/.zsh/completions/_chap-devops`
- `~/.config/fish/completions/chap-devops.fish` (or under `$XDG_CONFIG_HOME`)

When the script adds the link `cdx`, it also writes the bash and fish script a
second time under the name `cdx` (`completions/cdx` and `cdx.fish`). bash and fish
load a completion file by the name of the command, so `cdx` then completes
before `chap-devops` was completed once. zsh needs nothing more, because `_chap-devops`
starts with `#compdef chap-devops cdx`.

`chap-devops completions <shell>` prints one to stdout for the cases it does not
cover: a checkout, a different directory, or a shell that reads its completions
from somewhere else. The shells are `bash`, `zsh`, `fish`, `powershell` and
`elvish`. The scripts complete `chap-devops` and the link `cdx`.

bash, with the `bash-completion` package installed (on macOS,
`bash-completion@2` from Homebrew):

```sh
mkdir -p ~/.local/share/bash-completion/completions
chap-devops completions bash > ~/.local/share/bash-completion/completions/chap-devops
chap-devops completions bash > ~/.local/share/bash-completion/completions/cdx
```

zsh, into a directory that is on your `fpath`. If it is a new directory, add
`fpath=(~/.zsh/completions $fpath)` before `compinit` in `~/.zshrc`:

```sh
mkdir -p ~/.zsh/completions
chap-devops completions zsh > ~/.zsh/completions/_chap-devops
```

fish:

```sh
mkdir -p ~/.config/fish/completions
chap-devops completions fish > ~/.config/fish/completions/chap-devops.fish
chap-devops completions fish > ~/.config/fish/completions/cdx.fish
```

PowerShell, appended to your profile:

```powershell
chap-devops completions powershell | Out-String | Invoke-Expression
```

elvish:

```sh
mkdir -p ~/.config/elvish/lib
chap-devops completions elvish > ~/.config/elvish/lib/chap-devops.elv
```

Then add `use chap-devops` to `~/.config/elvish/rc.elv`.

The scripts are generated from the same command tree `--help` is rendered
from, so they are current for the binary that printed them. Regenerate them
after an update.

## From a checkout

A checkout needs Rust 1.91 or newer. In the root of the checkout, run:

```sh
cargo install --path .
```

This builds the binary and puts it in `~/.cargo/bin` (or `$CARGO_HOME/bin`).
`cargo build --release` builds it into `target/release/chap-devops` and installs
nothing.

Or build and install through the Makefile, which puts the binary in
`$PREFIX/bin` with `PREFIX` defaulting to `~/.local`:

```sh
make install
```

To install into another directory, set `PREFIX`, for example
`make install PREFIX=/usr/local`.

On macOS `make release` builds a universal (arm64 plus x86_64) binary at
`bin/chap-devops` first, and `make install` copies that. It needs the two Rust
targets; if one is missing, it stops and prints the command to add it:
`rustup target add aarch64-apple-darwin x86_64-apple-darwin`. On Linux it
builds the host binary. `make install` also adds the link `cdx`, as the install script does;
`cargo install` installs `chap-devops` alone. See [Development](./development.md) for
the rest of the targets.

chap-devops is released as binaries on GitHub, not on crates.io: `publish = false` in
`Cargo.toml` makes a `cargo publish` fail at once.

## Requirements

On the machine that runs Chap: Docker, with Compose v2.24.4 or newer. That
version is where the `!override` YAML tag arrived, and every `compose.chap-devops.yml`
`chap-devops sync` writes uses it to replace chap-core's own port mapping rather than
add to it; see [Ports](./ports.md). The other requirement is older: `include:`,
which `chap-devops` uses for the umbrella file that names one overlay per enabled
model, arrived in 2.20.

Nothing else is required. No Python, no `uv`, no checkout of chap-core. This
is also true for chap-core's own `chap` CLI: `chap-devops chap` runs it in a
container. See [The chap CLI](./chap-cli.md). The same is true for chapkit's
CLI: `chap-devops chapkit` runs it in the chap-core image. See
[The chapkit CLI](./chapkit-cli.md).

`chap-devops` checks the installed Compose version and warns on stderr when it is
older than 2.24.4 rather than failing, because an older Compose still runs most
of Chap. Between 2.20 and 2.24.4 the model overlays load and the API ends up
published on two ports, its own and the override's; below 2.20 the model
overlays are the part that will not load at all.

## A note on the name

The binary is `chap-devops`, not `chap`, because chap-core's own Python package
already installs a console script called `chap`. The two never collide, so a
machine can have both.
