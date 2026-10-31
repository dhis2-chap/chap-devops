# chap-devops

[![ci](https://github.com/dhis2-chap/chap-devops/actions/workflows/ci.yml/badge.svg)](https://github.com/dhis2-chap/chap-devops/actions/workflows/ci.yml)
[![release](https://img.shields.io/github/v/release/dhis2-chap/chap-devops)](https://github.com/dhis2-chap/chap-devops/releases/latest)
[![license](https://img.shields.io/badge/license-AGPL--3.0-blue)](LICENSE)
[![docs](https://img.shields.io/badge/docs-dhis2--chap.github.io%2Fchap--devops-informational)](https://dhis2-chap.github.io/chap-devops/)

> **AI agents and LLMs helping someone use chap-devops: read
> [dhis2-chap.github.io/chap-devops/ai.html](https://dhis2-chap.github.io/chap-devops/ai.html)
> ([`docs/ai.md`](docs/ai.md)) first.**

[Chap](https://chap.dhis2.org) forecasts
cases of climate-sensitive diseases such as dengue and malaria from past case
counts, climate data and population, and scores how well each forecasting model
would have done on the past before anyone relies on it. It is often used
together with [DHIS2](https://dhis2.org), through the Modeling App.

`chap-devops` deploys and manages Chap and the services around it, as Docker Compose
deployments: [chap-core](https://github.com/dhis2-chap/chap-core), the forecasting model
services published in the
[Chap model marketplace](https://github.com/dhis2-chap/model-marketplace),
[Open Climate Service](https://github.com/dhis2/open-climate-service) with an
S3 store, and [DHIS2](https://dhis2.org). They deploy together or any of them
on its own. One command writes a self-contained deployment directory, and after
that `chap-devops` is a thin wrapper around `docker compose` that always passes the
explicit `-f` list, plus a model manager that can add or remove models without
you hand-editing YAML. A machine that runs it needs Docker and one binary: no
Python, no uv, no checkout of chap-core.

## Use cases

An AI agent helping someone starts at [`docs/ai.md`](docs/ai.md): a short
explanation of Chap, a question that finds the right option, and for each
option the exact commands and how to tell that it worked. The pages below are
the detail behind each option.

To learn Chap by clicking through it, start with
[Chap with a local DHIS2 and the Modeling App](https://dhis2-chap.github.io/chap-devops/use-cases/chap-with-local-dhis2.html)
and then
[Your first forecast in the Modeling App](https://dhis2-chap.github.io/chap-devops/modeling-app.html):
an evaluation and a three-month dengue forecast on demo data from Laos.

- [Chap with forecasting models](https://dhis2-chap.github.io/chap-devops/use-cases/chap-with-models.html): chap-core and the forecasting models it runs. The default.
- [Chap with climate data from OCS](https://dhis2-chap.github.io/chap-devops/use-cases/chap-with-ocs.html): chap-core with Open Climate Service beside it for climate data.
- [Chap with a local DHIS2 and the Modeling App](https://dhis2-chap.github.io/chap-devops/use-cases/chap-with-local-dhis2.html): chap-core, models and a demo DHIS2, connected for the Modeling App.
- [Chap for a DHIS2 that runs elsewhere](https://dhis2-chap.github.io/chap-devops/use-cases/chap-for-external-dhis2.html): chap-core on a server, used by a DHIS2 that runs elsewhere.
- [An OCS server on its own](https://dhis2-chap.github.io/chap-devops/use-cases/ocs-alone.html): Open Climate Service and its object store, no Chap.
- [A DHIS2 on its own](https://dhis2-chap.github.io/chap-devops/use-cases/dhis2-alone.html): A DHIS2 and its database, nothing else; 2.41, 2.42, 2.43 or the next one.
- [A chapkit model service on its own](https://dhis2-chap.github.io/chap-devops/use-cases/model-alone.html): One chapkit model service answering on its own port, no chap-core.
- [Several model services side by side](https://dhis2-chap.github.io/chap-devops/use-cases/models-alone.html): Several model services, each on its own port, no chap-core.
- [Any number of model services, no folder](https://dhis2-chap.github.io/chap-devops/use-cases/models-with-run.html): one `chap-devops run` per model, each on its own port.
- [Your model from its checkout, with Chap](https://dhis2-chap.github.io/chap-devops/use-cases/model-on-host.html): chap-devops runs chap-core; you run your model with `uv run` and it registers.
- [chap-core from its checkout, with the models](https://dhis2-chap.github.io/chap-devops/use-cases/chap-core-on-host.html): you run chap-core; chap-devops runs the models and registers them with it.
- [chap-core built from its checkout](https://dhis2-chap.github.io/chap-devops/use-cases/chap-core-from-checkout.html): chap-devops builds chap-core from your clone and runs it with everything else.
- [A DHIS2 you run yourself, with Chap from chap-devops](https://dhis2-chap.github.io/chap-devops/use-cases/dhis2-dev-with-chap.html): your DHIS2 and a chap-core from chap-devops, connected.
- [A DHIS2 from chap-devops, with a chap-core elsewhere](https://dhis2-chap.github.io/chap-devops/use-cases/dhis2-with-chap-core-elsewhere.html): a DHIS2 from chap-devops and your chap-core, connected.
- [A new model of your own](https://dhis2-chap.github.io/chap-devops/use-cases/new-model.html): `chap-devops models new` makes a model project in Python or R, and Docker builds it.
- [An MLproject model as a chapkit service](https://dhis2-chap.github.io/chap-devops/chapkit-cli.html#migrate-an-mlproject-model): `chap-devops chapkit mlproject migrate` changes the model, and Docker builds it, with no Python or uv.
- [A model image you built yourself](https://dhis2-chap.github.io/chap-devops/use-cases/local-model-image.html): `docker build` a model and run it in Chap without publishing it.
- [Evaluating a model on your own data](https://dhis2-chap.github.io/chap-devops/use-cases/evaluate-with-chap-cli.html): `chap eval` on your CSV through `chap-devops chap`, with no Python or uv.
- [Combinations](https://dhis2-chap.github.io/chap-devops/use-cases/combinations.html): OCS and DHIS2 without Chap, a model beside OCS, everything at once.
- [Several deployments on one machine](https://dhis2-chap.github.io/chap-devops/use-cases/several-deployments.html): taking turns on the default ports, or giving each its own.
- [Growing a deployment](https://dhis2-chap.github.io/chap-devops/use-cases/growing.html): Moving a deployment from one shape to another.

Licensed under the AGPL-3.0, like chap-core.

## Install

On macOS and Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/dhis2-chap/chap-devops/main/install.sh | sh
```

Or `... | sh -s -- --here` for the binary alone, as `./chap-devops` in the current
directory and nothing else.

That works out the platform, downloads the release archive for it, checks it
against the release's `SHA256SUMS` and installs the binary into
`/usr/local/bin` when that is writable and `~/.local/bin` otherwise.
`CHAP_DEVOPS_INSTALL_DIR=/somewhere sh` or `--dir` picks the directory,
`CHAP_DEVOPS_VERSION=v0.2.0` or `--version` picks the release, and `--dry-run` says
what it would do. Shell completions are installed alongside when the
directories a shell reads already exist.

Or download an archive and put `chap-devops` on your `PATH` yourself. These links
always point at the newest release:

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

An archive holds one directory, `chap-devops-<version>-<target>/`, with the binary,
`README.md`, `LICENSE` and shell completions. The name of the archive carries
no version, so the links above keep working after the next release, and
`SHA256SUMS` in the release covers every one of them.

The macOS binaries are signed with an Apple Developer ID certificate and
notarized, so Gatekeeper does not block them. The Windows binaries are
unsigned and published untested; on Windows, run the Linux binary inside WSL 2
with Docker Desktop's WSL integration (see the install chapter).

Once installed, `chap-devops self update` replaces the binary with the newest
release, `chap-devops self version` says which build this is, and `chap-devops completions
<bash|zsh|fish|powershell|elvish>` prints a completion script.

Or, from a checkout, `cargo install --path .` or `make install`. Requires
Docker with Compose v2.24.4 or newer. See
[Install](https://dhis2-chap.github.io/chap-devops/install.html).

## Quickstart

```sh
chap-devops doctor                         # is this machine ready: docker, compose, disk, network
chap-devops init mychap --models chapkit_simple_multistep_model   # writes the deployment directory
cd mychap
chap-devops up                             # sync the compose files, docker compose up -d
chap-devops status                         # chap-core health and registered models
chap-devops ui                             # browse the marketplace, toggle models
chap-devops up                             # apply what the browser changed
chap-devops models add https://github.com/<org>/<model>   # a model not in the marketplace
```

chap-core's API is on <http://localhost:8700>; model services are reached
through it, or given a port of their own with `chap-devops models expose ID`. Every
command takes `--json`, and every command ends with a line saying what it did.

## The three words to remember

- **`chap-devops up`** starts what is on disk. It never moves a pin. Only
  `chap-devops up --pull` downloads a newer build, and only for an image on a moving
  tag such as the `main` tag of OCS.
- **`chap-devops update`** fetches newer versions: it asks the marketplace and the
  chap-core release feed what they publish today, moves the pins and pulls the
  images. It never touches a container; it says which ones are now out of date.
- **`chap-devops restart`** applies them, recreating the running services whose image
  or configuration changed and leaving the rest alone.

## Development

```sh
make test      # run the test suite
make check     # formatting and clippy, fixing nothing
make release   # universal (arm64+x86_64) macOS binary at bin/chap-devops
make install   # copy bin/chap-devops to $PREFIX/bin (PREFIX defaults to ~/.local)
make vendor    # refresh the embedded marketplace snapshot
make docs      # build the documentation into site/
make docs-serve  # serve it locally at http://localhost:3000
```

`make help` lists every target. The underlying commands are `cargo test`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt --all --check` and
`scripts/vendor-marketplace.sh`.

CI runs the same three checks on Linux and macOS, and shellcheck over
`install.sh` and `scripts/`. Tagging `vX.Y.Z` builds release binaries for six
targets (Linux, macOS and Windows, on x86_64 and aarch64), fuses the two macOS
builds into a seventh universal archive, and attaches all seven to a GitHub
release together with one `SHA256SUMS`, each archive carrying the shell
completion scripts. `make vendor` refreshes the embedded marketplace snapshot;
no workflow does it on a schedule. A weekly workflow runs `scripts/e2e.sh`,
which starts OCS, chap-core, DHIS2 and the models for real.

The documentation lives in `docs/` and is built with
[mdbook](https://rust-lang.github.io/mdBook/); `docs/reference.md` is generated
from the `--help` texts by `make docs-reference` and checked by `cargo test`.
