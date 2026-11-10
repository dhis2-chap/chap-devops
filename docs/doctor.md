# Doctor

`chap-devops doctor` is the first command to run on a machine you have not deployed
on before, and the first one to run when something is wrong. It asks, in one
pass, every question the other commands assume the answer to: is Docker there,
is its daemon up, is Compose new enough, is there disk, can this machine reach
the hosts Chap pulls from, and is this the newest `chap-devops`. Inside a deployment
directory it goes on to the project: its compose project name, the files, the
compose files, `.env`, the volumes, the host ports, the pins, the images and
whether Chap is up.

```sh
chap-devops doctor
```

```text
ok    docker cli                                   docker 29.9.0
ok    docker daemon                                Docker Engine 29.8.2
ok    docker compose                               v5.5.1
warn  os and arch                                  macos/aarch64; Chap images are linux/amd64 only, so they run under emulation
      nothing to do: Rosetta runs the amd64 images, they are only slower
ok    disk space                                   1067.0 GB free on /Users/you/mychap
ok    leftovers                                    no volumes left by removed deployments
ok    network ghcr.io                              reachable (HTTP 401)
ok    network marketplace                          reachable (HTTP 200)
ok    network releases                             reachable, newest chap-core is v2.4.0
ok    github api                                   reachable, 41 of 60 requests left this hour (no token; set GITHUB_TOKEN for 5000)
ok    chap-devops                                        v0.102.0, aarch64-apple-darwin, cargo build
ok    compose project                              mychap-1ab2c3 (recorded in .chap-devops/project.yaml)
ok    deployment files                             all 7 present
ok    components                                   chap-core
ok    compose files                                in sync (0 to write, 4 unchanged, 0 to remove)
ok    .env                                         auth off, POSTGRES_PASSWORD set, CHAP_IMAGE_TAG present
ok    volumes                                      no mychap-1ab2c3_* volume yet; `chap-devops up` creates them
fail  api port                                     8700 is in use by something else
      port 8700 is already in use on this machine (needed by chap); free it, or set CHAP_API_PORT=8701 in `.env`
ok    chap-core pin                                v2.4.0 is the newest release
ok    image chapkit-simple-multistep-model         ghcr.io/chap-models/chapkit_simple_multistep_model:sha-10b2bc7 (linux/amd64)
ok    registry pin chapkit_simple_multistep_model  sha-10b2bc7 is the newest build on main
ok    user chapkit-simple-multistep-model          1001:1001, as the image declares
skip  health                                       no container of this deployment is running
      run `chap-devops up` to start Chap

23 checks: 20 ok, 1 warn, 1 fail, 1 skipped
```

Four words carry the verdict, and only one of them is a problem:

| Word | Colour | What it means |
| --- | --- | --- |
| `ok` | green | Nothing to do. |
| `warn` | yellow | It works, but something will bite later. |
| `fail` | red | Chap will not work until this is dealt with. |
| `skip` | dim | Not evaluated: there was nothing to ask, or asking was ruled out. |

A line that is not `ok` carries an indented line under it saying what to do
about it, and that line is the same sentence the command it belongs to would
have failed with. In a deployment without chap-core, the `health` line says
``run `chap-devops up` to start the deployment``. The exit code is non-zero **only when something failed**, so
`chap-devops doctor` in a CI step goes red on the checks that would have stopped the
deployment and stays green on the ones that are merely worth knowing.

Every check is bounded. Each external command is killed if it overruns, each
network probe has a three-second timeout, and the probes run in parallel, so
`chap-devops doctor` always finishes and normally does so in a few seconds.
`-vv` prints every command and request it made.

## Machine checks

These run everywhere, inside a deployment directory or not.

