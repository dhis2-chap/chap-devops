# Running one model

`chap-devops run` starts one model service and prints where it answers. There is no
`chap-devops init` first and no directory to keep: name the model, by marketplace id,
GitHub repository or image, and use the URL.

```sh
chap-devops run chapkit_simple_multistep_model
chap-devops run https://github.com/chap-models/chapkit_ghr_model
chap-devops run ghcr.io/my-org/chapkit_dengue_model:sha-1eb8cf1
chap-devops run my-dengue-model:dev                 # built on this machine
```

```text
starting chapkit-simple-multistep-model (chapkit_simple_multistep_model in /home/me/.local/share/chap-devops/run/default)
running chapkit_simple_multistep_model on http://localhost:5001 (answered in 4s)
```

With `-v`, two hints follow: the command that stops the model, and the command
that shows its log:

```text
hint: `chap-devops stop chapkit_simple_multistep_model` stops it
hint: `chap-devops -C /home/me/.local/share/chap-devops/run/default logs chapkit-simple-multistep-model` shows its log
```

The command returns once the model answers its own `/health`, so the URL works
when the line is printed. With `--no-wait`, the line starts with `started`
instead of `running`, and `chap-devops ps` shows when the model answers. A first run pulls the image, which takes a while for
the R-INLA models; `--timeout SECONDS` (300 by default) is how long it waits
before it fails naming the state the model was in.

## In the foreground: `-a`

`-a` (also `--attach` or `--foreground`, as on `chap-devops up`) keeps `chap-devops run` in
the foreground. After the URL, it shows the model's log, and Ctrl-C stops the
model:

```text
running chapkit_simple_multistep_model on http://localhost:5001 (answered in 4s)
following the log of chapkit-simple-multistep-model; Ctrl-C stops it
...
stopped chapkit_simple_multistep_model; its data stays, and `chap-devops run chapkit_simple_multistep_model` starts it again with it
```

Ctrl-C takes the model out of its group, as `chap-devops stop` does. The model's data
volume stays, so the next run starts with the model's trained state. `--rm`
removes the data volume too, and the group when it has no model left
(`removed group default`), as `chap-devops stop --purge` does. A second Ctrl-C exits at once. A model that ran
before the command is not stopped: Ctrl-C then stops only the log. With
`--no-wait`, you watch the model start in its log. `--json` always returns.

**Ctrl-C during the start**, with or without `-a`, takes back out what the start
put in: a model that this run enabled goes back out of the group, as after a
failed start. The command then exits with 130:

```text
error: stopped by Ctrl-C; chapkit_simple_multistep_model is taken back out of /home/me/.local/share/chap-devops/run/default
```

## Your own chap-core

A group has no chap-core of its own, so its models register nowhere.
`--chap-core URL` makes them register with a chap-core that runs elsewhere,
for example one from its own checkout:

```sh
chap-devops run -a --rm --chap-core http://localhost:8000 https://github.com/chap-models/chapkit_ghr_model
```

```text
running chapkit_ghr_model on http://localhost:5001 (answered in 2s)
registered with http://localhost:8000
```

- chap-devops records the chap-core on the group, as `chap-devops components enable
  chap-core --url` does in a deployment. Every model of that group then
  registers with it. Use `--group NAME` for a group of its own.
- chap-devops decides where chap-core calls the models back. A chap-core in a
  container on this machine calls `host.docker.internal`, and a chap-core
  process calls `localhost`. `--models-host HOST` sets it.
- After the start, chap-devops asks chap-core whether it lists the model, and says
  `registered with URL`, or gives the warning `not registered with URL`. "Up" means chap-core
  answered, not only that the container started.
- A group that had another chap-core moves to the new one. Its models that run
  now register with the old one until they restart, and chap-devops says so.

In a deployment of your own, `--chap-core` is refused: set the chap-core there
with `chap-devops components enable chap-core --url URL`.

## What the model can be

