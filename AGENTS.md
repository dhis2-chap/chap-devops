# Working on chap-devops

This file is for changing chap-devops. If you are helping someone use chap-devops rather
than change it, read `docs/ai.md` instead and follow it.

chap-devops is a Rust CLI (crate and binary `chap-devops`) that deploys and
manages Chap (climate-informed disease forecasting) and the services around
it, all through docker compose: chap-core, model services from the Chap model
marketplace, the Open Climate Service (OCS) with an S3 store, and DHIS2. They
deploy together or any of them on its own; chap-core is one component among
the others, and a deployment without it is a first-class shape, not a
degraded one. `chap-devops init` writes a self-contained deployment directory;
everything else operates on it.

Read `docs/development.md` for the build, test, docs and release mechanics.
This file holds the rules that are not derivable from the code.

## Wording

- Write in ASD-STE100 Simplified Technical English. This applies to help,
  messages, docs, commit subjects, and to the agent's own replies in the
  session. Two modes:
  - Strict for help strings, messages, and instructions (`docs/ai.md`, the
    steps in a chapter). Apply every rule below.
  - STE-flavored for explanatory prose in `docs/`, commit bodies, and PR
    text. Apply every rule, but the word rules are advice, not law.

  The rules to keep:
  - One word, one meaning. Use the simple, common word ("use", not
    "utilize"; "start", not "initiate"; "make sure", not "ensure").
    Technical names (commands, files, options, products) stay as they are.
  - One name for one thing. Do not call one thing "deployment", then
    "project", then "directory" in the same text.
  - No phrasal verbs. Write "start", not "spin up"; "remove", not "take off".
  - Keep the modality. Do not change "may have failed" to "failed". A
    message claims only what chap-devops checked.
  - Sentences of at most 20 words in instructions and 25 words in
    descriptions. Paragraphs of at most six sentences.
  - Active voice and simple tenses: present, past, future.
  - Instructions in the imperative, one action per sentence, the condition
    first: "If the port is in use, set `CHAP_API_PORT` in `.env`."
  - Keep articles ("the", "a"). No noun clusters of more than three words.
  - No "-ing" words as nouns or adjectives where a simple form does the job.
  - Lists for steps and for more than two items.

  Change existing text to STE when you edit it for another reason; do not
  rewrite files only for STE.
- The product is "Chap" or "chap-core". The tool is `chap-devops`; the section "The
  name" in `docs/introduction.md` explains the name. Never write "stack" in
  anything a user sees: help, messages, docs.
  Do not describe Chap as a DHIS2 product; it is often deployed with DHIS2, not
  always. Do not expand the name: chap-core and the docs already use two
  expansions, and a third adds confusion. Where a short description is
  necessary, write "Chap (climate-informed disease forecasting)".
- Every `--help` string is one clause of about 70 characters. Explanations go
  in `docs/`, never in `long_about`, never as paragraphs on args. Global
  options sit under `help_heading = "Global options"`.
- Every command reports what it did. No command exits silently, not on
  success and not when there is nothing to do. Verify before claiming a state
  ("up" means chap-core answered, not that compose returned).
- A command prints its closing lines through `Out::report` (or `report_ok`),
  and each line has a level (`src/output/report.rs`, `docs/status.md`):
  - `info`: what the command did or found, and a step the reader must do
    next. Keep it short: one line for a simple result.
  - `warning`: something that needs attention; it goes to stderr.
  - `hint`: background and optional next commands. Shown only with `-v`.
  `--json` carries every line in `messages`, with its level. Do not color
  parts of a sentence; color is for tables, status words and the `warning:`
  label. `-vv` is the trace on stderr, `-d` adds the response bodies.
- No emojis anywhere: code, comments, commits, docs, output.
- Messages name the file or command the reader must go to, in backticks, and
  give the way out on the same line: `port 8700 is already in use on this
  machine (needed by chap); free it, or set CHAP_API_PORT=8701 in \`.env\``.

## Code

- Rust 2024, clap derive, serde_yaml_ng, ureq 3 (blocking), ratatui. Keep the
  binary static and dependency-light; a server install is Docker plus this
  one file.
- chap-devops is tested on Linux and macOS; the Windows builds are published
  untested, and Windows users are pointed at WSL with the Linux binary.
- A code file stays under about 600 lines; past that it becomes a folder
  module split by concern (`foo.rs` + `foo/<concern>.rs`, never `mod.rs`).
  Unit tests always go in `foo/tests.rs` behind `#[cfg(test)] mod tests;`,
  never inline.
- State lives in `.chap-devops/` as YAML (`project.yaml`, `models.yaml`,
  `components.yaml`, `models-manual.yaml`); the compose files next to it are
  rendered from that state by `chap-devops sync` and carry the one-line header
  `# Generated by chap-devops from .chap-devops/; edit there and run \`chap-devops sync\`.`.
  Never make a command edit a generated file directly; change the state and
  re-render.
- `.env` is written once by `init` and never rewritten (`--fresh-env` is the
  exception); the database password in it outlives everything else.
