# Concepts

## The project directory

`chap-devops init` writes a directory you own; nothing outside it is touched.

```text
mychap/
  .chap-devops/
    project.yaml               intent: schema version, chap-core tag and compose
                               source, the compose project name, registry URL,
                               API port, whether the two auth secrets are in
                               use, the -f list, the model port range, and
                               which root files sync wrote
    models.yaml                intent: the enabled models (image, pinned version,
                               channel, host port or none, data dir, user and
                               where it was read from, platform, overlay name)
    models-manual.yaml         intent: the models this deployment defines itself,
                               added by `chap-devops models add`; only present when it
                               has added one
    components.yaml            intent: which components this deployment is made of
                               (chap-core, ocs, s3, dhis2) and their settings
    compose.chap-core.<tag>.yml
                               chap-core's own compose.ghcr.yml at the pinned tag,
                               exactly as downloaded; compose.yml is rendered from it
  compose.yml                  artifact: the base services, from the file above
  compose.chap-devops.yml            artifact: chap-devops-owned overrides on top of it: the
                               compose project name, the API's host port and the
                               registration key chap-core checks
  compose.ocs.yml              artifact: the ocs component, when it is enabled
  compose.s3.yml               artifact: the s3 component, when it is enabled
  compose.dhis2.yml            artifact: the dhis2 component, when it is enabled
  compose.marketplace.yml      artifact: include: list, one line per enabled model
  compose.<service_id>.yml     artifact: one overlay per enabled model
  ocs/climate-service.yaml     the OCS instance configuration; scaffolded once by
                               chap-devops, which changes only two keys in it
  dhis2/dhis.conf              the DHIS2 instance configuration; likewise, and DHIS2
                               does not start without it
  .env                         written once by init; only pins, the two auth
                               secrets and the component settings are ever added
```

