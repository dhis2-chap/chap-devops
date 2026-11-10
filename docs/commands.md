# Commands

```text
chap-devops [--json] [-C DIR] [--registry-url URL] [--offline] [--cache-dir DIR] <command>
```

The groups below are the shape of the tool. The
[Command reference](./reference.md) is generated from the `--help` texts and
lists every command, every flag and every default.

## Running one model

| Command | What it does |
| --- | --- |
| `chap-devops run MODEL [--port N\|auto] [--bind ADDR] [--group NAME] [--allow-starter] [--no-wait] [--timeout S] [-a [--rm]] [--chap-core URL]` | Start one model - a marketplace id, a GitHub repository URL, an image on any registry or a local image - and print where it answers, once it does. Outside a deployment it runs in a `chap-devops run` group under the data directory, on `127.0.0.1`; inside one, in that deployment. `-a` keeps it in the foreground with its log, and Ctrl-C then stops the model and keeps its data (`--rm` removes it). `--chap-core URL` makes the group's models register with a chap-core that runs elsewhere. |
| `chap-devops ps [--group NAME]` | Every model in every group (or in the deployment the command is inside), with its state and URL. |
| `chap-devops stop ID\|--all\|--group NAME [--purge]` | Stop a model, in whichever group has it, or every model in a group, and remove its compose overlay; the data stays unless `--purge`, which also removes a group left empty. |
| `chap-devops top [--interval S] [--record FILE [--duration S]]` | Every chap-devops deployment on the machine as a live tree - `init` deployments and `run` groups, every service with its state, CPU, memory and URL - with logs and stop on a key. One snapshot off a terminal. `--record` writes the CPU and memory of every service to a file as JSON lines instead, for a load test. See [chap-devops top](./top.md). |

See [Running one model](./run.md).

## The everyday verbs