- Upstream's `compose.yml` is kept byte for byte at the pinned chap-core tag;
  chap-devops changes behaviour through `compose.chap-devops.yml` (`!override`, needs
  Compose 2.24.4) and per-model overlays.
- Marketplace model images are amd64-only, and so is chap-core, so every
  model overlay pins `platform: linux/amd64` (`src/compose/spec.rs`), a model
  added by URL or image too. The service user comes from the image config
  (`src/compose/resolve.rs`),
  not from assumptions; a root image gets no `user:` line, and its init
  container chowns to `0:0` like every other model's.
- Talk to chap-core through `src/api.rs` (base URL from the project, Bearer
  token from `.env`, `CHAP_API_TOKEN` as the fallback). Never print a token.
- Anything that needs the network or docker must degrade: `--offline` works
  from the cached or embedded marketplace, and a missing docker CLI is a
  reported skip, not a panic.

## Tests

- `make check` (fmt + clippy with `-D warnings`) and `make test` must be green
  before a change is reported done. Add tests with the change: unit tests next
  to the code, CLI tests in `tests/cli/` (a module per concern, shared
  helpers in `tests/cli/common/`) against the routed local HTTP
  server that stands in for the marketplace, GitHub, ghcr and chap-core
  (`--registry-url`, `CHAP_DEVOPS_GITHUB_API`, `CHAP_DEVOPS_GHCR_URL`), and the DHIS2
  stand-in in `tests/cli/dhis2_stand_in.rs`.
- Tests never depend on the machine: no fixed low ports (use 18xxx), no real
  docker unless gated by `docker_ready()`, `CHAP_DEVOPS_NO_DOCKER_PROBE=1` in the
  sandbox, paths compared as `PathBuf` rather than as strings (the Windows
  builds still ship, untested, and need separator-agnostic code), and no
  browser window (`chap-devops open` only with `--no-browser` or `--json`).
- Golden files under `tests/fixtures/` are the rendered compose files; update
  them deliberately, with the reason in the commit.
- `docs/reference.md` is generated from the clap help by `make
  docs-reference`; regenerate it whenever a help string changes and never edit
  it by hand.

## Testing what chap-devops deploys, and BUGS.md

The main purpose of chap-devops is to run Chap and the systems around it for
real, so that their bugs show up. Test every shape a user deploys, not only
the full one:

- chap-core with marketplace models, and with the built-in baselines;
- chap-core with chapkit: `chapkit mlproject run`, `migrate` and `export`, and
  `chap eval` with `--run-config.as-chapkit`;
- OCS on its own, with S3, read-only and behind `--base-url`;
- DHIS2 on its own, at every supported version, and connected to Chap with
  the Modeling App;
- the combinations, and the development options in `docs/ai.md`.

Test the newest images: OCS publishes `main` and no releases, so pull it
first. Write down the versions and the image digests that you tested.

Record each bug in a deployed system in `BUGS.md` as you find it, not at the
end: the Modeling App, DHIS2 and its demo databases, chap-core, chapkit and
servicekit, OCS, and the chap-models. Each entry says where it was seen (the
versions), what happens (the exact message), and what would fix it. A bug
that is fixed upstream leaves the file; do not keep "fixed" entries. A bug in
chap-devops itself is not a `BUGS.md` entry: fix it, with a test.

## Docs

mdbook, `book.toml` at the root, chapters in `docs/`, one chapter per
concern (`models.md`, `components.md`, `ports.md`, `jobs.md`, ...), a row per
command in `docs/commands.md`, and `docs/troubleshooting.md` entries keyed by
the exact message or log line a user will search for. When a command gains an
option, the docs chapter gets the explanation the help text is not allowed to
carry.

`docs/ai.md` is what an AI assistant follows to set chap-devops up for someone,
command by command. A change to a command or message it uses updates its
steps and its "It worked when" lines in the same commit.

## Commits and releases

- Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:`, `refactor:`), one
  concern per commit, no attribution lines of any kind.
- Commits are signed through the 1Password SSH agent; "failed to write commit
  object" means 1Password is locked.
- `scripts/release.sh X.Y.Z --push` bumps, verifies, tags and pushes; the tag
  starts the release workflow (seven archives, an unsigned macOS universal
  binary, `SHA256SUMS`; it signs and notarizes macOS only when the Apple
  secrets are set). Every push to main also publishes the `dev`
  pre-release. Release notes are generated from the commit types by
  `scripts/release-notes.sh`, so commit subjects are the changelog.

## Reference checkouts

Checkouts on this machine that are the source of truth when something is
unclear: `~/dev/dhis2-chap/chap-core` (compose.ghcr.yml, the REST API),
`~/dev/dhis2-chap/chapkit` (model service framework, `chapkit test`, the
sample-data route), `~/dev/dhis2-chap/open-climate-service` (OCS: `read_only.py`
for what read-only closes, `shared/urls.py` for `CLIMATE_SERVICE_BASE_URL`,
`plugins/datasets/` for which dataset needs which credential),
`~/dev/chap-models/*` (every marketplace model), and the marketplace registry
at github.com/dhis2-chap/model-marketplace (YAML, vendored under
`vendor/marketplace/`).