| File | What it is |
| --- | --- |
| `compose.yml` | The base of a Chap deployment, which is chap-core, its worker and database plus the enabled models: chap-core, worker, Valkey and PostgreSQL, with the models added on top by their overlays. chap-core's own `compose.ghcr.yml` at the pinned tag, so upstream stays the source of truth. Rendered by `sync` from `.chap-devops/compose.chap-core.<tag>.yml`, or from the copy compiled into the binary when there is none. |
| `compose.chap-devops.yml` | The chap-devops-owned settings that sit on top of the base file: the compose project `name:`, the API's host port as `ports: !override`, `SERVICEKIT_REGISTRATION_KEY` plus `XDG_RUNTIME_DIR` in the `chap` service's environment, and the chap-devops labels on every service the base file defines ([Container labels](#container-labels)). It is a separate `-f` entry because a file in `include:` cannot override a service the main file defines. Rendered from `.chap-devops/project.yaml`. |
| `.chap-devops/compose.chap-core.<tag>.yml` | That upstream file as downloaded, one per tag the project has used. Deleting it does not break Chap; it only means `sync` can no longer re-render `compose.yml`. |
| `.env` | PostgreSQL credentials (the password is 32 random hex characters generated once), the chap-core image tag, `CHAP_API_PORT` (an active line even at the default 8700, so chap-core's published port is discoverable by reading the file), the two authentication secrets (`CHAP_API_TOKEN` and `SERVICEKIT_REGISTRATION_KEY`, active lines when the deployment is protected and commented placeholders when it is not), and commented placeholders for `CHAP_DATABASE_URL` and the per-model image pins. |
| `compose.ocs.yml`, `compose.s3.yml`, `compose.dhis2.yml` | One per enabled component other than chap-core, rendered from `.chap-devops/components.yaml`. They sit in the `-f` list between `compose.chap-devops.yml` and the umbrella, and are removed again when the component is disabled. See [Components](./components.md). |
| `ocs/climate-service.yaml` | The Open Climate Service instance configuration, scaffolded when the `ocs` component is first enabled. It is yours from that moment: `chap-devops` changes only its `read_only` and `plugins_dir` keys, and re-creates it only if it goes missing. |
| `dhis2/dhis.conf` | The DHIS2 instance configuration, on exactly the same terms - scaffolded once when `dhis2` is first enabled, never rewritten, re-created only if it goes missing. That last part matters more here: DHIS2 does not start without it. See [DHIS2](./dhis2.md#dhis2dhisconf). |
| `compose.marketplace.yml` | An umbrella file whose `include:` list names one overlay per enabled model. With no models enabled it holds `services: {}` instead of an empty `include`. It carries the project `name:` as well, because it is the one file that is always in the `-f` list. |
| `compose.<service_id>.yml` | One model service, rendered from its `models.yaml` entry. |
| `.chap-devops/project.yaml`, `.chap-devops/models.yaml`, `.chap-devops/components.yaml` | The intent, as above. All three open with one line saying `chap-devops` manages them. A deployment created before `components.yaml` existed reads as chap-core alone, which is what it was. |
| `.chap-devops/models-manual.yaml` | The definitions of the models this deployment added itself, one entry per `chap-devops models add`: what a marketplace file would have said about each. It is intent like the rest, and it is a definition rather than an enablement - `models.yaml` still says which models are on. Absent in a deployment that has added none. See [Models outside the marketplace](./models.md#models-outside-the-marketplace). |

## Intent and artifacts

`.chap-devops/` is the **intent**. It is what `models enable`, `models disable`,
`components enable`, `components disable`, the browser and `update` edit, and it
is small enough to read and to diff.

The compose files at the project root are **artifacts** rendered from it by
`chap-devops sync`. They are plain Compose files with nothing `chap-devops`-specific in
them, so

```sh
docker compose -f compose.yml -f compose.chap-devops.yml -f compose.marketplace.yml up -d
```

works without `chap-devops` installed, which is the point of generating them.

`compose.yml` is an artifact too. `init` downloads chap-core's own
`compose.ghcr.yml` at the tag it pinned, keeps the raw copy as
`.chap-devops/compose.chap-core.<tag>.yml` and records its URL and SHA-256 in
`project.yaml`; `sync` renders `compose.yml` as two header lines plus that copy,
byte for byte. Rendering from the copy rather than the network keeps `sync`
offline and deterministic, and it is why a hand edit of `compose.yml` is drift
that `chap-devops sync` undoes. Edit the copy in `.chap-devops/` instead, and `sync` will
follow it, and say that the checksum no longer matches. When the copy is
missing, `sync` leaves `compose.yml` alone rather than guessing.

### One change at a time

Every command that changes `.chap-devops/` takes an exclusive lock on
`.chap-devops/lock` before it reads the state and keeps it until it has written it
back: `models enable`, `disable`, `add`, `remove`, `expose`, the `components`
commands, `sync`, `update`, `restore`, `auth`, `dhis2 use`, `down`, and `up`
while it renders the compose files. A second such command on the same
deployment waits for the first, and says so on stderr:

```text
another chap-devops command is changing this deployment; waiting for it to finish
```

So two `models enable --port auto` started together, from two terminals or
from a tool that runs chap-devops for its users, end with both models enabled on
different ports. `up` lets go of the lock once the files are written, so a
long image pull does not hold up the next `models enable`. Commands that only
read - `status`, `models list`, `logs` - never wait. The operating system
releases the lock when the command exits, however it exits, so there is no
stale lock file to clear; `.chap-devops/lock` is left in place and is not part of a
backup.

## `chap-devops sync`

```sh
chap-devops sync
```

```text
in sync: 0 written, 5 unchanged, 0 removed
```

With `-v`, a hint line names each file:

```text
$ chap-devops -v sync
hint: compose.yml is unchanged
hint: compose.chap-devops.yml is unchanged
hint: compose.auto-arima-chapkit.yml is unchanged
hint: compose.chapkit-simple-multistep-model.yml is unchanged
hint: compose.marketplace.yml is unchanged
in sync: 0 written, 5 unchanged, 0 removed
```

`chap-devops up` runs `sync` before `docker compose up`, so the intent and the
artifacts never drift in normal use. Rendering is deterministic and every file
is compared before it is written, so a second sync reports everything as
unchanged.

- `chap-devops sync --check` reports drift without writing, for CI or a pre-commit
  hook, and exits non-zero when anything would change. It names each file
  that would change (`would write compose.yml`) without `-v`.
- A plain `chap-devops sync` re-creates a deleted overlay or picks up a hand edit of
  `models.yaml`.
- Sync only ever removes overlays it wrote itself; `project.yaml` keeps the
  list. A hand-written `compose.custom.yml` next to them is left alone. Add it
  to the umbrella by hand if you want it included.

### Orphans

Removing a definition does not stop a container: compose only knows about the
services the `-f` list names, and a container it no longer finds a definition
for is an **orphan**. `chap-devops` owns the whole `-f` list, so an orphan in one of
its projects is exactly that - the container of a model or a component that was
disabled, still running and still publishing its host port.

So `chap-devops up` and `chap-devops restart` both pass `--remove-orphans`, and `models
disable` and `components disable` go further: they stop and remove the
container of the service they are taking away there and then, while compose
still has the definition to name it by, and say so in their closing line. The
port is free the moment the command returns, rather than at the next `chap-devops
up`.

The flip side is the rule for hand-written compose files. A container is only
an orphan of the compose project it belongs to, and `chap-devops` renders its `-f`
list from `.chap-devops/`, so a service of your own is only at risk if you started it
**under this deployment's project name** - which is what

```sh
docker compose -f compose.yml -f compose.chap-devops.yml -f compose.marketplace.yml \
               -f compose.custom.yml up -d
```

does, because the `name:` comes from the `chap-devops` files in that list. The next
`chap-devops up` does not have `compose.custom.yml` in its own list, so it takes
those containers with it. Run a hand-written file as a compose project of its
own (its own directory, or `-p`) and nothing `chap-devops` does touches it.

## The compose project name

Compose puts a project name in front of every container and every named volume
it creates, and unless a file says otherwise that name is the **directory
name**. Two deployments in directories both called `demo` therefore share
`demo_chap-db`, `demo_logs` and the rest - on different paths, and even when
one of them was deleted months ago. A fresh `chap-devops init demo && chap-devops up` then
starts chap-core against a PostgreSQL volume created with a password it has
never seen, the container never becomes healthy, and `up` ends on

```text
dependency failed to start: container demo-chap-1 is unhealthy
```

So `chap-devops init` generates a name of its own and records it:

```yaml
compose_project: demo-1ab2c3
```

`chap-devops sync` renders it as the top-level `name:` of the files it owns
(`compose.chap-devops.yml` and `compose.marketplace.yml` - compose takes the `name:`
of the last `-f` file that sets one, and the umbrella is the one file always in
the list). Containers become `demo-1ab2c3-chap-1` and volumes
`demo-1ab2c3_chap-db`, which no other deployment can be holding.

The name never changes once it is written. `chap-devops init --force` keeps the one
it finds, because a new name would leave the running deployment's containers
and its data behind under the old one.

`chap-devops status --json`, `chap-devops doctor` and the closing line of `chap-devops down` all
name the project.

`COMPOSE_PROJECT_NAME` in the environment still wins over the `name:` key, as
it does for any compose project.

## Container labels

Every container `chap-devops` starts carries labels under `io.github.dhis2-chap.chap-devops`, so a
tool can find them all on a machine without knowing where any deployment
lives:

```sh
docker ps --filter label=io.github.dhis2-chap.chap-devops.role
```

lists every chap-devops container, of every deployment and every `chap-devops run` group,
and `chap-devops top` finds its containers the same way. The one-shot containers of
`chap-devops chap` and `chap-devops chapkit` have no role label; `label=io.github.dhis2-chap.chap-devops.kind` finds it too.

| Label | Value | On |
| --- | --- | --- |
| `io.github.dhis2-chap.chap-devops.role` | `chap-core`, `model`, `ocs`, `s3` or `dhis2` | Every container except the ones of `chap-devops chap` and `chap-devops chapkit`. `chap-core` covers chap, the worker, Valkey and PostgreSQL; `dhis2` covers its database and one-shots; `s3` its bucket one-shot. |
| `io.github.dhis2-chap.chap-devops.model` | The marketplace id, such as `chapkit_simple_multistep_model` | A model service and its `-init` container. |
| `io.github.dhis2-chap.chap-devops.kind` | `run` for a [`chap-devops run` group](./run.md#where-it-runs-groups), `init` for a deployment `chap-devops init` wrote, `cli` for the one-shot containers of `chap-devops chap` and `chap-devops chapkit` | Every container. |
| `io.github.dhis2-chap.chap-devops.group` | The group name | Every container of a `chap-devops run` group, and nothing else. |

The model and component files carry the labels themselves. chap-core's
services get theirs from `compose.chap-devops.yml`, since `compose.yml` is upstream's
file byte for byte and compose merges `labels` across the `-f` files; a
chap-core built from a checkout is labelled the same way.

Nothing in a label depends on the `chap-devops` version or the time, because a
changed label makes compose recreate the container: an upgrade of `chap-devops`
renders the same labels, and `chap-devops up` leaves running containers alone.

## Pins

A deployment is reproducible because everything it runs is pinned.

- **Models** are pinned to an image tag, which for marketplace images is a
  `sha-<short commit>` build:
  `image: <repo>:${<ID>_IMAGE_TAG:-sha-24d58c0}`. A model that follows a
  channel records both the channel and the version the channel resolved to; one
  enabled with `--version` records the version alone and never moves.
- **chap-core** is pinned to a release tag. `init` resolves the default
  `--chap-tag latest` to the release it points at today, `v2.3.1` say, and
  writes that into `.env` and `.chap-devops/project.yaml`, so a deployment is
  reproducible rather than following a tag that changes under it. `--chap-tag
  master` or `dev` keeps a moving tag on purpose.

- **OCS and the object store** follow a moving tag, because neither publishes a
  release feed: `${OCS_IMAGE_TAG:-main}` and `${S3_IMAGE_TAG:-latest}`. Set
  either variable in `.env` to pin a build of your own; `chap-devops update` says which
  of the two is in force. **DHIS2** is pinned to a minor line instead,
  `${DHIS2_IMAGE_TAG:-2.42}`, so a patch release arrives with a pull and moving
  to another line is a deliberate edit - DHIS2 migrates its schema forward only.
  See [Components](./components.md) and [DHIS2](./dhis2.md#changing-the-dhis2-version).

Model pins only move in two ways: `chap-devops models enable ID` (with `--channel`
or `--version`) and `chap-devops update`. Nothing else, `up`, `restart`, `expose` and
`unexpose` included, changes the version a model runs. `update` moves the pins
and pulls; `chap-devops restart` applies what it fetched to the services that are
running. See [Updating](./updating.md).

## The `.env` contract

`.env` is the one file that is written once and then left alone. `init` writes
it when the directory has none, and never rewrites it afterwards: a second
`init` keeps the file it finds, `--force` included, `sync` only appends missing
pin comments and `update` only moves the pin comments of models it changed.
One line is the exception. When `init --force` keeps the file, it moves the
active `CHAP_IMAGE_TAG` line to the new tag if that line holds the tag that the
deployment recorded (or `checkout`), as `chap-devops update --chap-tag` does. It says
`moved CHAP_IMAGE_TAG in the kept .env from checkout to v2.4.0`. A line with
another value is kept, with a warning that names the line to set.

That is deliberate. The file holds the database password the PostgreSQL volume
was created with, plus the API token and the registration key, and a rotated
password leaves chap-core unable to authenticate against its own data.

Because of what it holds, `.env` is readable by its owner only (mode 600), and
every write goes to a temporary file that is renamed over it, so a crash or a
full disk never leaves half a file. If a restored or hand-made `.env` is
readable by everyone, the next `chap-devops sync` or `chap-devops up` closes it.

Those two secrets are the one exception to "written once", and a narrow one.
`chap-devops init --api-token` fills them in as it renders the file;
`chap-devops auth enable`, `disable` and `rotate` rewrite exactly those two lines
afterwards, leaving every other line, every comment and every blank line byte
for byte as it found them:

```text
CHAP_API_TOKEN=d1f0...                 the whole API, as `Authorization: Bearer`
SERVICEKIT_REGISTRATION_KEY=9a3c...    what each model sends when it registers
```

Commented out, as the generated file ships them, means no authentication at all.
The values live only here: `.chap-devops/project.yaml` records two booleans under
`auth` and never a secret, so the state directory stays safe to commit and to
paste into a bug report. See [Authentication](./auth.md).

Enabling a component appends its own section the same way `sync` appends a
model's pin: a commented `OCS_IMAGE_TAG`, and for the object store a generated
`S3_ACCESS_KEY` and `S3_SECRET_KEY`. DHIS2 brings the longest section - a
generated `DHIS2_DB_PASSWORD` and `DHIS2_ENCRYPTION_PASSWORD`, plus commented
`DHIS2_IMAGE_TAG`, `DHIS2_DB_DUMP_URL` and `DHIS2_JAVA_TOOL_OPTIONS` carrying the
values the rendered compose file already defaults to. Every generated value there
is written once and never rewritten, for the same reason the database password
is: the volume was created with them.

`init --fresh-env` is the explicit override: it renders a new `.env` with a new
password, so an existing volume has to be dropped with

```sh
chap-devops down --volumes
```

(or the role changed with `ALTER USER`) before Chap will start again.
`init --no-env` writes no `.env` at all.

### What `.env` holds

Every line is optional except the ones `init` writes as active assignments; a
commented placeholder means "not set", which is the default the compose files
already carry.

| Variable | What it does |
| --- | --- |
| `POSTGRES_USER`, `POSTGRES_PASSWORD`, `POSTGRES_DB` | The database Chap creates. The password is 32 random hex characters, generated once. It has to be URL-safe, because the connection URL is composed from it. |
| `CHAP_DATABASE_URL` | A full percent-encoded URL that replaces the one composed from `POSTGRES_*`. The way to use a password that is not URL-safe. |
| `CHAP_IMAGE_TAG` | Tag for both chap-core images: a release tag such as `v2.3.1` pins one, `latest` is the newest release and `master` the branch. |
| `CHAP_API_PORT` | Host port chap-core's API is published on, and the only one a deployment of chap-core and models publishes. A component publishes a well-known port of its own beside it. See [Ports](./ports.md). |
| `CHAP_API_TOKEN` | API token. Commented out means no authentication at all. See [Authentication](./auth.md). |
| `SERVICEKIT_REGISTRATION_KEY` | Shared secret each model sends when it registers, and the one chap-core checks. It travels with the API token. |
| `CHAP_ROOT_PATH` | Path prefix, for serving the API behind a reverse proxy. Not written by `init`; add it when you need it. |
| `<ID>_IMAGE_TAG` | Per-model pin, one commented line per enabled model. Uncomment to run a different build of that model. |
| `OCS_IMAGE_TAG`, `S3_IMAGE_TAG`, `DHIS2_IMAGE_TAG` | The same for the components. See [Components](./components.md). |
| `S3_ACCESS_KEY`, `S3_SECRET_KEY` | Root credentials for the object store, generated once when `s3` is enabled. The volume is created with them, so changing them later locks the store's own data away. |
| `DHIS2_DB_PASSWORD`, `DHIS2_ENCRYPTION_PASSWORD` | DHIS2's own database password and the key it encrypts stored credentials with, generated once when `dhis2` is enabled. 32 hex characters each, past the 24 DHIS2 demands of the second. Nothing to do with the Chap database: DHIS2 has its own. See [DHIS2](./dhis2.md#encryptionpassword). |
| `DHIS2_DB_DUMP_URL`, `DHIS2_JAVA_TOOL_OPTIONS` | The dump the DHIS2 database is seeded from and the JVM options it runs with, both commented with the value the compose file already defaults to. See [DHIS2](./dhis2.md#the-seed). |

Compose reads `.env` automatically, so `${CHAP_IMAGE_TAG:-latest}` and the
per-model `${<ID>_IMAGE_TAG:-sha-xxxxxxx}` pins can be overridden there without
regenerating anything. Because Compose reads `.env` last, a line in it wins
over what `.chap-devops/` records; `CHAP_API_PORT` is the case that matters most, and
[Ports](./ports.md) covers it.

### How `chap-devops` reads it

By Compose's rules, because Compose is the only thing that acts on the file:

- the **last** active assignment of a variable wins, so a file carrying two
  `CHAP_API_TOKEN=` lines is protected by the second one;
- `export KEY=value` assigns the same thing as `KEY=value`;
- a value wrapped in matching single or double quotes does not include them;
- a line whose first non-blank character is `#` assigns nothing, and an empty
  `KEY=` sets nothing (which is how a commented-out token and `CHAP_API_TOKEN=`
  both mean "no authentication").

Writing follows the same rules, so the line `chap-devops` writes is always the line
Compose reads back: `chap-devops auth enable`, `disable` and `rotate` rewrite the
variable's first active assignment and **remove every later duplicate of it**,
leaving exactly one active line. A file with two `CHAP_API_TOKEN=` lines comes
out of `chap-devops auth rotate` with one, holding the new token.

## Context-sensitive help

`chap-devops --help` adapts to where you are. Outside a deployment directory it lists
only the commands that can work there:

```text
Commands:
  init         Create a deployment directory: compose files, .env and .chap-devops/
  run          Start one model and print where it answers
  ps           List the models that run, and where each one answers
  stop         Stop a model and remove its compose overlay; its data stays
  top          Watch every chap-devops deployment on this machine as a live tree
  models       Browse and manage marketplace models
  registry     Inspect and refresh the marketplace registry
  update       Move the pins to what upstream publishes now, and pull
  api          Send one authenticated request to chap-core's API
  chap         Run the chap CLI in a container, with no Python or uv installed
  chapkit      Run the chapkit CLI in the chap-core image, with no Python installed
  doctor       Run a checklist over this machine and this deployment
  cleanup      Delete what deployments whose directory is gone left in docker
  self         Update chap-devops itself, and report what this build is
  completions  Print a shell completion script for chap-devops
  help         Print this message or the help of the given subcommand(s)
```

Inside one, the deployment commands are listed too: up, down, logs, restart,
status, sync, ui, components, docker, backup, auth, jobs, open and dhis2. The
hiding runs both ways, so `init` drops out of that listing: the deployment it
would create is already here. It still runs there - `chap-devops init --force` is how the API
port or the components of a deployment are rewritten, see
[How the API port is set](./ports.md#how-the-api-port-is-set), and
`chap-devops init sub` creates a second deployment in a subdirectory of the first.

The hidden commands still run if you type them; they just tell you there is no
project. A group command typed on its own says the same thing rather than
listing subcommands that cannot run either, so `chap-devops components` outside a
deployment is the missing-project error, not the components help. `chap-devops
models` is split the same way: `list`, `search` and `info` browse the
marketplace and work anywhere, while `add`, `remove`, `enable`, `disable`,
`expose`, `unexpose` and `test` need a deployment and are hidden outside one. The
[command reference](./reference.md) lists all of them, because it is rendered
from the static command tree rather than from where you happen to be.

## Project discovery

Every command that operates on a project finds it the way git finds `.git`:
from the current directory, or from `-C DIR`, upwards to the nearest `.chap-devops/`.
So `chap-devops up` works from any subdirectory of the deployment, and `chap-devops -C
/srv/mychap status` works from anywhere at all.

The wrappers then always run

```sh
docker compose -f <dir>/compose.yml -f <dir>/compose.chap-devops.yml -f <dir>/compose.marketplace.yml ...
```

with one `-f` per enabled component before the umbrella (and without
`compose.yml` and `compose.chap-devops.yml` when chap-core is not a component), from
the project directory, so they behave the same whatever your shell's
working directory is, and they exit with Compose's own exit code.