| Check | What it asks | What a bad answer means |
| --- | --- | --- |
| `docker cli` | `docker --version` answers | Asked without the daemon, so a Docker Desktop that is not started reads as such on the `docker daemon` line below rather than as a missing CLI. Fails when `docker` is not on `PATH`. Install Docker Desktop or the docker CLI from <https://docs.docker.com/get-docker/>. |
| `docker daemon` | `docker info` reaches a daemon | Either nothing is listening, which is `open Docker Desktop` or `sudo systemctl start docker`, or the socket refuses this user, which is `sudo usermod -aG docker $USER` and a fresh login. The two are told apart by what Docker printed, because the fixes have nothing in common. |
| `docker compose` | `docker compose version` and how old it is | Below 2.24.4 the API port override will not work: `compose.chap-devops.yml` uses `!override`, which arrived in 2.24.4, so the API is published on two ports. Below 2.20 the model overlays will not load either: `compose.marketplace.yml` uses `include:`, which arrived in 2.20. A warning rather than a failure, because the base services still run. |
| `os and arch` | what this host is | On an arm64 host it warns: chap-core and every marketplace image are published for amd64 only. On macOS that is nothing to do, Rosetta runs them. On arm64 Linux it needs qemu/binfmt: `docker run --privileged --rm tonistiigi/binfmt --install amd64`. Nothing is probed, because probing means running an amd64 container, which is far too slow for a checklist. |
| `disk space` | free space where the images land | Warns below 10 GB and fails below 3 GB. The measured filesystem is Docker's own data root when this host can see it, and the working directory otherwise: Docker Desktop reports a path inside its Linux VM, which says nothing about the disk out here. |
| `memory` | how much memory docker says it can give a container | Only asked on a deployment that has the `dhis2` component enabled: DHIS2 is the one service here whose failure for want of a few gigabytes is a silent kill rather than an error, and a line about a limit nothing else comes near would be noise everywhere else. Warns below 6 GB and fails below 4 GB, and names both ways out - give docker more (Docker Desktop: Settings, Resources), or size the heap down with `DHIS2_JAVA_TOOL_OPTIONS` in `.env` - because below 4 GB the JVM is killed part-way through the analytics populate phase and the run never finishes. The figure is docker's own rather than this host's memory: on macOS and Windows the containers live in a Linux VM whose size a large host says nothing about, and on Linux the daemon reports the host's memory anyway. An engine that will not say is a skip, as is having no docker CLI to ask through, because a guess here sends an operator after a memory problem they do not have - or leaves them without the one they do. See [Memory](./dhis2.md#memory). |
| `network ghcr.io` | can this machine reach the image registry | Without it no image can be pulled. A warning, not a failure: `--offline` keeps `chap-devops` itself working from the cache or the catalogue built into the binary. |
| `network marketplace` | can it reach `registry.yaml` | The catalogue falls back to the cache and then to the embedded snapshot, so this is a warning too. `chap-devops registry show` says which one is in use. |
| `network releases` | can it reach the chap-core release list | `chap-devops update` needs it; everything else works without it. The answer doubles as the newest release, which `chap-core pin` below compares against. |
| `github api` | how much of this address's hour is left on GitHub's REST API | `GET /rate_limit`, which GitHub documents as not counting against the limit it reports. Unauthenticated that limit is 60 requests an hour **per address**, which the release lookup, the `chap-core pin` line, every `registry pin <id>` line and every `chap-devops models add` all spend - a busy operator, or a CI runner sharing its egress address, runs out before lunch. The line says which of the two limits this is: `4990 of 5000 requests left this hour (token)`, or `43 of 60 requests left this hour (no token; set GITHUB_TOKEN for 5000)`. It warns when nothing is left, with the clock time the window resets at; it never fails, because every check that needs GitHub already degrades to a skip with the reason on its own line. See [a used-up rate limit](./troubleshooting.md#githubs-rate-limit-is-used-up). |
| `leftovers` | volumes that deployments whose directory is gone left in docker | Warns with how many volumes of how many removed deployments are still there: `chap-devops cleanup` deletes them, `chap-devops cleanup --dry-run` lists them first. Nothing else in chap-devops would ever name those volumes again. See [Cleaning up after removed deployments](#cleaning-up-after-removed-deployments). Skipped when there is no docker to ask. |
| `chap-devops` | version, target, install method, newer release | Warns when a newer release exists: `chap-devops self update`. Under `--offline` the line still names the build, as a `skip`: whether there is an update is the one thing this run did not ask. |

An anonymous request to `ghcr.io/v2/` answers `401`, which is the registry
answering; any HTTP status counts as reachable, because a host that refuses us
is still a host this machine can reach.

### Raising the GitHub limit

Set `GITHUB_TOKEN` (or `GH_TOKEN`, which is what the `gh` CLI uses) and every
GitHub request `chap-devops` makes carries it, for 5000 requests an hour instead of
60:

```sh
GITHUB_TOKEN=$(gh auth token) chap-devops doctor
```

```text
ok    github api                                   reachable, 4989 of 5000 requests left this hour (token)
ok    registry pin chapkit_simple_multistep_model  sha-10b2bc7 is the newest build on main
```

A classic token with no scopes at all, or a fine-grained token with read
access to public repositories, is enough: everything `chap-devops` asks GitHub is a
public read. `chap-devops` never writes the token anywhere - not to `.chap-devops/`, not
to `.env`, not to the cache - and never prints it: a `-vv` trace says
`Authorization: Bearer <token>` and nothing more. Nothing on the command line
sets it, because a token on a command line is a token in the shell history.

## Cleaning up after removed deployments

Deleting a deployment's directory does not delete what docker holds for it:
its volumes, with the models' data and any database, stay until something
removes them, and once the directory is gone no chap-devops command names them any
more. `chap-devops cleanup` is that something:

```sh
chap-devops cleanup --dry-run     # list what would go, change nothing
chap-devops cleanup               # list it, ask, then delete
chap-devops cleanup --yes         # delete without asking, for a script
```

A deployment counts as removed only when chap-devops can prove it. Every time
chap-devops saves a deployment it records its compose project name and its
directory in `deployments.yaml` in its data directory (`$CHAP_DEVOPS_DATA_DIR`,
else `$XDG_DATA_HOME/chap-devops`, else `~/.local/share/chap-devops`). A recorded
deployment is removed when its directory was deleted from a parent directory
that is still there, or when the directory now holds a deployment of another
name (`chap-devops init --force` wrote over it). Only the name in
`.chap-devops/project.yaml` is read, so a typo in another state file never makes a
deployment look removed. Everything else is kept and named in a warning of
its own: a `project.yaml` that cannot be read, a directory missing along with its
parent (as on a disk that is not mounted), and a removed deployment that
still has containers (the line names the `docker compose -p <project> down`
that removes them). Volumes of a compose project chap-devops never recorded are not
touched, and neither is a volume any container still mounts.

A deployment you moved to another directory is recorded again the next time
any chap-devops command saves it there; until then it looks removed, so run
`chap-devops sync` in its new place before `chap-devops cleanup`.

## Project checks

When `docker info` does not answer - Docker Desktop not started, or a daemon
that is wedged - the project lines that would have to ask it (`volumes`,
`user <model>`, `health`) are skipped with "the docker daemon is not
answering" rather than asked: those calls have no deadline of their own, and
an answer of nothing from a daemon that is down would read as "no volumes" or
"nothing running".

These only run when the command found a deployment, which it does the way git
finds `.git`: from the current directory (or `-C DIR`) upwards. Outside one,
`doctor` prints the machine half. With `-v`, a hint under the summary says why
the project checks are not there:

```text
hint: no deployment here, so the deployment checks did not run; run `chap-devops doctor` in a deployment directory for them
```

| Check | What it asks | What a bad answer means |
| --- | --- | --- |
| `compose project` | what compose calls this deployment, and whether that name is its own | Warns when another directory records the same compose project name, as `chap-devops backup restore --adopt-identity` on the same machine leaves it: the two directories then use the same containers and volumes. Warns when `compose_project` in `.chap-devops/project.yaml` is empty. Then compose names the deployment after its directory, and another deployment in a directory of the same name shares every container name and every named volume. See [the compose project name](./concepts.md#the-compose-project-name). |
| `volumes` | which named volumes docker holds under this deployment's name, whether the database one is this deployment's, and whether the rest still belong to something it enables | Warns when `<project>_chap-db` was created before the `.chap-devops/` directory was: it belongs to something else, most often an earlier deployment of the same name, and it still holds that deployment's role password - which is exactly the state in which chap-core cannot log in and never becomes healthy. That comparison is skipped where the filesystem records no creation time. Failing that, it warns about leftover volumes: a `ck_<id>_data` whose model is no longer in `.chap-devops/models.yaml`, or an `ocs_data`, `s3_data`, `dhis2_home`, `dhis2_db` or `dhis2_dump` whose component is off. Their data is kept on purpose, so that a model or component enabled again finds it, but nothing declares those volumes any more and `chap-devops down --volumes` no longer reaches them - `chap-devops models disable <id> --purge`, `chap-devops components disable <name> --purge` or `docker volume rm <name>` is what removes one. While the `ocs` container is down, the line also says how much its data volume holds (`; mychap-1ab2c3_ocs_data holds 3.0 GB`), so an operator can see how much data a `chap-devops down --volumes` would destroy before typing it. While that container is up the size is not measured here at all, because the `components` line already has it from inside the container. The size comes from a `du -sk` in a throwaway busybox mounting the volume read-only, bounded at ten seconds, because neither `docker volume inspect` nor `docker system df -v` reports one reliably. |
| `deployment files` | are `.chap-devops/project.yaml`, `.chap-devops/models.yaml`, `.chap-devops/components.yaml`, `.env`, `compose.yml`, `compose.chap-devops.yml` and `compose.marketplace.yml` all there, plus one file per enabled component | It names the missing ones. `chap-devops sync` renders the compose files again; `chap-devops init --force` writes the whole deployment. A deployment with `chap-core` disabled is not asked for the base compose file, which it does not have. Files in a component's own directory (`ocs/`, `s3/`, `dhis2/`) are not counted here. The `ok` line names those directories, because the `components` line checks them. |
| `components` | which components this deployment has, and whether the files they need are there | Fails when `ocs` is on and `ocs/climate-service.yaml` is missing, or when `dhis2` is on and `dhis2/dhis.conf` is missing. Warns while `ocs/climate-service.yaml` holds the example values. Also reports facts it does not judge; see [The components line](#the-components-line). |
| `compose files` | `chap-devops sync --check`, without its exit code | The rendered files no longer match `.chap-devops/`, which is drift the next `chap-devops up` would undo anyway. Run `chap-devops sync`. |
| `.env` | is it readable, and does it still say what it should | Warns when `POSTGRES_PASSWORD` is the default `chap` or unset, and when the `CHAP_IMAGE_TAG` pin comment is gone. Whether authentication is on is reported either way: `auth: off` is a fact to be able to see, not a fault. |
| `api workers` | how many API worker processes `CHAP_API_WORKERS` in `.env` asks for, and whether this chap-core can run them | Left out when the deployment has no chap-core. Warns when the value is not a whole number of 1 or more, and when it is more than 1 but the deployment's `compose.yml` has no `WEB_CONCURRENCY`: that is chap-core v2.7.0 or older, where the workers share the database connections of the main process and requests fail just after a start. The fix is `CHAP_API_WORKERS=1`, or a newer chap-core with `chap-devops update --chap-tag master`. See `CHAP_API_WORKERS` in [Concepts](./concepts.md#what-env-holds). |
| `api port`, `port <model>`, `port <component>` | is every host port the deployment publishes free | Fails when something else holds one, with the same line `chap-devops up`'s preflight prints for that port: free it, or move it. A port held by this deployment's own running container is not a conflict, exactly as `up` treats it: the line says `is held by this deployment's own container`. See [Ports](./ports.md). |
| `manual <id>` | is a model added with `chap-devops models add` still on the newest build its branch has published (one line per such model) | Warns when the branch has moved on: `chap-devops update` moves the pin. A model added from an image reference is pinned on purpose and skipped as such, and a repository or registry that would not answer is a check that was not made rather than a fault in the deployment. Skipped under `--offline`. See [Models outside the marketplace](./models.md#models-outside-the-marketplace). |
| `chap-core pin` | is the pinned tag still the newest release (left out when `chap-core` is not a component of this deployment) | Warns when a newer chap-core has been released: `chap-devops update --dry-run` says what would move. A moving tag (`latest`, `master`, `dev`) is not behind anything, so it passes whether or not the list was read, and says which build it is on instead - `dev (moving tag, running 7f3a1c2e9b4d)` - plus the two commands that leave it: `chap-devops update` re-pulls it, `chap-devops update --pin-chap-core` pins a release. The digest comes from the image chap-core's container runs and is left out when nothing is running. See [Switching chap-core's tag](./updating.md#switching-chap-cores-tag). Skipped when the release list was not reachable, and under `--offline`, where it was never asked for. |
| `image <model>` | does each enabled model's exact tag exist on ghcr, with a linux/amd64 image | `docker manifest inspect` per model, in parallel, ten seconds each, of the tag compose will pull: the model's tag variable in `.env` when it sets one, the pin otherwise. A tag that is gone ("manifest unknown") fails and names the model to re-resolve with `chap-devops models enable <id>`; a registry that refuses or fails without saying - a rate limit, a 5xx, `unauthorized` - is a warning that quotes it, because it says nothing about the tag. Skipped, with the reason on the line, under `--offline`, when there is no docker CLI to ask through, and when `ghcr.io` was unreachable. `--offline` is the reason given first, so the line reads the same on a machine with Docker and one without. |
| `registry pin <id>` | is the commit the marketplace pins for each enabled model still the newest build that model's own repository has published (one line per enabled marketplace model) | Warns when the catalogue is behind: the model repository has built a newer commit than the version this deployment's channel resolves to, which is the state in which every deployment runs a fortnight-old image without anything saying so. No `chap-devops` command moves that pin - it lives in the marketplace, and `chap-devops models add` refuses an id the catalogue already lists - so the line asks for a new pin there. A commit the branch has not built yet, and one taken from a branch or a tag of its own, are ahead rather than behind and pass. The default branch, one commit listing and at most five registry lookups per model, run in parallel; a repository that will not answer, a rate limit and a branch with no published build are all skips with the reason on the line. Models added with `chap-devops models add` are left out, because `manual <id>` above already asks the same question. Skipped under `--offline`. See [Channels and versions](./models.md#channels-and-versions). |
| `user <model>` | does the account the overlay runs each enabled model as still match what its image declares (one line per enabled model) | `docker image inspect --platform linux/amd64` of the exact tag the overlay pins, read from the local daemon only - no network, so this line is answered under `--offline` too. Warns when the two have drifted apart, which is what a `user:` written by hand, or an image that changed its account, looks like: an image that runs as root under `user: 1000:1000` starts fine and then fails on its own binaries. `chap-devops models enable <id>` reads the account off the image again, and `chap-devops update` does it for a model it moves to a new tag. Skipped when the amd64 variant of the tag is not in this machine's image store - the line names the `chap-devops docker pull` that would put it there, which is also what an arm64 host holding only its own architecture is told - when the docker daemon is not answering, and when the image runs as an account name only the image itself can turn into numbers. See [Data directories and users](./models.md#data-directories-and-users). |
| `image <component>` | does each enabled component's image exist | The same `docker manifest inspect`, of the tag `OCS_IMAGE_TAG`, `S3_IMAGE_TAG` or `DHIS2_IMAGE_TAG` in `.env` sets when it sets one, with the same split between a missing tag and a registry that did not say, and without the linux/amd64 question: every component image is multi-arch and no component pins a platform, so "the tag exists" is the whole answer. Skipped for the same three reasons. |
| `health` | chap-core's health and whether every model registered | Skipped when no container of this deployment is running, which is `chap-devops up`. Otherwise it is the verdict of [`chap-devops status`](./status.md) as one line. A `chap` container that is up and failing its healthcheck is reported as such, with the last error line from its own log and the fix that goes with it. A deployment without chap-core is judged by its components instead. When every model **is** registered the line is `ok` and still carries a next step - `chap-devops models test --all` - because registration is only a heartbeat: the check that settles whether a model can predict has to run the models, which takes minutes and is therefore not something `doctor` does itself. See [Testing a model](./models.md#testing-a-model). |

### The components line

The line starts with the names of the enabled components. Then it adds a part
for `ocs` and a part for `dhis2`, if they are enabled. The line takes the worse
status of the two parts, so one part cannot hide the other.

For `ocs`:

- If `ocs/climate-service.yaml` is missing, the line fails: the container
  starts with no instance configuration. `chap-devops sync` writes the file again.
- If the file still holds the Laos example values, the line warns. Edit the
  file and delete the note at the top.
- The line reports `ERA5-Land: credentials unset (WorldPop and CHIRPS3 work
  without them)` when `.env` sets none of the data source variables. This is
  not a problem: WorldPop and CHIRPS3 need no credentials.
- The line reports `plugins/: N files` when `ocs/plugins/` exists.
- While the `ocs` container runs, the line also reports the same two facts
  that [`chap-devops status`](./status.md) shows: `3 datasets` from
  `GET /datasets?f=json`, and `212.0 MB data` from `du -sk /app/data`. Both
  have a time limit, and the line leaves out a fact it could not get.

These facts come with the `ok` line and with the warning. The line under the
warning stays the one thing to do. A missing file shows none of the facts.

For `dhis2`:

- If `dhis2/dhis.conf` is missing, the line fails: DHIS2 does not start without
  it. `chap-devops sync` writes the file again.
- The line reports the seed: `none`, the dump, or `default` with the dump that
  `default` gives. If chap-devops has no dump for the minor version, the line says
  that the database starts empty. An empty database is not an error.
- If chap-core is enabled, the line reports the last `chap-devops dhis2 connect`, or
  ``no `chap-devops dhis2 connect` recorded``. This is not a verdict: `doctor` does not
  ask DHIS2. `chap-devops dhis2 show` asks DHIS2.

A deployment with `dhis2` enabled also gets the `memory` check. See
[The seed](./dhis2.md#the-seed) and [Components](./components.md).

## `--json`

The same checklist as one document: an array of checks, the counts they add
up to, and the closing lines in `messages`.

```sh
chap-devops doctor --json
```

```json
{
  "checks": [
    {
      "id": "docker-cli",
      "name": "docker cli",
      "status": "ok",
      "detail": "docker 29.8.1",
      "fix": null
    },
    {
      "id": "api-port",
      "name": "api port",
      "status": "fail",
      "detail": "8700 is in use by something else",
      "fix": "port 8700 is already in use on this machine (needed by chap); free it, or set CHAP_API_PORT=8701 in `.env`"
    }
  ],
  "summary": { "ok": 1, "warn": 0, "fail": 1, "skip": 0 },
  "messages": [
    { "level": "info", "text": "2 checks: 1 ok, 0 warn, 1 fail" }
  ]
}
```

`id` is stable, so a script can pick one line out of the report:

```sh
chap-devops doctor --json | jq -r '.checks[] | select(.status == "fail") | "\(.id): \(.detail)"'
```

Outside a deployment directory the project checks are simply absent from
`checks`, which is how a consumer tells the two cases apart.

## `--offline`

`--offline` skips every check that would touch the network, reporting each as
`skip` with the reason, and the reason always names the flag:

```text
skip  network ghcr.io                              --offline: the image registry was not probed
skip  network marketplace                          --offline: the model catalogue was not probed
skip  network releases                             --offline: the chap-core release list was not probed
skip  github api                                   --offline: the GitHub API rate limit was not asked
skip  chap-devops                                        v0.102.0, aarch64-apple-darwin, cargo build; --offline: the release list was not asked
skip  chap-core pin                                v2.4.0 pinned; --offline: the release list was not asked
skip  image chapkit-simple-multistep-model         --offline: the registry was not asked
skip  registry pin chapkit_simple_multistep_model  --offline, so the model repositories were not asked about newer builds
```

`--offline` is the reason on every one of those lines whatever else the
machine has: a host without a docker CLI has no image manifest to ask for
either, but the flag is the reason the user gave, so it is the reason
reported, and the missing CLI is already a failure on the `docker cli` line.
That makes the offline report the same everywhere, which is what a test or a
CI step can rely on.

Everything else still runs, so `chap-devops doctor --offline` is the whole local
half of the checklist on a machine with no way out.