| Command | What it does |
| --- | --- |
| `chap-devops init [DIR]` | Create a deployment directory: compose files, `.env` and `.chap-devops/`. `--api-token` protects the API from the start; `--source PATH` builds chap-core from a checkout; `--chap-core-url URL` uses a chap-core elsewhere; `--adopt DIR` takes over a chap-admin deployment with its database (see [Taking over a chap-admin deployment](./use-cases/from-chap-admin.md)); `--with ocs[,s3][,dhis2]` adds optional components, `--only LIST` deploys exactly those and nothing else, and `--ocs-port`, `--s3-port`, `--ocs-read-only`, `--dhis2-port`, `--dhis2-seed` and `--dhis2-seed-password` settle how they start. |
| `chap-devops sync [--check]` | Render the compose files from `.chap-devops/`; `--check` writes nothing and exits non-zero if anything would change. |
| `chap-devops up [-a] [--pull] [--no-preflight] [--replace] [--wait [--timeout S]] [EXTRA..]` | Sync, check that the host ports are free (offering to stop another chap-devops deployment that holds them; `--replace` does so without asking), then `docker compose up -d --remove-orphans`, and end with what started or was recreated. The orphans are the containers of models and components that were disabled; see [Orphans](./concepts.md#orphans) for what that means for a compose file of your own. `-a` (also `--attach`, `--foreground`) runs in the foreground and streams the logs instead. A deployment with no components and no models has nothing to start, and `up` says so and names the commands that add one. `--wait` holds the command until chap-core answers and every model is registered (or, without chap-core, answers on its own port), and until OCS and DHIS2 answer their health check when each has a host port, then prints where each one answers; after `--timeout` seconds (300) for all of them together it fails and names what never did. |
| `chap-devops down [--volumes] [-y] [EXTRA..]` | `docker compose down`, then say what it stopped and that the volumes are still there, naming the compose project they are prefixed with. `--volumes` removes them too, and the data in them: it names the volumes first and asks, unless `-y`/`--yes` says it was meant. `--volumes` also removes the leftover volumes that docker labels with this deployment's compose project, such as the data volume of a removed model, and warns about each volume that is still there. Either way it ends on `chap-devops up`, which brings it back. |
| `chap-devops logs [-f] [--tail N] [SERVICE..]` | `docker compose logs`; says so instead of printing nothing when the project has no containers, and lists the services when `SERVICE` is not one of them. `--tail N` keeps the last N lines of each service. When stdout is not a terminal, or under `--no-color` or `NO_COLOR`, the colour codes the containers print themselves are removed too, so a script reads plain text. |
| `chap-devops restart [SERVICE..] [--all]` | Recreate the running services whose image or configuration changed (`docker compose up -d --remove-orphans`), and say which ones that was. Changes no file and no pin; `--all` recreates the named services anyway. |
| `chap-devops status [--url URL] [--timeout SECONDS]` | `GET /health` and `/v2/services`, check that the answers are chap-core's, and diff the registered services against the ones this project enabled. Sends the API token from `.env` when there is one, and says `auth: on` or `auth: off`. |
| `chap-devops open [NAME]` | Open a component's web interface in a browser: chap-core's API documentation at `/docs`, the OCS web interface, DHIS2's own; an enabled model's id opens that model's `/docs`, on its host port or through chap-core. With no name it lists every component and what each one opens. `--no-browser` prints the address instead of opening one, and `--json` never opens one. A component this deployment does not have, one that publishes no host port and the object store - which serves no web interface at all - are each refused with what is true instead and the command that changes it. |
| `chap-devops jobs [list] [--status S].. [--type T] [--limit N]` | The backtests, predictions and datasets chap-core has run, newest first, with what each one cost and which of them failed. `show`, `logs`, `cancel` and `delete` take one job id, or enough of its start to name one. |
| `chap-devops api METHOD PATH [--data JSON\|@FILE\|-] [--url URL] [--raw]` | One authenticated request to chap-core's API, with this deployment's base URL and token filled in. JSON comes back pretty-printed; the exit code is 0 for a 2xx, 1 for a 4xx/5xx and 2 when chap-core is not answering. |
| `chap-devops bench [BACKTEST] [--repeat N] [--parallel N] [--url URL]` | Time the chap-core requests the Modeling App sends for one evaluation, the newest when no id is given, and report min, p50, p95 and max for each. See [chap-devops bench](./bench.md). |
| `chap-devops bench --write --model MODEL [--backtests N] [--dataset ID] [--keep]` | Import a synthetic dataset, start N backtests of one model on it at the same time, and check that each one stored all its forecasts. See [Backtests under load](./bench.md#backtests-under-load---write). |
| `chap-devops bench dataset [--out DIR] [--import] [--locations N] [--periods N]` | Write a synthetic dataset as CSV and GeoJSON, or import it into chap-core. See [Synthetic datasets](./bench.md#synthetic-datasets-chap-devops-bench-dataset). |
| `chap-devops update [--dry-run] [--pin-chap-core] [--chap-tag TAG] [--list-tags] [--yes]` | Move the pins to what upstream publishes now, pull the images, and end with one line saying what moved and what needs restarting. Never touches a container. `--chap-tag` moves chap-core to another tag (a release, `latest`, `master` or `dev`) instead, asking first when the move goes backwards, which `--yes` answers; `--list-tags` lists where it can go and writes nothing. Outside a deployment it refreshes the marketplace registry instead. |
| `chap-devops doctor` | Run a checklist over this machine and this deployment: Docker, Compose, architecture, disk, the hosts Chap pulls from, and - inside a deployment - the files, the ports, the pins, the images and whether Chap is up. Works anywhere. |
| `chap-devops cleanup [--dry-run] [--yes]` | Delete the volumes and network that deployments whose directory is gone left in docker, after listing them and asking. `--dry-run` (`-n`) only lists; `--yes` deletes without asking. Only deployments chap-devops recorded are considered, and nothing a container still uses. Works anywhere. See [Cleaning up after removed deployments](./doctor.md#cleaning-up-after-removed-deployments). |
| `chap-devops dhis2 connect` | Let the DHIS2 Modeling App reach this deployment's Chap: the `chap` route, the apps, then analytics. `show`, `route`, `analytics` and `apps` are the same work one step at a time; `chap-devops dhis2 use` points them at a DHIS2 that runs elsewhere. |

`chap-devops up` starts what is on disk, `chap-devops update` fetches newer versions, and
`chap-devops restart` applies them to the services that are running. That is the
whole distinction, and it is why `up` and `update` are both safe to run at any
time. See [Updating](./updating.md).

`chap-devops jobs` and `chap-devops api` are the two that talk to the API rather than to
Docker. A job is one unit of slow work chap-core handed to its worker, and a
failed one says why in `chap-devops jobs logs <id>` and nowhere else; `chap-devops api` is
what starts a job from a terminal in the first place. See
[Jobs and the API](./jobs.md).

`chap-devops doctor` is the one to run first on a machine you have not deployed on
before, and first again when something is wrong: it asks in one pass what the
other commands assume. It exits non-zero only when a check failed. See
[Doctor](./doctor.md).

`chap-devops down --volumes` is the reset that takes the data with it
(`docker compose down -v --remove-orphans`): it lists the volumes docker holds
under this deployment's name - with how much the OCS data volume holds next to
it, when that can be measured - asks before removing them and ends with the
ones that are gone, by name. `-y` (`--yes`) is how a script says it meant it, and is
required where there is nothing to ask at - a stdin that is not a terminal, or
`--json` - rather than the question being skipped. Compose removes the volumes
its files still declare, so a volume left over from a disabled model survives;
`chap-devops doctor` finds those, and `chap-devops models disable <id> --purge` removes
them. There is no `-v` for this: `-v` is the global `--verbose` flag, so a `-v`
that reaches the passthrough (`chap-devops down -- -v`) is refused and told to use
`--volumes`, rather than being passed on to compose or quietly dropped.

## Models

| Command | What it does |
| --- | --- |
| `chap-devops models list` | List marketplace models (`--all`, `--starters`, `--enabled`). |
| `chap-devops models search QUERY` | Search id, name and summary. |
| `chap-devops models info ID` | Everything known about one model. |
| `chap-devops models test [ID..] [--all]` | Make each model train and predict, and say whether it could: `chapkit test` in its own container, or `--backtest` for the whole way round through chap-core (`--config NAME`, `--seed`, `--timeout`, `--keep`). With `--backtest`, the line under each row names the configured model it used. |
| `chap-devops models configs [list] [ID] [--all]` | List the configured models that chap-core has of each enabled model: name, covariates, options and source; `--all` adds the archived ones. See [Configured models](./models.md#configured-models). |
| `chap-devops models configs add ID` | Add a configured model: `--name` and `--set KEY=VALUE` (`--covariates a,b`), or `--from FILE` for every configuration in a file of the marketplace format. With no options at a terminal, it opens the form. |
| `chap-devops models configs update ID NAME` | Give a configured model new values (`--set`, `--unset`, `--covariates`), or open the form with its current values. chap-core keeps the old values as an archived configured model. |
| `chap-devops models configs archive ID NAME` | Archive a configured model. chap-core keeps it, and the Modeling App shows it as Archived. |
| `chap-devops models configs export [ID] [--out FILE]` | Write the configured models of one model as the `configurations:` block of a marketplace entry. |
| `chap-devops models configs sync [ID..]` | Create the configured models that chap-core needs to run each model: one for each configuration of its marketplace entry. A model that has one of its version already is left alone. See [Sync](./models.md#sync). |
| `chap-devops models new DIR` | Create a model project from a template (`--template`, `--with-validation`): a chapkit service or an MLproject, in Python or R. The build runs in Docker, so Python and chapkit are not necessary. See [A new model project](./models.md#a-new-model-project). |
| `chap-devops models add SOURCE` | Add a model the marketplace does not list, from a GitHub repository URL or an image reference on any registry, and enable it (`--id`, `--service-id`, `--name`, `--port`, `--bind`, `--data-dir`, `--user`, `--runtime-amd64`, `--allow-starter`). A source the marketplace lists is enabled as that marketplace model unless `--id` is given; `--id auto` picks a free id. |
| `chap-devops models remove ID [--purge] [--keep-in-chap]` | Disable such a model if it is on, drop its definition from `.chap-devops/models-manual.yaml`, and retire it in chap-core; `--purge` removes its data volume too, `--keep-in-chap` leaves chap-core alone. |
| `chap-devops models enable ID [ID..]` | Record each model in `.chap-devops/models.yaml` and write its overlay (`--channel`, `--version`, `--port`, `--bind`, `--data-dir`, `--user`, `--allow-starter`). |
| `chap-devops models disable ID [--purge] [--retire]` | Drop the model from `.chap-devops/models.yaml` and remove its overlay, keeping its data volume and naming it; `--purge` removes that volume too, `--retire` retires the model in chap-core. |
| `chap-devops models expose ID [--port N\|auto] [--bind ADDR]` | Publish a host port for an enabled model, without touching the version it is pinned to; `--bind 127.0.0.1` keeps it on this machine. See [Ports](./ports.md#which-address-a-model-port-is-published-on). |
| `chap-devops models unexpose ID` | Remove the host port of an enabled model. |
| `chap-devops ui` | Open the browser: marketplace models and components, on two pages `Tab` moves between. One `s` saves both. |

`list`, `search` and `info` read the catalogue and work outside a project. The
rest need a project; of them, only `test`, `configs list` and `configs export`
change nothing.
Registration is a heartbeat, so `chap-devops status` and `chap-devops doctor` can be green
while a model cannot predict at all - `chap-devops models test` is the check that
settles it. See [Testing a model](./models.md#testing-a-model). `add` is the only one that needs the network for
something other than the catalogue: it reads GitHub and ghcr to resolve what it
was given. See [Models and the marketplace](./models.md).

## Components

| Command | What it does |
| --- | --- |
| `chap-devops components list` | Every component, whether this deployment has it and where it is reached. |
| `chap-devops components enable NAME [--port N]` | Enable a component, or change the settings of an enabled one, then sync. `ocs` also takes `--ocs-name`, `--ocs-country` and `--ocs-bbox` for the instance config it scaffolds, plus `--base-url` and `--read-only`/`--read-write`. `dhis2` also takes `--tag`, `--image` and `--seed`. A port already in use is a warning, not a refusal. |
| `chap-devops components disable NAME [--purge]` | Turn it off, remove its compose file and sync, keeping its data volumes and naming each one; `--purge` removes them too, and is refused for chap-core. |

A component is a service (or a small group) that `chap-devops sync` renders one
compose file for: `chap-core`, which is Chap itself and is on unless you turn it
off, `ocs` (Open Climate Service), `s3` (the object store OCS will use) and
`dhis2` (a demo or development DHIS2 with its own database). The set lives in
`.chap-devops/components.yaml`. `chap-devops init --with ocs,s3`, `--without chap-core` and
`--only ocs,s3` set it at creation time. `chap-devops open NAME` opens one of them in a browser, and
`o` does the same from the browser's components page. See
[Components](./components.md),
[Reaching a component from a browser](./components.md#reaching-a-component-from-a-browser)
and [DHIS2](./dhis2.md).

## DHIS2

For a deployment with the [`dhis2` component](./dhis2.md), or with an
[external DHIS2](./dhis2.md#a-dhis2-that-runs-elsewhere) recorded by `chap-devops dhis2
use`. Apart from `use`, which records a DHIS2 in `.chap-devops/components.yaml`, and
`connect`, which records when it ran in the same file, nothing here touches
Docker or a file: every verb is requests to DHIS2's own API.

| Command | What it does |
| --- | --- |
| `chap-devops dhis2 show [--user NAME]` | Where the `chap` route points, what DHIS2's analytics timestamp is worth, which of the two apps are installed, and each piece that is missing. Writes nothing. |
| `chap-devops dhis2 route` | Create the `chap` route, or repoint one that points elsewhere, is disabled or has lost the `F_CHAP_MODELING_APP` authority - then proxy a request through it to prove chap-core answers. |
| `chap-devops dhis2 analytics [--timeout SECONDS] [--no-wait]` | Generate the analytics tables the Modeling and Climate apps read, and wait for the run; a run that is already going is watched rather than queued behind, since DHIS2 runs one at a time. |
| `chap-devops dhis2 apps` | Install the Modeling App and the DHIS2 Climate App from the App Hub, at the newest version this DHIS2 can run. The one verb that needs the network. |
| `chap-devops dhis2 connect [--timeout SECONDS] [--no-wait]` | The route, the apps, then analytics. Under `--offline` the apps are a reported skip and the other two still happen. On an external DHIS2 only the route; the other two are left to its admin. |
| `chap-devops dhis2 use [URL] [--chap-url URL] [--clear]` | Record a DHIS2 that runs elsewhere, and where it reaches chap-core, then ask it whether it answers and takes the credential. Bare, it says which DHIS2 is used; `--clear` forgets it. |

Every one of them is idempotent: a second run repoints nothing, reinstalls nothing
and says so. All of them take `--user NAME` and `--wait SECONDS` (how long to wait
for DHIS2's API: 20 minutes by default for the `dhis2` component, because a first
start takes minutes, and 60 seconds for an external DHIS2). The credential comes
from `DHIS2_API_TOKEN` or `DHIS2_ADMIN_USERNAME`/`DHIS2_ADMIN_PASSWORD` in
`.env`, else `CHAP_DEVOPS_DHIS2_TOKEN` or `CHAP_DEVOPS_DHIS2_PASSWORD` in the environment,
else the seed password, else `admin`/`district` (only on a DHIS2 that chap-devops
deployed). See [The
credentials](./dhis2.md#the-credentials-and-where-they-come-from). There is no
`--password` flag, because a password on a command line is in the shell history. `chap-devops up` does none of this and
[the chapter says why](./dhis2.md#chap-devops-up-does-none-of-this-on-purpose) - so
`chap-devops up` and `chap-devops status` each end by naming `chap-devops dhis2 connect` until one
has been recorded in `.chap-devops/components.yaml`. That record suppresses the hint
and is never evidence that the route is right; `chap-devops dhis2 show` is what asks
DHIS2. See [Connecting the Modeling App to
Chap](./dhis2.md#connecting-the-modeling-app-to-chap) and [`connected_at`, and
what it is not](./dhis2.md#connected_at-and-what-it-is-not).

## Authentication

| Command | What it does |
| --- | --- |
| `chap-devops auth show [--reveal]` | Whether the API token and the registration key are set, and the token itself only with `--reveal`. |
| `chap-devops auth token` | The token alone on stdout, for `TOKEN=$(chap-devops auth token)`; nothing on stdout and exit 1 when authentication is off. |
| `chap-devops auth enable [--token VALUE]` | Write both secrets to `.env`, record them in `.chap-devops/project.yaml` and re-render the overlays. |
| `chap-devops auth disable` | Comment both `.env` lines out, keeping their values, and re-render. |
| `chap-devops auth rotate` | Replace both secrets with new generated ones. |

All four touch only those two `.env` lines, and none of them restarts anything:
chap-core and the models read `.env` when Compose creates them, so `chap-devops up`
has to follow. `chap-devops init --api-token` does the same thing at creation time.
See [Authentication](./auth.md).

## The chap CLI

| Command | What it does |
| --- | --- |
| `chap-devops chap [--tag TAG] [--image core\|worker] [--group NAME] [--stop] [--timeout S] [--docker] CHAP_ARGS..` | Run chap-core's own `chap` CLI in a container, with the current directory mounted at the same path, so its file arguments and outputs are on this machine. A model id in `--model-name` starts that model, without chap-core and without `chap-devops up`, and `--stop` stops it after the run. The image follows `--model-name`; a deployment gives its tag and its network. `--docker` gives the container the docker socket, for `docker_env` models. Works anywhere. |
| `chap-devops chapkit [--tag TAG] [--image IMAGE] CHAPKIT_ARGS..` | Run the `chapkit` CLI in the chap-core image, with the current directory mounted at the same path. `mlproject migrate DIR` makes a chapkit service from an `MLproject` model, and chap-devops then writes its `uv.lock`. `mlproject run DIR` serves an `MLproject` model in the foreground, in a chapkit `-cli` image or `--image`, and in a deployment registers it with chap-core. `mlproject export main.py` writes the `MLproject` of a chapkit service. Works anywhere. See [An MLproject model, with no image to build](./use-cases/mlproject-model.md). |

You need no Python, uv or R for it. See [The chap CLI](./chap-cli.md).

## Registry

| Command | What it does |
| --- | --- |
| `chap-devops registry update` | Fetch the catalogue now and refresh the cache. |
| `chap-devops registry show` | Where the catalogue came from and what it holds. |

## Docker

The Docker plumbing you only reach for when you already know what Docker is
doing lives under `chap-devops docker`, so the top-level list stays readable if you
have never used Compose.

| Command | What it does |
| --- | --- |
| `chap-devops docker ps [EXTRA..]` | List the containers this deployment is running (`docker compose ps`). |
| `chap-devops docker pull` | Download the pinned images into the local Docker daemon; no files change. |
| `chap-devops docker exec SERVICE [CMD..]` | Run a command in a running container, which has to be up already (`chap-devops up`). `CMD` defaults to a shell, and `-T` is passed for you when there is no terminal, so it works in scripts. |
| `chap-devops docker run -- ARGS..` | Any `docker compose` command, behind the deployment's `-f` list. |
| `chap-devops docker config [-- EXTRA..]` | The finished configuration: every compose file merged into one document (`--json` prints it as JSON). |

`chap-devops docker config` is what Docker actually reads after the `-f` list, the
`include:` and the `.env` substitutions have been applied, which is the fastest
way to find out why a setting is not taking effect.

## Backup

| Command | What it does |
| --- | --- |
| `chap-devops backup create [--out PATH] [--no-db] [--no-models] [--no-components]` | Write the chap-core database (pg_dump), the model data, the component data (the DHIS2 database as a pg_dump) and the deployment files to one `tar.gz`. |
| `chap-devops backup restore ARCHIVE [--yes] [--files-only] [--db-only] [--no-models] [--no-components] [--adopt-identity] [--no-start]` | Put a deployment back from such an archive, after printing what it overwrites. |

See [Backup and restore](./backup.md).

## chap-devops itself

The two commands that are about the CLI rather than about a deployment. Both
work anywhere, inside a deployment or not.

| Command | What it does |
| --- | --- |
| `chap-devops self update [--check] [--version TAG] [--yes]` | Replace this binary with the newest release: download the archive for this target, check it against the release's `SHA256SUMS`, rename it over the running one. `--check` only reports. |
| `chap-devops self version` | Version, git revision, target triple, binary path and whether it came from a release archive, `cargo install`, a `cargo build` in a checkout, or another local build. |
| `chap-devops completions SHELL` | Print a completion script for bash, zsh, fish, PowerShell or elvish. |

`chap-devops update` moves a deployment's pins; `chap-devops self update` replaces the
`chap-devops` binary. Nothing in the first touches the second. See
[Install](./install.md) for both of these and for the once-a-day notice that a
newer release exists.

## Global options

They are accepted before or after the subcommand, on every command, and the
[reference](./reference.md#chap-devops) lists them with their defaults. The last
three are the registry options. The help lists them under "Registry options"
only for the commands that use them; [Global
options](./status.md#global-options) names those commands. On the other
commands they change only the once-a-day notice that a newer release exists.

| Option | What it does |
| --- | --- |
| `--json` | Machine-readable output: exactly one JSON document on stdout. |
| `--no-color` | Never colour the output; `NO_COLOR` in the environment does the same. |
| `-v, --verbose` | Show the hints: the next commands and the background. `-vv` also narrates on stderr what runs: commands, HTTP requests, the registry source, the files `sync` compared. |
| `-d, --debug` | Everything `-vv` says, plus response bodies and resolved paths. |
| `-C, --project-dir DIR` | Where to look for the project; found like git finds `.git`. |
| `--registry-url URL` | A different marketplace index, for a fork or a mirror. Inside a deployment the default is the one `init` recorded. |
| `--offline` | Never touch the network; use the cache or the embedded snapshot. Shown by the commands that read the registry, and by `dhis2 apps`, `dhis2 connect` and `self update`, which use it for the App Hub and the release feed. |
| `--cache-dir DIR` | Override the cache directory for one invocation: the registry snapshot, and the downloads of `self update`. |

## `--json` for scripts and tools

Under `--json` every command prints exactly one JSON document on stdout, and
everything meant for a person - progress, notes, warnings - goes to stderr.
A tool that drives chap-devops reads stdout and branches on one field:

- **Success** of a command that changes something (`models enable`, `disable`,
  `add`, `remove`, `expose`, `unexpose`, `up`, `models test`) carries
  `"ok": true`. The read-only commands (`status`, `models list`, `models info`)
  print their report as it is; `status` says how things are in its own exit
  code.
- **Failure** of any command is

  ```json
  {"ok": false, "error": "...", "hint": "run `chap-devops ...`", "causes": []}
  ```

  `hint` is the way out: the message's last clause when that clause names a
  command or a flag to use instead (`pass \`--id auto\` ... to add this one
  beside it`), and `null` when the message has no way out to give. `error` is
  the message up to that clause, so the two never say the same thing; the
  terminal shows them together after `error:`. A message of several lines,
  for example the port check with more than one busy port, stays whole in
  `error`, with `hint: null`, because each line has its own way out. A usage error - an unknown flag, a
  missing argument - is the same document, with clap's message as `error` and
  the command's `--help` as `hint`, and exits 2.
- `models enable` and `models add` list what they enabled under `models`, one
  object per model with `id`, `service_id`, `port`, `bind` and `url` (the
  model's own port, else chap-core's proxy to it, else `null`), next to the
  full change report.
- `up` prints `api_url`, the compose services `running` and the ones this run
  `started`, every enabled model the same way, and under `--wait` what the wait
  found: `wait.ready`, `wait.waited_s` and each model's `state` and `url`.
- `models test` prints `{"ok": ..., "models": [...]}`, one object per model with
  its `result` (`pass`, `fail` or `skip`), the `summary` and the `detail` that
  says why and what to do.
- `models configs sync` prints `{"ok": ..., "models": [...]}`, one object per model
  with `id`, `service_id` and its `state`: `created` (with the names in
  `configured`), `already-configured`, `not-registered` or `failed` (with the
  reason in `error`).
- `models configs list` prints `{"models": [...]}`, one object per model with
  `id`, `service_id` and `configs`. Each configured model has `id`, `name`,
  `variant`, `archived`, `version`, `covariates`, `values` and `source`.
- `models configs add` prints `{"ok": true, "model": ..., "configured": [...]}`,
  with the rows that chap-core created.
- `models configs update` prints `model`, `changed`, `configured` (the new
  row) and `archived` (the old row). If nothing changed, `changed` is `false`.
- `models configs archive` prints `{"ok": true, "model": ..., "archived": ...}`.
- `models configs export` prints `model`, `configurations` (the block, as
  JSON) and `prediction_periods_from`, which says where each
  `prediction_periods` comes from.

The exit code agrees with `ok`: 0 for `true`, non-zero for `false`. Two
commands print `"ok": false` rather than an error and still exit non-zero:
`models test`, when a model failed or every model was skipped, and `models
configs sync`, when a model could not be configured.