| `MODEL` | What `run` does |
| --- | --- |
| A marketplace id or service id: `chapkit_simple_multistep_model` | Enables it at the catalogue's stable version. |
| A GitHub repository the marketplace lists | The same: the catalogue's reviewed pin, not the newest build of the branch. |
| Any other GitHub repository URL | Adds it the way [`chap-devops models add`](./models.md#models-outside-the-marketplace) does: the newest published `sha-` build of the default branch. Needs the network. |
| An image on a registry, with a tag or digest | Pins exactly that image. A tag the marketplace publishes for one of its models enables that marketplace version instead. |
| A local image, `name:tag` with no registry | Runs the image from the local store, never pulled. |

A model that is already enabled is started as it is: a second `chap-devops run` of
the same id does not move its version or its port. A run by the repository URL
of an enabled marketplace model, or by its image at the tag of the enabled
version, finds that model and starts it as it is. The same holds for a
repository or an image: one added before, by an earlier `chap-devops run` or
`models add`, is that entry again, enabled or not, so running the same URL twice
starts one model, not two. An image counts as the same only at the same tag.
`--id` names an added model (`--id auto` picks a free one), exactly as on
`models add`; an `--id` other than the one the source was added under adds a
second copy beside it.

A word with no `:`, `/` or `@` is a marketplace id, never a local image (a local
image always carries a tag), so `chap-devops run does_not_exist` says there is no such
model and names `chap-devops models search`. A marketplace starter is refused like it
is on `models enable`; `--allow-starter` starts it anyway.

When the container cannot be started - the image does not exist, the pull is
denied, the port is taken - `run` says what compose said and takes the model it
enabled back out, so `chap-devops ps` does not list it and the next `chap-devops run` of the
same source starts afresh. The definition of an added model stays.

## Where it runs: groups

Outside a deployment, `chap-devops run` keeps its models in a *group*: a deployment
chap-devops owns under its data directory, `run/<group>/`. The data directory is
`$CHAP_DEVOPS_DATA_DIR`, else `$XDG_DATA_HOME/chap-devops`, else `~/.local/share/chap-devops`.
`--group NAME` picks one; without it the group is `default`. The first
`chap-devops run` into a group creates it, as `chap-devops init --only none --models none`
would, so there is no chap-core and no component in it: only model services,
each on its own host port.

Groups keep models apart. Each one is a compose project of its own, so its
containers, volumes and network carry its name, and stopping a group touches
nothing else. `.chap-devops/project.yaml` records the group as `group: <name>`, and
every container in it carries the labels `io.github.dhis2-chap.chap-devops.kind: run` and
`io.github.dhis2-chap.chap-devops.group: <name>`, so
`docker ps --filter label=io.github.dhis2-chap.chap-devops.group=trial` lists one group's
containers ([Container labels](./concepts.md#container-labels)):

```sh
chap-devops run chapkit_simple_multistep_model                    # group default
chap-devops run auto_arima_chapkit --group trial
chap-devops run chapkit_ghr_model --group trial
chap-devops ps
```

```text
GROUP    ID                              SERVICE                         STATE        URL
default  chapkit_simple_multistep_model  chapkit-simple-multistep-model  up           http://localhost:5001
trial    auto_arima_chapkit              auto-arima-chapkit              up           http://localhost:5002
trial    chapkit_ghr_model               chapkit-ghr-model               not running  http://localhost:5003
```

With `-v`, a hint after the table names the directory that holds the groups,
for example `hint: groups live in /home/me/.local/share/chap-devops/run`.

`--port auto` takes the lowest port in 5001-5999 that nothing on the machine
holds and no group has given to a model, running or stopped, so two groups
never collide.

Inside a deployment of your own - the working directory, or `-C DIR` - `run`,
`ps` and `stop` work on that deployment instead, and `--group` is refused,
because the deployment is the group. So a tool that only wants "start this
model and give me its URL" has one code path whether or not someone ran
`chap-devops init`.

## Only this machine

A group publishes every model port on `127.0.0.1`, so a model `chap-devops run`
started is reachable from this machine and not from the network: models have no
login. `--bind 0.0.0.0` publishes one on every address instead. Inside a
deployment, `run` publishes as that deployment does. See
[Ports](./ports.md#which-address-a-model-port-is-published-on).

## Listing and stopping

`chap-devops ps` lists every model in every group, or in one with `--group`, with the
state `chap-devops status` would give it: `up` when its `/health` answers,
`running, not answering` while it starts or when it failed to,
`not running` when its container is gone.

When it finds no model, `chap-devops ps` says where it looked:

- `` there is no `chap-devops run` group X; `chap-devops ps` lists every group ``, for a
  `--group X` that does not exist.
- `` no model runs in a `chap-devops run` group ``, when the groups exist and have no
  model.
- `` nothing has been started with `chap-devops run` yet ``, when there is no group.

`-C` on a group that `chap-devops stop --purge` removed names the command that makes
it again: `` the `chap-devops run` group X does not exist, or `chap-devops stop --purge`
removed it; `chap-devops run <model> --group X` makes it again ``. `chap-devops ps`,
`chap-devops stop`, `chap-devops run`, `chap-devops chap`, `chap-devops logs` and `chap-devops status` all
give this error. A `-C` to another directory that is not a deployment, or to
one that does not exist, gives `DIR is not a chap-devops deployment`. So `chap-devops ps`
does not list every group, and `chap-devops stop --all` does not stop every model,
because of a wrong `-C`.

`chap-devops stop ID` stops a model and takes its overlay away, in whichever group
has it; when two groups do, it asks for `--group`. `chap-devops stop --all` stops
every model in every group, and `chap-devops stop --group NAME` every model in one.
The data volume stays, as with `models disable`, and so does the definition of
an added model, so `chap-devops run ID` starts it again without asking GitHub.
`chap-devops stop` prints one line for each model it stopped. With `-v`, hints say
what became of the container and the volume, and how to start the model again:

```text
$ chap-devops -v stop chapkit_simple_multistep_model
stopped chapkit_simple_multistep_model
hint: stopped and removed 2 containers (chapkit-simple-multistep-model, chapkit-simple-multistep-model-init); host port 5001 is free again
hint: kept volume default-1ab2c3_ck_chapkit_simple_multistep_model_data; remove it with `chap-devops -C /home/me/.local/share/chap-devops/run/default models disable chapkit_simple_multistep_model --purge` or `docker volume rm default-1ab2c3_ck_chapkit_simple_multistep_model_data`
hint: `chap-devops run chapkit_simple_multistep_model` starts it again
```

`--purge` takes the volume too, and the group it leaves with no model in it
goes as well: its network, its directory and every volume compose made for
it, including the volume of a model stopped earlier without `--purge`. Only
that group: another group with no model left, emptied by an earlier plain
`stop`, keeps its data. `chap-devops stop --group NAME --purge` empties and removes
that one group, and `chap-devops stop --all --purge` every group. A volume docker
will not remove keeps its group, so the same `chap-devops stop --group NAME --purge`
can finish the job once the volume is free.

The deployment behind a group is an ordinary chap-devops deployment, so every other
command works on it with `-C`: `chap-devops -C ~/.local/share/chap-devops/run/default logs
chapkit-simple-multistep-model`, `chap-devops -C ... status`. Messages from inside a group name
their commands that way.

## For tools

Every one of the three takes `--json` and prints one document on stdout:

```json
{
  "bind": "127.0.0.1",
  "enabled": true,
  "group": "default",
  "id": "chapkit_simple_multistep_model",
  "messages": [...],
  "ok": true,
  "port": 5001,
  "project_dir": "/home/me/.local/share/chap-devops/run/default",
  "service_id": "chapkit-simple-multistep-model",
  "url": "http://localhost:5001",
  "wait": {"ready": true, "waited_s": 4, "models": [...]}
}
```

Each document also has the `messages` list of every closing line with its
level; see [Output](./status.md#output). `chap-devops ps --json` is
`{"models": [...]}` with the same fields per model plus `state`; `chap-devops stop --json` lists what it `stopped`, the groups it
`removed` and the volumes that went with them, `removed_volumes`. A failure is
`{"ok": false, "error": ..., "hint": ...}` like every other command's, a usage error included; see
[`--json` for scripts and tools](./commands.md#--json-for-scripts-and-tools).
docker's own progress, such as a pull, goes to stderr, so stdout is the one
document.

Two `chap-devops run` started at the same time into one group wait for each other
rather than writing over each other, and land on different ports; see
[One change at a time](./concepts.md#one-change-at-a-time). That holds for a
group neither of them has created yet, and for two runs of the same model,
which start it once and both report it.
