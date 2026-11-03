# Troubleshooting

Start with `chap-devops doctor`. It runs the whole checklist in one pass - Docker,
Compose, the architecture, free disk, the hosts Chap pulls from, and, inside a
deployment, the files, the ports, the pins, the images and whether Chap is up -
and prints what to do about every line that is not `ok`. Most of the sections
below are one of its lines with the reasoning spelled out. See
[Doctor](./doctor.md).

When a command does something you did not expect, run it again with `-vv`. It
prints every external command, every HTTP request with its status and timing,
which catalogue it loaded and which files `sync` compared, all on stderr and
all out of the way of `--json`. `-d` adds the response bodies and the resolved
project paths.

## the restore of the DHIS2 seed stopped before the end

`chap-devops up` printed this under `why dhis2-db is unhealthy:`. The restore of the
seed dump into `dhis2_db` stopped with an error, so the database is
incomplete. The lines above the hint say why. Two causes are common:

- The dump is not a complete gzip file. Download it again.
- The dump needs a newer PostgreSQL than 16, the version of `dhis2-db`. chap-devops
  removes the lines of `pg_dump` 17 that PostgreSQL 16 does not know
  (`\restrict`, `\unrestrict`, `SET transaction_timeout`), but a dump can
  use other new features.

Fix the cause, then start the restore again:

```sh
chap-devops components disable dhis2 --purge
chap-devops components enable dhis2
chap-devops up
```

`--purge` removes only the three DHIS2 volumes. `seed:` and `seed_password:`
in `.chap-devops/components.yaml` stay. Do not use `chap-devops down --volumes` for this:
it also removes the chap-core database. See
[The seed](./dhis2.md#a-restore-that-stops).

## A host port is already in use

`chap-devops up` refuses before it calls Docker, with one line per conflict:

```text
error: 2 host ports this deployment needs are already in use; nothing was started
  port 8700 is already in use on this machine (needed by chap); free it, or set CHAP_API_PORT=8701 in `.env`
  port 5001 is already in use on this machine (needed by chapkit-simple-multistep-model); free it, or run `chap-devops models unexpose chapkit-simple-multistep-model` (the model stays reachable through chap-core) / `chap-devops models expose chapkit-simple-multistep-model --port auto`
  or run `chap-devops up --no-preflight` to hand the conflict to Docker
```

Pick one of the three: free the port, move the API with `CHAP_API_PORT` in
`.env`, or move (or drop) the model's port. Ports held by this deployment's own
running containers are not conflicts, so `chap-devops up` on a running deployment is still
a no-op. `chap-devops up --no-preflight` hands the question back to Docker.

When the port belongs to another chap-devops deployment that is running, that one is
named, with the command that stops it. A stopped deployment is not named:

```text
1 host port this deployment needs is already in use; nothing was started
  port 8790 (needed by ocs) is in use, and climate (/srv/climate) publishes it too; stop it with `chap-devops -C /srv/climate down`, or move this one: run `chap-devops components enable ocs --port 8791`
  or run `chap-devops up --no-preflight` to hand the conflict to Docker
  or run `chap-devops up --replace` to stop climate first
```

At a terminal it asks instead of stopping there:

```text
climate is using these ports. Stop it and start this deployment instead? Its data is kept. [y/N]
```

`y` runs `chap-devops down` on the other deployment (its volumes and data stay; its
own `chap-devops up` brings it back) and then starts this one. `chap-devops up --replace`
is the same answer given in advance, for a script. The offer is only made
when every taken port belongs to another chap-devops deployment: a port some other
program holds is never chap-devops's to take away.

## The port answers, but it is not chap-core

```text
chap-core   down   http://localhost:8700   v2.3.1 (pinned)   auth: off
...
error: chap-core at http://localhost:8700 is not responding: port 8700 answers
but it is not chap-core (got text/html)
```

Something else holds the port: a dev server, a proxy, an older deployment.
`chap-devops status` reports it as down on purpose, because `up` has to mean that the
deployment works and not that the port is taken. Find the listener, or point
the deployment somewhere else with `CHAP_API_PORT` in `.env`.

## `` this deployment has no container for `chap`, so there is no log ``

```text
error: this deployment has no container for `chap`, so there is no log; start it with `chap-devops up`
```

`chap-devops logs chap` found no container for the service, so the service never
started and has no log. The command exits with status 1. Run `chap-devops up`, then
`chap-devops logs chap` again.

`chap-devops status` says the same thing about chap-core:

```text
error: chap-core at http://localhost:8700 is not responding: ...; this deployment has no `chap` container yet, so start it with `chap-devops up`
```

Run `chap-devops up`.

## `this deployment's chap-core is not running`

```text
this deployment's chap-core is not running; http://localhost:8700 is first (/srv/first) answering on the same port; stop it with `chap-devops -C /srv/first down`, or run `chap-devops up --replace` here
```

Two deployments made with the same ports take turns on them, and the one that
is up answers on the port for both. `chap-devops status` asks docker whether this
deployment's own `chap` container is running before it believes an answer on
its port: when it is not, the answer is the other deployment's, and none of what
it says (health, version, registered models) is about this one. A browser on
`http://localhost:8700` shows the other deployment for the same reason.

Stop the other one with the `chap-devops -C ... down` it names, or let this one take
the ports with `chap-devops up --replace`. `chap-devops status --url` asks whatever URL it
is given and skips this check. See
[Several deployments on one machine](./use-cases/several-deployments.md).

## A model is running but not registered

```text
chapkit-rwanda-malaria-bym-model  running, not registered  via chap-core  -
```

A model that started under two minutes ago is still starting, and registering
is the last part of that; `chap-devops status` says so for that row
(`started under two minutes ago and registers once it is ready`) and the fix is
to run it again in a minute. Past that, it is stuck.

chapkit tries to register with chap-core five times during its startup and then
stops. A model that came up before chap-core was healthy therefore stays
invisible until it is restarted:

```sh
chap-devops restart --all chapkit-rwanda-malaria-bym-model
chap-devops status
```

`--all` because nothing about the service has changed: it is running the image
and the configuration it should be, and the restart is only there to make it
introduce itself again. A plain `chap-devops restart` recreates what moved, which
here is nothing.

The overlay's `depends_on: chap: {condition: service_healthy}` is there to stop
this happening in the first place, so a model in this state usually means
chap-core became unhealthy and came back after the model had given up.

If registration is failing rather than racing, and the deployment has
authentication on, see
[the models stopped registering](#the-models-stopped-registering-after-i-turned-authentication-on)
below.

## `chap-devops models test` says a model failed

```text
chapkit-rwanda-malaria-bym-model    FAIL   14s   predict: Error in file(file, "rb") : cannot open file 'model.rds': Permission denied
```

The row is one clause out of the model's own error stream, which is as much as
a row has room for. Three places have more, in the order worth reading them:

```sh
chap-devops models test chapkit_rwanda_malaria_bym_model -vv  # the whole run, as it happens
chap-devops logs chapkit-rwanda-malaria-bym-model             # what the service itself said
```

`-vv` streams everything `chapkit test` printed, which includes the phase that
failed, the diagnostic artifact it stored and the last lines of the model's
stderr. `chap-devops logs <service>` is the other half: a model that cannot open a
file usually said so on startup too.

With `--backtest` the work happened inside chap-core, so the log is a job's:

```sh
chap-devops jobs                          # the backtest that failed, newest first
chap-devops jobs logs <id>                # the traceback and the model's own output
```

The failure row already names that command with the id filled in. See
[Jobs and the API](./jobs.md).

The two levels fail for different reasons, which is what makes running both
worth it:

- Model level fails, `--backtest` not tried: the model itself is broken.
  Permission denied on its own files is the common one - see
  [`Permission denied` from a model's own binaries](#permission-denied-from-a-models-own-binaries)
  - followed by a missing runtime library, which is a fault in the image and
  needs a newer pin (`chap-devops update`).
- Model level passes, `--backtest` fails: the model works and the round trip
  does not. The usual cause is a covariate the model reads and does not
  declare, which shows up as a `KeyError` from chap-core rather than as
  anything the model said:

  ```text
  chapkit-simple-multistep-model    FAIL    8s   KeyError: "['mean_relative_humidity'] not in index"
  ```

  The sample data is generated from the covariates the service declares
  (`chap-devops models info <id>`, or
  `chap-devops api GET /v2/services/<service_id>`), so a model whose configured model
  in chap-core asks for more than that gets a frame without it. That is a fault
  in the model's own declaration, not in the deployment.
- Both fail the same way: the model, again. Fix it at the model level, where
  the loop is seconds rather than minutes.

A `--seed N` makes the generated data the same on every run, which is what to
add when a failure only happens sometimes.

## ``an MLproject service has no `chapkit test` ``

```text
rain-model    skip    1s   an MLproject service has no `chapkit test`
  go through chap-core with `chap-devops models test rain_model --backtest`
```

The image serves an `MLproject` file with `chapkit mlproject run`, as the
`mlproject-*` types of `chap-devops models new` do. The image has chapkit, but
chapkit has `chapkit test` only in a chapkit service project. A newer image
does not change this. Test the model through chap-core with `--backtest`, or
make the model a chapkit service with `chap-devops chapkit mlproject migrate DIR`. See
[Migrate an MLproject model](./chapkit-cli.md#migrate-an-mlproject-model).

## `` `chapkit test` in this container cannot reach a model service ``

`chap-devops chapkit` runs chapkit in a container that is on no network of a model,
and the container stops when the command ends. So `chapkit test` has no
service to test. Use `chap-devops models test ID`: it runs `chapkit test` in the
model's own container. For a model image that you built, start it first with
`chap-devops run IMAGE`.

## `No Python (.py) or R (.r/.R) scripts at the project root or in the entry-point commands; cannot pick a base image.`

`chapkit mlproject migrate` selects the base image from the scripts at the root
of the model directory and the scripts that the train and predict commands
name, and found none. Give the image with `--base-image`: `chapkit-py`,
`chapkit-r`, `chapkit-r-tidyverse` or `chapkit-r-inla`. chap-core v2.5.0 and
older look at the root only, and say `No Python (.py) or R (.r/.R) scripts at
the project root`; there, the `mlproject-*` templates need `--base-image`, or
`--tag v2.5.1`. See
[Migrate an MLproject model](./chapkit-cli.md#migrate-an-mlproject-model).

## `Only ShellModelRunner services can be exported`

`chapkit mlproject export` writes an `MLproject` with train and predict
commands, so the service must run scripts. A model in `main.py`, as in the
`fn-py` template, has no commands to write. Move the model to scripts, as in
the `shell-py` template, or keep it a chapkit service.

## `No such file or directory: 'config.yml'` from `chap-devops chap`

The `MLproject` came from `chapkit mlproject export`. Its commands read a
`config.yml` that chapkit writes, and chap writes it only with
`--run-config.as-chapkit`. Add that option, or serve the model with
`chap-devops chapkit mlproject run DIR`. See
[Export a service as an MLproject](./chapkit-cli.md#export-a-service-as-an-mlproject).

## `the MLproject service on ... stopped, because its container got SIGTERM`

`chap-devops chapkit mlproject run` served a model, and something outside chap-devops
stopped its container: `docker stop`, or a restart of Docker. chapkit shut
down normally, and `--rm` removed the container. To serve the model again, run
the same `chap-devops chapkit mlproject run` command.

## `has changes that are not committed, and the service reports the last commit`

`chap-devops chapkit mlproject run` gives chapkit the git commit of the model
directory. chap-core stores each version of a model with one revision, so a
change that is not committed would run under the revision of the last commit.

1. Commit the changes.
2. Raise `version` in `MLproject`.
3. Run `chap-devops chapkit mlproject run DIR` again.

## `is not a git checkout, so chap-core cannot store the model`

The service registers with chap-core, but chap-core stores a model only with
a git revision. Without it, `chap-devops models configs sync` and
`chap-devops models test ID --backtest` fail with
`is stored from revision None`, or `its source reports no git revision` from
chap-core v2.6.0. In the model directory, run `git init`,
then `git add .` and `git commit`. Then run `chap-devops chapkit mlproject run DIR`
again.

## `chapkit did not accept the arguments, and its usage is above`

chapkit exited with status 2, which is a usage error: a command, an argument
or an option is missing or wrong, for example an option that the command does
not have. The usage that chapkit printed is above the line. The line names the
help to read, such as `chap-devops chapkit artifact --help`.

A group with no command after it, `chap-devops chapkit artifact` or
`chap-devops chapkit mlproject`, is not an error: chap-devops asks chapkit for the help of
the group.

## ``could not write DIR/uv.lock, and the `Dockerfile` copies it``

`chapkit init` and `chapkit mlproject migrate` write a `Dockerfile` that copies
`uv.lock`. chap-devops runs `uv lock` in the chap-core image to write it, and this
time it failed. Without the file, `docker build` fails with
`"/uv.lock": not found`.

1. Run `chap-devops chapkit --help` to make sure that the image runs.
2. Run `chap-devops chapkit init` or `chap-devops chapkit mlproject migrate` again on a new
   copy. `uv lock` needs the network to read PyPI.
3. If it fails again, run `uv lock` in `DIR` on a machine that has uv.

## `is stored from revision ..., but its source now reports revision ...`

```text
dengue-lags    FAIL       8s   ValueError: Model template 'dengue-lags' version '0.1.0' is stored from revision 'db139b6b09185e0be0971d31cafb39f126c910...
```

chap-core stores each version of a model template only once, with the git
revision of the image that first registered it. A new image under the same
version reports another revision, and chap-core refuses to run it. The model
still registers, so `chap-devops status` shows it as `registered`.

1. Raise the version of the model: `version` in `main.py`, or `version` in
   `MLproject`.
2. Commit, and build the image again with `GIT_REVISION`.
3. Run `chap-devops restart`.
4. Run `chap-devops models test ID --backtest`, or `chap-devops models configs sync ID`.
   This creates the configured models of the new version.

## `already exists and is not empty; give a new directory`

`chap-devops models new DIR` writes a project only into a directory that is not
there, or that is empty. It writes no file when the directory has files.
Give another directory, or remove the files in it first.

## `could not delete /v1/crud/datasets/N (HTTP 500 Internal Server Error)`

`chap-devops models test --backtest` deletes the dataset it made, and chap-core
refused, because a backtest still points at it. From chap-core v2.5.0, the
built-in baselines write backtests on the dataset, and chap-devops deletes them
first. If a baseline was still running a minute after the test, chap-devops could
not delete its backtest yet.

1. Run `chap-devops jobs`, and wait until no `run_baseline` job is running.
2. Run `chap-devops api GET /v1/jobs`, and note the `result` of each `run_baseline`
   job: it is the id of its backtest.
3. Run `chap-devops api DELETE '/v1/crud/backtests?ids=ID,ID,ID'` with these ids.
4. Run the `chap-devops api DELETE /v1/crud/datasets/N` command that the warning
   names.

## `chap-core has no configured model for`

```text
SERVICE    skip    0s   chap-core has no configured model for SERVICE VERSION
  it is registered, and chap-core has nothing to run it with; run `chap-devops models configs sync ID`, then `chap-devops models test ID --backtest`
```

The model is registered, but chap-core has no configured model for it. chap-core
2.4 and later makes no configured model from a registration, so a model without
one cannot run, and the Modeling App does not list it. `chap-devops status` shows such
a model as `registered, not configured`.

SERVICE is the service id, ID the model id, and VERSION the version that the model registered
with. A backtest uses only a configured model of that version that chap-core
has not archived. If the model registered with a new version, the configured
models of the old version do not count, because chap-core does not run them.
For a model registered from outside the deployment, `chap-devops status` then warns
`chap-core has configured models of it of other versions only`.

`chap-devops models test --backtest` makes the configured models itself before it
starts. So this skip comes only when that step failed, and the lines above the
table say why.

Make the configured models, then test again:

```sh
chap-devops models configs sync ID
chap-devops models test ID --backtest
```

See [Configured models](./models.md#configured-models).

If `chap-devops models configs sync` says that it could not create the configured
models and the reason is `answered HTTP 409 Conflict`, chap-core refused the
template. The model reports no git revision, or another revision than the
template chap-core stored for its version.

1. Read the reason in `chap-devops logs chap`.
2. If the reason says `is stored from revision None` or
   `its source reports no git revision`, the model image reports no git
   revision. See
   [`it reports no git revision`](#it-reports-no-git-revision-so-running-this-again-does-not-help).
3. If the model image changed, run `chap-devops update` to move the pin, or run
   `chap-devops restart --all <service>` to make the model register again.
4. Run `chap-devops models configs sync` again.

## `could not create its configured models`

```text
warning: chapkit_ewars_model: could not create its configured models: ... (after monthly_climate); the template version it stored is retired again, with the configured models made so far; run `chap-devops models configs sync chapkit_ewars_model` again, and `chap-devops logs chap` says why
```

chap-core refused a configured model of the marketplace entry, or stopped
answering. chap-devops retired the template version that it stored, so the
model has no configured model of this version that is half done.

1. Read `chap-devops logs chap` for the reason.
2. Run `chap-devops models configs sync ID` again. The run starts again from
   the template.

If the line says `retiring the template version it stored failed too`, the
configured models made so far are still live. Run the `chap-devops api DELETE`
command in the line, then run `chap-devops models configs sync ID`.

## `chap-core refused ID VERSION, so chap-devops started VERSION again`

```text
warning: chap-core refused chapkit_ewars_model 1.1.0, so chap-devops started 1.0.3 again; `chap-devops update` moves it again
```

`chap-devops update` moved the pin of the model, and `chap-devops up --wait`
started the new version. chap-core then refused to configure it, and
chap-devops retired the template version, as the line above it says. As
`chap-admin update` does, chap-devops put the pin back to the version before
the update and started that container again.

`chap-devops update` records the previous pin in `.chap-devops/models.yaml`
(`previous:`). The record goes when chap-core has configured the new version.
A model added with `chap-devops models add` gets no record.

1. Read `chap-devops logs chap` for the reason.
2. When the cause is fixed, run `chap-devops update` again.

## `chap-core has no configured model NAME for`

```text
chapkit-simple-multistep-model    skip       0s   chap-core has no configured model weekly for chapkit-simple-multistep-model 0.1.2; it has monthly_climate, monthly_selfhistory, short_lags
  run `chap-devops models configs list chapkit_simple_multistep_model` to see its configured models
```

`chap-devops models test --backtest --config weekly` asked for the configured model
`weekly`, and the model has no such configured model of the version it
registered with. The line names the variant names that it has.

1. Run `chap-devops models configs list ID` to see the configured models.
2. Run the test again with one of the names after `--config`.
3. To test a new configuration, add it with `chap-devops models configs add ID`.

With `--all`, `--config` applies to each model. A model without that
configured model is a skip, and the other models are tested.

## `chap-core did not list its configured models, so chap-devops cannot find --config`

```text
SERVICE    skip    0s   chap-core did not list its configured models, so chap-devops cannot find --config weekly
  run `chap-devops status` to see why
```

chap-devops asked chap-core for its configured models
(`GET /v1/crud/configured-models`), and chap-core did not answer with a list.
Without the list, chap-devops cannot find the configured model that `--config`
names.

1. Run `chap-devops status`.
2. If chap-core is not `up`, read `chap-devops logs chap`.
3. Add `-vv` to the test. The trace on stderr then shows what chap-core
   answered.

Without `--config`, the backtest does not skip. It sends the service id, and
the line under the row says `(by name, as chap-core could not list them)`.

## `it reports no git revision, so running this again does not help`

```text
warning: my_model: could not create its configured models: ...; it reports no git revision, so running this again does not help; build its image again with `--build-arg GIT_REVISION=$(git rev-parse HEAD)`, then run `chap-devops restart`
```

For a model registered from outside the deployment, the line ends
differently:

```text
warning: my-model: could not create its configured models: ...; it reports no git revision, so running this again does not help; set `GIT_REVISION` where it runs, then start it again
```

`chap-devops models configs sync` (and the same step in `chap-devops up --wait` and `chap-devops
models test --backtest`) asked chap-core for the model template, and chap-core
refused it. The model reports no git revision, and chap-core 2.4 stores no
template for such a model. In `--json`, the model has the state `no-revision`.
`chap-devops status` gives the same way out on the line under the table.

A model reports no git revision when its registration has `git_revision:
null`. On chap-core 2.4 and later, a registration with no `git_revision` key
counts too. chapkit 2.1 leaves out the key when it has no revision.

1. In the model's checkout, build the image again with
   `docker build --platform linux/amd64 --build-arg GIT_REVISION=$(git rev-parse HEAD) -t my-model:dev .`.
2. Run `chap-devops restart`.
3. Run `chap-devops models configs sync`.

For a model that chap-devops does not run, set `GIT_REVISION` where the model runs,
then start it again.

## `there is no terminal here; name the configuration with --name and its values with --set KEY=VALUE`

```text
error: there is no terminal here; name the configuration with --name and its values with --set KEY=VALUE, or run it at a terminal for the form
```

`chap-devops models configs add ID` without options opens a form, and the form
needs a terminal. In a pipe, in a script or with `--json`, there is no
terminal, so the command exits with status 2. `update` gives the same error
with `--set`, `--unset` and `--covariates` as the way out.

1. Give the values as options:
   `chap-devops models configs add ID --name NAME --set KEY=VALUE`.
2. Or run the command in a terminal to get the form.

## `has a configured model NAME already; give another --name, or archive it first`

```text
error: chapkit_simple_multistep_model has a configured model monthly_climate already; give another --name, or archive it first with `chap-devops models configs archive chapkit_simple_multistep_model monthly_climate`
```

A configured model with this variant name is live. If chap-devops sent the new one,
chap-core would show the new one and hide the old one, and give no message.
So chap-devops refuses the name and exits with status 2.

- To keep both, give another `--name`.
- To change the values of the existing one, use
  `chap-devops models configs update ID NAME`.
- To replace it, archive it first with the command in the message.

`add --from FILE` gives the same refusal, with "remove it from FILE" as the
other way out.

## `is not an option of ID; its options are`

```text
error: `lag` is not an option of chapkit_simple_multistep_model; its options are n_samples (integer), n_target_lags (integer), rf_max_depth (integer), rf_min_samples_leaf (integer)
error: `two` is not a value of `n_target_lags`, which takes an integer, such as `3`
```

chap-devops checks each `--set` against the options of the model template before it
asks chap-core. chap-core does not check the values of a chapkit model, and
chapkit accepts keys that it does not know. Without the check, a wrong key
would have no effect, and a wrong value would show only when a run fails.

1. Use a key from the list in the message.
2. Give a value of the kind in the message. Write a list with commas, such as
   `n_lags=2,2`.

## `takes only these additional covariates`

```text
error: my_model takes only these additional covariates: rainfall, mean_temperature; remove `foo`
error: my_model takes no additional covariates; remove --covariates
error: `population` is a required covariate of chapkit_ghr_model, which every run gets; remove it from --covariates
```

The model template does not allow free covariates, so a configured model can
use only the defaults of its marketplace entry. A required covariate is never
in `--covariates`, because every run gets it. Remove the covariate that the
message names.

## `an export is the configurations of one model; name one of`

```text
error: an export is the configurations of one model; name one of auto_arima_chapkit, chapkit_ghr_model, chapkit_rwanda_malaria_bym_model, chapkit_simple_multistep_model
```

The deployment runs more than one model, and `chap-devops models configs export`
got no ID. An export goes under one marketplace entry, so it holds one model.
Run the command again with one of the ids in the message.

## `the new values are live, but the old row N was not archived`

```text
warning: the new values are live, but the old row 104 was not archived: ...; archive it with `chap-devops api DELETE /v1/crud/configured-models/104`
```

`chap-devops models configs update` creates a configured model with the new values,
then archives the old one. The first step worked and the second did not. So
chap-core may show two configured models with this variant name. Run the
`chap-devops api DELETE` command in the message to archive the old row.

## ``unknown model `X`; run `chap-devops models list` to see the model ids``

```text
error: unknown model `nosuch`; run `chap-devops models list` to see the model ids
```

The marketplace has no model with that id, and no model added with `chap-devops
models add` has it. The command exits with status 1. Run `chap-devops models list`
and use an id from the `ID` column. Both the marketplace id
(`chapkit_simple_multistep_model`) and the service id
(`chapkit-simple-multistep-model`) work.

chap-devops 0.102.0 and older say this for `chapkit_ewars_model` on a plain
`chap-devops init`. The marketplace no longer lists CHAP-EWARS, and those versions
enabled it when `--models` was not given. Update chap-devops with `chap-devops self
update`. Or give the models by name, for example `chap-devops init mychap --models
chapkit_simple_multistep_model`.

## 401 from the Modeling App

```text
{"detail": "Missing or invalid API token"}
```

The token DHIS2's `chap` route is sending is not the one chap-core is
enforcing, or the route sends none. `chap-devops dhis2 connect` finds out by asking
through the route, and writes the token this deployment holds into it:

```sh
chap-devops dhis2 connect
```

`chap-devops dhis2 show` reports the same thing without changing anything:
``the `chap` route does not carry chap-core's API token``.

Two ways to get here. Either `chap-devops auth rotate` was run and the clients were
never updated, which is the second half of rotating; or `chap-devops auth enable` was
run and `chap-devops up` was not, so chap-core is still running without the token
while `.env` already has one. `chap-devops status` tells the two apart:

```text
chap-core   up, token rejected   http://localhost:8700   v2.3.1 (pinned)   auth: on
error: chap-core at http://localhost:8700 is up and did not accept the API token:
port 8700 answers /v2/services with HTTP 401: the API token in .env is not accepted
```

means the running chap-core has a different token - it is up, which is why no
container log is printed under it and no model table: the registry is behind
the same token, so whether a model registered cannot be read - and

```text
chap-core   up   http://localhost:8700   v2.3.1   auth: on
```

means `.env` and the running container agree, so the mismatch is in the client.
Either way `chap-devops up` is what hands a changed `.env` to the containers. See
[Authentication](./auth.md).

## The models stopped registering after I turned authentication on

```text
chapkit-simple-multistep-model  running, not registered  via chap-core  -
```

A protected chap-core rejects an unauthenticated registration like any other
request, so a model that came up without the shared secret never appears in
`GET /v2/services`. `chap-devops auth enable` writes `SERVICEKIT_REGISTRATION_KEY`
alongside the API token for exactly this reason, and re-renders every overlay
to pass it on, but the containers only read `.env` when Compose creates them:

```sh
chap-devops up
chap-devops status
```

If it persists, check that both ends carry the line:

```sh
grep SERVICEKIT_REGISTRATION_KEY compose.chapkit-simple-multistep-model.yml compose.chap-devops.yml
```

An active `SERVICEKIT_REGISTRATION_KEY: ${SERVICEKIT_REGISTRATION_KEY:-}` in
the overlay is what the model sends; a commented one means
`.chap-devops/project.yaml` does not know the project has a key, which
`chap-devops auth show` will report as a mismatch and `chap-devops auth enable` puts right.

The same line in `compose.chap-devops.yml` is what chap-core checks it against.
Upstream's `compose.ghcr.yml` passes only `CHAP_API_TOKEN` into the `chap`
service, so without that line the container has no key, and the model's
`X-Service-Key` is rejected as an invalid API token - a 401 in
`chap-devops logs chapkit-simple-multistep-model`. `chap-devops sync` then `chap-devops restart` puts the
line back.

## `unable to open database file`

```text
sqlite3.OperationalError: unable to open database file
```

Docker seeds a fresh named volume from whatever the image has at the mount
point, ownership included, so an image that never creates its data directory
yields a root-owned volume the unprivileged model cannot write to.

Every model overlay ships a one-shot `<service_id>-init` container that chowns
the volume to the model's numeric uid:gid before the model starts (`0:0` for a
root image), so this is fixed by construction. If you see it anyway:

- the overlay was hand-edited, or the init container was removed. Run
  `chap-devops sync` to render it again.
- `--user` names an account `chap-devops` does not know, so it fell back to
  `1000:1000`. `chap-devops sync` warns when it does. Pass the numbers instead:
  `chap-devops models enable ID --user 1001:1001`.
- the model writes somewhere other than its data directory, on the read-only
  root filesystem. `--data-dir` points the volume at the right path; see
  [Data directories and users](./models.md#data-directories-and-users).

A model that crash-loops right after starting is almost always one of the last
two.

## `Permission denied` from a model's own binaries

```text
sh: /usr/local/lib/R/site-library/INLA/bin/linux/64bit/inla.run: Permission denied
```

The model started, registered, and then could not execute a file it ships
itself. The overlay is running it as an account the image does not use.

Some model images end their Dockerfile on `USER root` and keep their binaries
root-owned and not world-executable - the Rwanda BYM model's INLA binaries were
mode 744 up to 0.1.1 - so an overlay that hardens such an image down to
`user: 1000:1000` takes away the one permission it needed. The container comes
up either way, which is why this surfaces as a failed prediction rather than a
failed start.

`chap-devops models enable <id>` reads the account off the image again and rewrites
the overlay; `chap-devops up` then restarts the service with it:

```console
$ chap-devops models enable chapkit_rwanda_malaria_bym_model
$ chap-devops up
```

The rendered overlay should then carry no `user:` line for a root image, and
its `<service_id>-init` container should chown the volume to `0:0`. For an
image that does drop to an account of its own - which Rwanda BYM 0.1.2 now
does, as `chapkit` - it carries that account's `uid:gid` instead, and the init
container chowns to the same two numbers.
`chap-devops models info <id>` shows what was recorded and where it came from, and
`chap-devops doctor` has one `user <service>` line per enabled model that compares
the two whenever the image's amd64 variant is pulled here.

`--user <uid>:<gid>` overrides the image, for the rare case where the image is
wrong about itself. See
[Data directories and users](./models.md#data-directories-and-users).

## `dependency failed to start: container ...-dhis2-db-1 is unhealthy` while the restore continues

The restore of a large seed took longer than the `start_period` of
`dhis2-db`. If `chap-devops logs dhis2-db` shows no error and the restore
continues, the database is not broken. Give the restore more time:

1. Set `DHIS2_DB_START_PERIOD=7200s` in `.env`, or a longer time.
2. Run `chap-devops components disable dhis2 --purge`.
3. Run `chap-devops components enable dhis2`.
4. Run `chap-devops up`.

A seed that is a file gets a time that grows with its size. A seed from a URL
gets 600 s. See [The time for the restore](./dhis2.md#the-time-for-the-restore).

## `dependency failed to start: container ... is unhealthy`

`chap-devops up` reads that line and then says why, out of the failing container's
own log:

```text
dependency failed to start: container demo-1ab2c3-chap-1 is unhealthy

why chap is unhealthy:
  sqlalchemy.exc.OperationalError: (psycopg2.OperationalError) connection to server at
  "postgres" (192.168.32.2), port 5432 failed: FATAL:  password authentication failed for
  user "chap"
  ERROR:    Application startup failed. Exiting.
  the database volume holds a different password than .env (a previous deployment with the
  same name, or --fresh-env); the `volumes` line of `chap-devops doctor` says which, and
  `chap-devops down --volumes` removes the volume if this deployment's data can go
```

The same lines close `chap-devops status` and `chap-devops doctor`'s `health` check, and
the chap-core line reads `down (container unhealthy)` rather than plain `down`:
the container is there, and it is the container that is wrong.

The two causes worth knowing by name are below. `chap-devops logs chap` has the rest.

## `password authentication failed for user "chap"`

The PostgreSQL volume holds a role password that is not the one in `.env`.
There are two ways to get there.

**A volume from another deployment of the same name.** The volume was made
by an earlier deployment with the same compose project name, and it still has
that deployment's role password. The new deployment's `.env` has a freshly
generated password. `chap-devops doctor` says so:

```text
warn  volumes   the database volume demo-1ab2c3_chap-db predates this deployment; if chap-core
                cannot log in, it belongs to an earlier deployment with the same name
      remove it with `chap-devops down --volumes` if this deployment's data can go, or keep both
      by giving one of them a name of its own
```

`chap-devops init` gives each deployment a name of its own (`demo-1ab2c3`), so this
happens only when two deployments record the same name; see
[the compose project name](./concepts.md#the-compose-project-name).

**`init --fresh-env`.** It rotates the PostgreSQL password on purpose, and the
existing volume was created with the old one.


Either way: drop the volume and start over,

```sh
chap-devops down --volumes
chap-devops up
```

or change the role to match the new password with `ALTER USER` inside the
running postgres container:

```sh
chap-devops docker exec postgres psql -U chap -d chap_core
```

This is why `init` never rewrites a `.env` it finds, `--force` included. See
the [`.env` contract](./concepts.md#the-env-contract).

## `no matching manifest for linux/arm64`

The marketplace images and chap-core are published for amd64 only. Every
overlay `chap-devops` writes pins `platform: linux/amd64`, so an arm64 host such as
Apple silicon pulls that variant and runs it under emulation instead of
failing.

Seeing this error means the pin is missing: a hand-edited overlay, or a compose
file not written by `chap-devops`. Run `chap-devops sync` to render the overlays again, and
check `chap-devops docker config` for the service that has no `platform`.

## Compose is older than 2.24.4

```text
warning: docker compose 2.18.1 is older than 2.24.4; compose.chap-devops.yml uses `!override`, which needs 2.24.4 or newer, and compose.marketplace.yml uses `include:`, which needs 2.20.0
```

`compose.chap-devops.yml` uses `!override`, which arrived in Compose 2.24.4, and
`compose.marketplace.yml` uses `include:`, which arrived in 2.20. `chap-devops` warns
rather than failing, because the base services still run.

What an older Compose loses, in order: below 2.24.4 the API port override is
merged into chap-core's own `ports:` instead of replacing it, so the API is
published on two host ports; below 2.20 the model overlays do not load at all.
Upgrade Docker Compose.

## `needs chap-core, and this deployment has no chap-core`

```text
error: `chap-devops jobs` needs chap-core, and this deployment has no chap-core; `chap-devops components enable chap-core` adds one, or `chap-devops components enable chap-core --url URL` names one elsewhere
```

The deployment was created without chap-core (`--without chap-core`), so there
is no chap-core to ask. `chap-devops jobs`, `chap-devops api` (without `--url`),
`chap-devops models test --backtest` and `chap-devops update --chap-tag` all say this
(`chap-devops models test` without `--backtest` does not need it). If you meant to
talk to a chap-core somewhere else, `chap-devops api --url <base>` sends the request
there. Otherwise add chap-core with `chap-devops components enable chap-core`, then
`chap-devops up`. See [Standalone OCS](./components.md#standalone-ocs).

## `registration.missing_orchestrator_url` in a model's log

```text
[error    ] registration.missing_orchestrator_url [servicekit.api.registration] env_var=SERVICEKIT_ORCHESTRATOR_URL
```

Expected in a deployment without chap-core, and harmless. The overlay leaves
`SERVICEKIT_ORCHESTRATOR_URL` out on purpose because there is nothing to
register with. servicekit logs this line at error level, then skips registration
and serves as usual. `chap-devops status` asks the model's own `/health`, which is the
check that matters there. In a deployment *with* chap-core the line means the
overlay is out of date: run `chap-devops sync`, then `chap-devops restart`.

## `running and not answering on /health`

```text
1 of 1 model is running and not answering on /health; a model that just started answers in a minute, so run `chap-devops status` again, or read `chap-devops logs chapkit-simple-multistep-model`
```

A deployment without chap-core asks each model's own `/health` on its host
port. A model image can take a minute to start, so ask again first. If it stays,
`chap-devops logs <service_id>` shows why the service did not come up. A model enabled
with `--port none` has no host port to ask and is judged by its container alone.

## `App never became ready, skipping registration`

```text
[error    ] registration.aborted [servicekit.api.service_builder] message='App never became ready, skipping registration' port=5001
```

Before servicekit registers a model, it checks that the app answers inside the
container. With a chap-core elsewhere, chap-devops reads the image's command when it
enables the model, and decides where the app listens:

- An image that starts with `--port 8000` listens on 8000, and its host port
  maps to 8000.
- An image whose command names no port gets `PORT` set to its host port.

So this message means that chap-devops could not read the command, for example
because the model was enabled `--offline`, or that the image does something
else. Enable the model again with a network (`chap-devops models enable <id>`), then
run `chap-devops up`. `chap-devops logs <service>` shows the port that servicekit checked.

## `not registered with ...` from `chap-devops run`

`chap-devops run --chap-core URL` started the model, and the chap-core at `URL` did
not list it within 30 seconds. The model's log says why: run the
`chap-devops -C ... logs` command that the message names.

- `registration.attempt_failed` with `Name or service not known` or
  `Connection refused`: the model cannot reach chap-core. Check that chap-core
  runs, and that it listens on `0.0.0.0` when it is a process on this machine.
- `App never became ready`: see the entry above.
- A 401: chap-core asks for a registration key, and the model has none. See
  [Authentication](./auth.md) for how a deployment passes the key to its
  models.

## `registered, unreachable` / `chap-core cannot reach it at`

`chap-devops status` asks chap-core's proxy for each registered model's `/health`
(`/v2/services/<id>/run/health`). A model is `registered, unreachable` when
that fails with a 5xx or no answer: the model registered, and chap-core cannot
call it back at the URL it registered under. Every backtest and prediction
through chap-core would fail the same way. The line under the table names the
URL and what came back, and the fix depends on where chap-core runs; see the
next entry.

## A model registers with an external chap-core, and its jobs fail to connect

The model shows as `registered` in `chap-devops status`, but a job fails with a
connection error to `localhost:<port>` or `host.docker.internal:<port>`. The
registration worked, and chap-core cannot reach the address the model
registered under. That address is `models_host` in `.chap-devops/components.yaml`
plus the model's host port:

- chap-core runs as a process on this machine: `localhost` is right.
- chap-core runs in a container: run
  `chap-devops components enable chap-core --url URL --models-host host.docker.internal`,
  then `chap-devops up`.
- chap-core runs on another machine: use the name or address it reaches this
  machine by, and make sure the model ports (5001 up) are open to it.

The same applies the other way round, to a model you run from its checkout
against a chap-core in chap-devops: `SERVICEKIT_HOST` must be `host.docker.internal`
and the model must listen on `0.0.0.0`. See
[Your model from its checkout, with Chap](./use-cases/model-on-host.md).

## `publishes no public image at`

`chap-devops models add https://github.com/OWNER/REPO` runs the image that
repository publishes on ghcr.io, and ghcr refused to hand out an anonymous pull
for it. It answers the same for a package that is private and for one that was
never published, so either way there is nothing chap-devops can pull from that URL.
Build the image from a checkout and add it by name:

```sh
docker build --platform linux/amd64 -t my-model:dev .
chap-devops models add my-model:dev
```

See [A model image you built yourself](./use-cases/local-model-image.md). If
the package exists and is private, making it public on GitHub (the package's
settings) is the other way.

## `is not in the local image store for linux/amd64`

```text
error: my-model:dev is not in the local image store for linux/amd64; build it with `docker build --platform linux/amd64 -t my-model:dev .` in the model's checkout, then add it again
```

`chap-devops models add` read `my-model:dev` as a local image, because it names no
registry, and the local store has no amd64 build under that name. Either it was
never built, it was built for another platform (a plain `docker build` on an
Apple Silicon Mac makes an arm64 image), or the tag differs. Run the command in
the message. If you meant a published image, give its full reference, such as
`ghcr.io/my-org/my-model:sha-1eb8cf1`.

## `is not a chap-core checkout`

```text
error: /Users/me/src/chap is not a chap-core checkout: it has no Dockerfile.worker; pass the directory you cloned github.com/dhis2-chap/chap-core into
```

`chap-devops init --source` builds the chap-core API from `Dockerfile`, the worker
from `Dockerfile.worker`, and renders `compose.yml` from `compose.ghcr.yml`, all
at the top of the checkout. Point it at the directory `git clone` created, not
a subdirectory. A very old chap-core may predate one of the three files; check
out a newer commit.

## `another chap-devops command is changing this deployment; waiting for it to finish`

Another chap-devops command on the same deployment holds the state lock (see
[One change at a time](./concepts.md#one-change-at-a-time)), and this one waits
until it exits. It is usually a `chap-devops up` rendering its files or an
`chap-devops update` checking the marketplace, and it ends on its own. If it does not,
find the command still running against this directory (`ps aux | grep chap-devops`)
and let it finish or stop it; the lock goes with the process.

## `did not start: the registry answered denied for IMAGE, so the image does not exist or is private`

```text
error: my-model did not start: the registry answered `denied` for ghcr.io/my-org/my-model:dev, so the image does not exist or is private; check the reference, or run `docker login ghcr.io`, then `chap-devops run ghcr.io/my-org/my-model:dev` tries again
```

`chap-devops run` asked compose to start the model, and the registry refused the
pull. A registry gives the same answer for an image that does not exist and for
a private image.

1. Check the reference. For a marketplace id, use `chap-devops models search`.
2. If the image is private, run `docker login <registry>` with an account that
   can read it.
3. Run the `chap-devops run` command that the message names.

## `did not start (...)` from `chap-devops run`

`chap-devops run` asked compose to start the model and compose refused; the
parentheses hold the line compose said it with. A `denied` from the registry
has its own message; see the entry above. `port is already allocated` is a
host port something else took between the pick and the start; run again. The
model was taken back out, so the `chap-devops run` the message names starts afresh.

## `leftovers: N volumes of M removed deployments still in docker`

`chap-devops doctor` found volumes of deployments whose directory is gone. Nothing
uses them and no chap-devops command would name them again, so they only take disk
space. `chap-devops cleanup --dry-run` lists them, and `chap-devops cleanup` deletes them
after asking. A deployment you moved rather than deleted counts as removed
until a chap-devops command runs in its new directory; run `chap-devops sync` there first.
See [Cleaning up after removed deployments](./doctor.md#cleaning-up-after-removed-deployments).

## `--json cannot ask before deleting data` from `chap-devops cleanup`

`chap-devops cleanup` deletes data and asks first, and a `--json` run (or one with
no terminal) has nobody to ask. `chap-devops cleanup --dry-run` lists what would go;
`chap-devops cleanup --yes` deletes it without asking.

## `` volume X is still there; remove it with `docker volume rm X` ``

`chap-devops down --volumes` removed the volumes that compose declares and the
leftover volumes that docker labels with this deployment's compose project.
A volume with the deployment prefix that has no such label, or that a
container still uses, stays. The warning names each one, with the reason in
parentheses when docker gave one. If you do not need its data, run the
`docker volume rm` command in the line.

If the reason is `volume is in use`, a container still holds the volume. A
backup or restore that was killed can leave its reader container behind, with
a name that starts with `chap-devops-volume-`. `docker ps -a --filter volume=X` shows
the container. Remove it with `docker rm -f NAME`, then run `chap-devops down
--volumes` again. See [Taking a backup](./backup.md#taking-a-backup).

## `docker would not remove volume ...` from `chap-devops stop --purge`

Purging a group removes every volume compose made for it, and docker refused
one, usually because a container still uses it. The group was kept, so nothing
is left behind that chap-devops cannot reach. `docker ps -a --filter volume=<volume>`
shows what holds it; stop that, run the `docker volume rm` the message names,
then the same `chap-devops stop --group <group> --purge` again.

## `resuming ..., left paused` from `chap-devops up`

A service of this deployment was paused, which is what a `chap-devops backup
create` that was killed while it held the service still leaves behind. `chap-devops
up` resumed it before starting anything else; nothing more to do. Before that,
`chap-devops status` shows the service as `paused`, with the warning `` dhis2 is
paused, so it does not answer; run `chap-devops up` to resume it ``.

## `there is no marketplace model ...`

`chap-devops run` was given a word with no tag, which is a marketplace id, and the
catalogue has no such id. `chap-devops models search <word>` finds the right one. A
local image needs its tag: `chap-devops run my-model:dev`.

## `not ready after 300s`

`chap-devops up --wait` gave up: the names after the colon are what never answered,
each with its `chap-devops status` state. `chap-core` means its `/health` never
answered; see [`this deployment's chap-core is not running`](#this-deployments-chap-core-is-not-running).
`running, not registered` is a model whose container is up and that never
registered; see [A model is running but not registered](#a-model-is-running-but-not-registered).
`running, not answering` is a model without chap-core whose `/health` never
answered on its host port: `chap-devops logs <service>` says why. A first start that
pulls large images can simply take longer than the default; `--timeout 900`
waits fifteen minutes.

## A hand edit disappeared

`chap-devops sync` re-renders the artifacts from `.chap-devops/`, so an edit to
`compose.yml`, `compose.chap-devops.yml`, `compose.marketplace.yml` or a
`compose.<service_id>.yml` is drift that the next `sync` (and therefore the next
`chap-devops up`) undoes.

- To change the base services, edit `.chap-devops/compose.chap-core.<tag>.yml`. `sync`
  follows it, and says that the recorded checksum no longer matches.
- To change a model, edit `.chap-devops/models.yaml` and run `chap-devops sync`.
- To add something of your own, write a `compose.custom.yml`. `sync` only ever
  removes overlays it wrote itself, so yours is left alone; add it to the `-f`
  list or to the umbrella by hand if you want it included.

`chap-devops sync --check` reports drift without writing, and exits non-zero, which
makes it a usable pre-commit or CI check.

## `scheduler.dataset_sync is no longer supported`

OCS does not start, and `chap-devops logs ocs` shows this line. The current OCS
keeps its schedules in its data volume, not in `ocs/climate-service.yaml`.

1. Write down the datasets and the times in the `dataset_sync:` list.
2. Remove the `dataset_sync:` list from `ocs/climate-service.yaml`. Keep
   `enabled: true`.
3. Run `chap-devops restart ocs`.
4. Make the schedules again in the Schedule tab of each dataset in the OCS web
   UI. OCS does not import the old list.

See [Schedules](components.md#schedules).

## An OCS dataset plugin does not appear after a restart

You put a plugin in `ocs/plugins/datasets/`, ran `chap-devops restart` - or
`chap-devops restart --all ocs` - and OCS still serves the datasets it served before.
Nothing failed: the plugin directory was never mounted.

The mount is rendered from the directory being there, and `chap-devops up` is the only
wrapper that re-renders the compose files before it calls Docker. On a deployment
where `ocs` was enabled before `ocs/plugins/` existed, `compose.ocs.yml` carries
no plugin mount, so `chap-devops restart` compares the container against a file it
already matches and does nothing at all.

```sh
chap-devops up                                          # syncs first, then recreates ocs
chap-devops docker exec ocs ls /app/plugins/datasets    # the plugin is inside
```

`chap-devops doctor` finds this on its own, on the `compose files` line, which is
`chap-devops sync --check`:

```text
warn  compose files   out of date with .chap-devops/ (2 to write, 5 unchanged, 0 to remove)
      run `chap-devops sync`, or `chap-devops up`, which syncs first
```

Two files, because the mount in `compose.ocs.yml` and the `plugins_dir` key in
`ocs/climate-service.yaml` are written by the same sync.

`chap-devops restart ocs` is the right command for a change to
`ocs/climate-service.yaml`, and for the opposite reason: that file is a bind
mount, so its new text is already inside the container and the compose files have
not changed - all that is wrong is that OCS read the old text at startup, and
`restart` recreates a service whose mounted config is newer than its container. A
directory that did not exist when the compose file was rendered is the other
case: the file itself is out of date, and recreating a container cannot mount
what the file does not mention. See
[Adding a plugin to a running deployment](./components.md#adding-a-plugin-to-a-running-deployment).

## A DHIS2 that answers but 404s every API request

The login page loads, `chap-devops status` may even say `dhis2  up`, and every
`/api/*` request answers **404**. Nothing in the log says "error".

DHIS2 migrates its schema **forward only**, so this is almost always an older
image started against a database a newer one has already migrated. The Spring
context fails to come up, Tomcat keeps running and keeps serving pages, and every
API route is simply not registered. The other two causes have the same shape: a
`dhis.conf` DHIS2 could not read, and a Flyway checksum that does not match the
database.

```sh
chap-devops logs dhis2 | grep -iE "flyway|migrat|exception|failed"
chap-devops docker config | grep "image: dhis2/"         # the tag that is running
grep -n "image_tag" .chap-devops/components.yaml          # the tag that was asked for
grep -n "DHIS2_IMAGE_TAG" .env                      # and any override, which wins
```

Put the tag back where the database is, rather than migrating further:

```sh
chap-devops backup create                        # before anything, if there is data worth keeping
chap-devops components enable dhis2 --tag 2.42   # the version of the database; or fix DHIS2_IMAGE_TAG in .env
chap-devops up
```

If the log says that the migration of the schema failed, the database is from
an older DHIS2. `chap-devops up` then says:

```text
DHIS2 could not migrate the database to its version; a database from an older DHIS2 must go up one version at a time, so run `chap-devops components enable dhis2 --tag VERSION` with the next version, then `chap-devops up`, and repeat
```

There is no way back down a migration. A database migrated by 2.42 does not work
under 2.41 again, so the choice is the newer image or a restore. `chap-devops` warns
before it happens - the warning names `chap-devops backup create` first for exactly this reason:

```text
warning: the DHIS2 image moves from 2.42 to 2.41 and `dhis2_db` is already there: DHIS2 migrates a schema forward only, so run `chap-devops backup create` first - an older image on a migrated database answers healthy while every API request 404s
```

A deployment with nothing in DHIS2 worth keeping is quicker to start again:
`chap-devops components disable dhis2 --purge` takes all three volumes, and the next
`chap-devops up` migrates or restores from scratch. See
[Changing the DHIS2 version](./dhis2.md#changing-the-dhis2-version).

## DHIS2 never becomes healthy

Two quite different things look the same from outside, and the first start is
where both show up.

**It is still migrating.** DHIS2 migrates its whole schema before it serves a
request, and on a seeded deployment it restores a dump first. That is about 40
seconds on a native arm64 host with an empty database and **8 to 15 minutes**
under emulation. The health check allows 240 seconds of `start_period` and 60
retries for that reason. Watch it rather than restarting it:

```sh
chap-devops logs -f dhis2
```

**It ran out of memory.** DHIS2 wants roughly **4 to 5 GB** for the analytics
populate phase, and below that the JVM is `SIGKILL`ed part-way through. That
failure looks like anything except memory: a container that exits with no Java
exception in the log, an analytics run that never ends, an instance that was
healthy a minute ago and is now gone.

```sh
docker inspect --format '{{.State.OOMKilled}} {{.State.ExitCode}}' \
  "$(chap-devops docker ps -- -q dhis2)"
docker info --format '{{.MemTotal}}'      # what the daemon has to give
```

`OOMKilled true`, or exit code 137, settles it. Raise the memory the Docker
daemon has - on Docker Desktop that is the VM's limit, in Settings, Resources -
rather than trimming the heap: a smaller `-Xmx` makes the JVM fail inside Java
instead of being killed, which is tidier and no more successful. The heap itself
is `DHIS2_JAVA_TOOL_OPTIONS` in `.env`, commented with the default
`-Xms2g -Xmx4g -XX:+UseG1GC`. See [Memory](./dhis2.md#memory).

## The Modeling App does not see Chap

Because nothing has connected the two yet, and one command does all of it:

```sh
chap-devops dhis2 show        # which of the three pieces is missing
chap-devops dhis2 connect     # the route, the apps, then analytics
```

The app does not reach chap-core directly. It goes through a DHIS2 Route with
`code: "chap"`, and until that row points at this deployment the app redirects to
`/get-started`.

The trap is that it will **look** configured. The climate demo dumps ship a
`chap` route of their own - right code, right authority, not disabled - aimed at
an external Chap server, so a seeded instance has a route by that name resolving
to somebody else's chap-core. `chap-devops dhis2 route` repoints it rather than skipping
it:

```text
repointed the `chap` route at http://chap:8000/**; chap-core answered through it
```

With `-v`, a hint says where it pointed:
`hint: the route was rewritten because it pointed at http://158.39.75.126/stable/**`.

`chap-devops dhis2 show` is the one to run first: it names each piece that is missing
and changes nothing. If the app shows Chap but a model is missing from its
model picker, run `chap-devops status`. If the model is `registered, not configured`,
run `chap-devops models configs sync`. If the route is right and the app still shows no figures,
analytics has not been generated - `chap-devops dhis2 analytics`. If there is no Chap
entry in DHIS2's apps menu at all, the app is not installed -
`chap-devops dhis2 apps`. See
[Connecting the Modeling App to Chap](./dhis2.md#connecting-the-modeling-app-to-chap).

Do not read a **silent** `chap-devops up` or `chap-devops status` as an answer here. Those
two name `chap-devops dhis2 connect` only until one has been recorded, and the record
is a note that the command ran rather than a check of the route - see
[`connected_at`, and what it is not](./dhis2.md#connected_at-and-what-it-is-not).
`chap-devops dhis2 show` is the command that asks DHIS2.

## `Oops! Sorry, an unexpected error` or `Unnamed evaluation` in the Modeling App

The browser still holds a login to a DHIS2 that is gone. After
`chap-devops down --volumes` (or `chap-devops components disable dhis2 --purge`) and a
fresh DHIS2 on the same port, the old session cookie means nothing to the new
instance: every request the Modeling App makes is redirected to the login
page, and the app shows either this error or a dashboard of
`Unnamed evaluation` rows with `0 locations`, sometimes after first showing
the evaluations of the deployment that was removed.

Nothing is wrong with the deployment, and `chap-devops jobs` shows what Chap really
holds. Open `http://localhost:8780/` (or the address `chap-devops open dhis2
--no-browser` prints), log in again with `admin` / `district`, and open the
Modeling App from the app menu.

## `chap-devops has not connected this DHIS2 to Chap`

`chap-devops up` closes with this while `.chap-devops/components.yaml` records no `chap-devops
dhis2 connect` for this deployment. `chap-devops status` closes with it too, but only
while its `dhis2` row says `up`. For an external DHIS2, `status` has no `dhis2`
row, so only `chap-devops up` says it:

```text
chap-devops has not connected this DHIS2 to Chap; run `chap-devops dhis2 connect`
```

Do what it says, once DHIS2 answers. The command is idempotent, so running it on
a deployment that is already connected repoints nothing, reinstalls nothing and
says so - which is also why the line is safe to print without asking DHIS2
anything.

It keeps coming back in three cases, all of them deliberate:

- **the component was disabled and enabled again.** The record is about a DHIS2
  instance, so it is forgotten with the component. A re-enabled `dhis2_db` can be
  restored from the seed dump, and [that dump ships a `chap` route of its
  own](#the-modeling-app-does-not-see-chap) pointing at an external server.
- **`chap-devops down --volumes` removed `dhis2_db`.** Same hazard, said on the spot:
  the record of `chap-devops dhis2 connect` went with `dhis2_db`.
- **the last `connect` found something wrong.** A route nothing answered on, or
  an app that would not install on the `dhis2` component, clears the record, and with `-v` the report says
  `cleared` - `chap-devops dhis2 show` names which of the two it was. A run that could
  not look is not one of these: `chap-devops dhis2 connect --offline` skips the apps,
  judges nothing and leaves the record exactly as it found it.

Connecting the instance by hand, in DHIS2's own Route administration and App
Management, does not record anything either, and the line then stays. `chap-devops
dhis2 connect` against that instance is the way to settle it: it will find
nothing to change and record that it ran.

## The route is there but nothing answers through it

`chap-devops dhis2 route` wrote the row and then could not prove the path:

```text
warning: the `chap` route is in place but nothing answered through it: HTTP 502 Bad Gateway; run `chap-devops status` to see whether chap-core is up
```

If no answer came at all, the line says so:

```text
warning: the `chap` route is in place but nothing answered through it: the request through DHIS2 timed out; run `chap-devops status` to see whether chap-core is up
```

DHIS2 answered the route write a moment before, so the request that got no
answer is about chap-core behind the route, not about DHIS2. A new or
repointed route gets a second request after two seconds before chap-devops gives
this warning.

The route is correct, so this is not a route problem. Three things it is, in the
order worth checking:

1. **chap-core is not running.** `chap-devops status` settles it, `chap-devops up` fixes it,
   and `chap-devops dhis2 route` then verifies on the next run.
2. **`route.remote_servers_allowed` does not list the target.** DHIS2 42 and later
   default that setting to `https://*` and refuse an `http://` target. The
   scaffolded `dhis2/dhis.conf` already permits every http and https target, so
   this is an instance whose file was narrowed or replaced; DHIS2 refuses the write outright
   in that case and the error names the line. After editing it,
   `chap-devops restart dhis2` applies it.
3. **Something else answers on that hostname.** A 200 that is not chap-core's
   health document is reported as such rather than as success.

The `` run `chap-devops status` `` clause is there only when chap-core does not answer on
this machine either. When chap-core answers, the line names the more probable
cause in its place. `chap-devops dhis2 show` adds the same clause after `` missing:
nothing answered through the `chap` route: ... ``.

### `chap-core answers at ..., so the DHIS2 allowlist may refuse`

```text
warning: the `chap` route is in place but nothing answered through it: HTTP 503 Service Unavailable; chap-core answers at http://localhost:8700, so the DHIS2 allowlist may refuse http://chap:8000; add it to `route.remote_servers_allowed` in `dhis2/dhis.conf` and run `chap-devops restart dhis2`
```

chap-core is up, and DHIS2 answered 503 for the route. The usual cause is
`route.remote_servers_allowed`, which does not list the origin of the route.

1. Add the origin that the line names to `route.remote_servers_allowed` in
   `dhis2/dhis.conf`.
2. Run `chap-devops restart dhis2`.
3. Run `chap-devops dhis2 connect` again.

On an external DHIS2, the line names `` the `dhis.conf` of the DHIS2 server ``.
That file belongs to the admin of the DHIS2, so ask that admin to add the
origin.

### `chap-core answers at ..., so DHIS2 may not reach it at`

```text
warning: the `chap` route is in place but nothing answered through it: ...; chap-core answers at http://localhost:8700, so DHIS2 may not reach it at http://localhost:8700; give the address DHIS2 reaches chap-core at with `chap-devops dhis2 use --chap-url URL`
```

chap-core is up on this machine, but the external DHIS2 cannot reach the
chap-url. A `localhost` address is the DHIS2 server itself, not this machine.

1. Run `chap-devops dhis2 use --chap-url URL` with an address that the DHIS2 server
   reaches this machine at.
2. Run `chap-devops dhis2 connect` again.

## `Allowed route URL must not have a path`

DHIS2 does not start after `route.remote_servers_allowed` was edited, and its
log ends with

```text
Caused by: java.lang.IllegalStateException: Allowed route URL must not have a path: http://localhost:8700/**
```

Each entry on that line is an origin: scheme, host and port, nothing after
them. The route target `chap-devops dhis2 route` writes ends in `/**`, and that is
not what goes on the line. Write the origin alone and restart DHIS2:

```text
route.remote_servers_allowed = https://*,http://localhost:8700
```

`chap-devops dhis2 connect` then writes the route. When DHIS2 refuses a route, the
error line names the origin to add.

## An edit to `dhis2/dhis.conf` changes nothing after a restart

You corrected a value in `dhis2/dhis.conf` - `route.remote_servers_allowed`,
a `connection.*` line, one of the commented `server.https` proxy lines - ran
`chap-devops restart dhis2`, and it said:

```text
nothing needed a restart: every container matches its files
```

`dhis.conf` is a **bind mount**, which compose does not compare, so `chap-devops
restart` looks at the file itself and recreates `dhis2` when the file was
written after its container was created (`recreating dhis2 to apply its edited
config file`). This line means the file's modification time is older than the
container: a copy or a restore that kept the original time. Recreate it
regardless:

```sh
chap-devops restart --all dhis2
```

DHIS2 migrates before it serves a request again, so it is not back when compose
returns; the next `chap-devops dhis2` command waits for it. Then re-run whatever
reported the problem:

```sh
chap-devops dhis2 route
```

`ocs/climate-service.yaml` is handled the same way - see
[Read-only instances](./components.md#read-only-instances) and
[`dhis2/dhis.conf`](./dhis2.md#applying-an-edit). It is not the same as
[An OCS dataset plugin does not appear after a restart](#an-ocs-dataset-plugin-does-not-appear-after-a-restart),
where the compose file itself is out of date: a restart cannot fix that one and
`chap-devops up` can.

## `the App Hub publishes no version of ... that DHIS2 ... can run`

```text
failed Modeling App was not installed: the App Hub publishes no version of Modeling that DHIS2 2.42.6 can run; install it from DHIS2's own App Management page
```

**Upgrade `chap-devops`.** Up to and including 0.4.0 this sentence was wrong for every
instance there is. The App Hub sends a version bound it has not set as an empty
string rather than as an absent field, and `""` was read as a maximum: every
published version looked capped below the instance, so every one was filtered
out. `chap-devops` now treats a missing, empty, whitespace or unreadable bound as no
bound, on both sides.

If it appears on a current `chap-devops`, it is what it says: check the app's own page
on [apps.dhis2.org](https://apps.dhis2.org) for a version that lists this
instance's release under `minDhisVersion`. `chap-devops dhis2 apps -v` prints the
version it resolved and the id it installed.

`POST /api/appHub/{versionId}` is idempotent, so installing from DHIS2's own App
Management page in the meantime costs nothing: `chap-devops dhis2 apps` afterwards
reports the app as already installed rather than fighting it.

## `analytics may never have run on this deployment`

```text
analytics  2026-06-16 09:51 (unconfirmed on a seeded database)
missing: analytics may never have run on this deployment: a seeded database can carry the dump's timestamp, and no run has finished since DHIS2 started; run `chap-devops dhis2 analytics` to settle it
```

This is not an error, and it is not `chap-devops` saying the tables are missing. It is
`chap-devops` declining to say they are there.

`lastAnalyticsTableSuccess` is a row of DHIS2's own settings, so a
[seeded](./dhis2.md#the-seed) deployment restores it with the rest of the dump.
Measured on a 2.42.6 with the Laos climate demo, a freshly seeded instance
reported a last success of `2026-06-16T07:51:00.093` while `analytics_2024` did
not exist at all - the table was absent, not empty, and the Modeling App saw
nothing. The timestamp was a fact about the database the dump was taken from.

```sh
chap-devops dhis2 analytics
```

settles it, and the row afterwards reads `(a run finished on this deployment)`.
Restarting DHIS2 empties the notifier `chap-devops` reads that from, so the line can
come back on a deployment where analytics really has been generated; running it
again is idempotent and takes tens of seconds on demo data. See
[What `show` can say about analytics](./dhis2.md#what-show-can-say-about-analytics-and-what-it-cannot).

## The analytics tables are empty but the run reported success

Almost certainly `lastYears`. Measured on the climate demo,
`POST /api/resourceTables/analytics?lastYears=8` reported success in 16.9 seconds
and wrote **zero rows into every analytics table**; the same request without it
wrote 146,129 rows into `analytics_2024` in 15.7 seconds.

`chap-devops dhis2 analytics` never sends it, so a run made with

```sh
chap-devops dhis2 analytics
```

populates what the database holds. A reference deployment's own
`trigger-analytics.sh` does send `&lastYears=8`, which is where this comes from.

If the tables are still empty afterwards, the database has no data in the period
the apps are asking about: the [Climate App](./dhis2.md#the-apps-come-from-the-app-hub-server-side)
is what imports climate data into DHIS2, and analytics has to run again after an
import.

## `chap-devops dhis2` says DHIS2 did not accept the password

```text
error: DHIS2 at http://localhost:8780 did not accept the password for `admin` (the DHIS2 default password); set `DHIS2_ADMIN_USERNAME` and `DHIS2_ADMIN_PASSWORD` in `.env`, or export `CHAP_DEVOPS_DHIS2_PASSWORD`
```

`chap-devops` holds no DHIS2 credentials of its own. On a DHIS2 it deployed it falls
back to `admin` / `district`, which is what a seeded demo dump and a
Flyway-bootstrapped empty database both give. So this is an instance whose
password has been changed, or one restored from a dump of your own. Either
uncomment and edit the lines in `.env`:

```ini
DHIS2_ADMIN_USERNAME=admin
DHIS2_ADMIN_PASSWORD=the-one-that-works
```

or keep it out of the file entirely:

```sh
export CHAP_DEVOPS_DHIS2_PASSWORD=the-one-that-works
```

or use a personal access token instead (`DHIS2_API_TOKEN` in `.env`, or
`CHAP_DEVOPS_DHIS2_TOKEN`). There is no `--password` flag, because a password on a
command line is in the shell history and in `ps`. A **403** rather than a 401
means the opposite problem: the credential is right, but that user is not
allowed to write a route or run analytics, which a DHIS2 superuser is. See
[The credentials](./dhis2.md#the-credentials-and-where-they-come-from).

## `chap-devops dhis2` says DHIS2 did not accept the API token

```text
error: DHIS2 at https://dhis2.example.org did not accept the API token (API token from `.env`): it is expired, revoked, or not allowed from this address; set a current one as `DHIS2_API_TOKEN` in `.env`, or export `CHAP_DEVOPS_DHIS2_TOKEN`
```

DHIS2 answers 401 for a token that has expired or been revoked, and for one
whose allowed IP addresses or referrers leave out the machine chap-devops runs on.
Create a new one under **Profile > Personal access tokens** in DHIS2. The 401
also covers a value that was never a token, such as one pasted with the
`ApiToken ` prefix: `DHIS2_API_TOKEN` holds the token alone.

## `chap-devops has no password for DHIS2 user`

```text
error: chap-devops has no password for DHIS2 user `alice`: `.env` and `CHAP_DEVOPS_DHIS2_USERNAME` give theirs to other users; export `CHAP_DEVOPS_DHIS2_PASSWORD` for this run
error: chap-devops has no password for DHIS2 user `ops`: `.env` names the user and sets no `DHIS2_ADMIN_PASSWORD`; set it there, or export `CHAP_DEVOPS_DHIS2_PASSWORD`
```

A password belongs to the user it was set with, and chap-devops will not send one
user's password in another's name. `--user alice` finds only a password that is
alice's, and the default `district` is `admin`'s alone. For a one-off run as
someone else, export theirs for the command:

```sh
CHAP_DEVOPS_DHIS2_PASSWORD=theirs chap-devops dhis2 show --user alice
```

## `chap-devops has no credentials for this DHIS2, and did not deploy it`

```text
error: chap-devops has no credentials for this DHIS2, and did not deploy it, so there is no default to try; set `DHIS2_ADMIN_USERNAME` and `DHIS2_ADMIN_PASSWORD`, or `DHIS2_API_TOKEN`, in `.env`
```

The DHIS2 was recorded with `chap-devops dhis2 use`, so `admin` / `district` is not
known to be anybody's password there, and chap-devops does not try it. Set a personal
access token (or a username and password) as in
[The credentials](./dhis2.md#the-credentials-and-where-they-come-from), then run
`chap-devops dhis2 use` again to check that DHIS2 accepts it.

## `this deployment already uses the external DHIS2`

```text
error: this deployment already uses the external DHIS2 at https://dhis2.example.org; run `chap-devops dhis2 use --clear` first to deploy one of its own
```

A deployment has one DHIS2, either the `dhis2` component or an external one, so
`chap-devops dhis2` never has to guess which. The opposite direction is refused the
same way (`the dhis2 component is on, and it is this deployment's DHIS2`). Clear
the one you no longer want, then add the other. See
[A DHIS2 that runs elsewhere](./dhis2.md#a-dhis2-that-runs-elsewhere).

## `chap-devops dhis2` waited twenty minutes and gave up

```text
error: DHIS2 at http://localhost:8780 did not answer /api/ping within 20 minutes; `chap-devops logs dhis2` is where the migration shows, and `--wait SECONDS` waits longer
```

Either DHIS2 is still migrating - under emulation a first start is a quarter of an
hour, and a seeded one is that plus the restore - or it has stopped. `chap-devops logs
dhis2` tells the two apart, and
[DHIS2 never becomes healthy](#dhis2-never-becomes-healthy) is the rest of the
list. `--wait 3600` waits an hour.

## `chap-devops update` said a restart is needed

That is the whole design, not a failure. `chap-devops update` moves the pins and
pulls the images and stops there: it never decides for you when a deployment
goes down. The closing line names the running services that are now behind
what the files say, and

```sh
chap-devops restart
```

recreates exactly those, leaving the rest running. If Chap is not running, the
line names no service, and `chap-devops up` starts it with the new versions (with
`-v`, a hint says so). See
[Updating](./updating.md).

## `chap-devops update` fails offline

```text
error: `chap-devops update` refreshes the marketplace registry, which needs the network; drop --offline, or run `chap-devops update --list-tags` to see the chap-core tags offline
```

By design. Inside a deployment, `chap-devops update` fetches the registry from the
network with no cache and no fallback, `--dry-run` included, because a plan
made from a stale catalogue is not a plan. Outside a deployment, `--dry-run`
shows the cached registry instead. Every other command falls back to the cache
and then to the snapshot compiled into the binary.

## `the stable version VERSION of ID is STATUS, not verified`

```text
error: the stable version 0.1.4 of `chapkit_simple_multistep_model` is unstable, not verified; pass `--version 0.1.4` to enable it anyway
```

The `stable` channel of the marketplace entry points at a version that the
marketplace has not verified. chap-devops follows `chap-admin install` and
takes only a verified version on that channel.

- To wait for a verified version, run `chap-devops update` again later.
- To run this version now, enable it with the `--version` in the message.
  The model is then pinned, and `chap-devops update` does not move it.

## `version VERSION of ID has the image tag TAG, which is not sha- and the start of its commit`

```text
error: version 1.0.2 of `chapkit_ewars_model` has the image tag latest, which is not `sha-` and the start of its commit, so the image can change after the review; pick another version with `--version`
```

The marketplace entry pins the version to a tag that can move to other code.
chap-devops follows `chap-admin` and runs a marketplace version only from a
`sha-` tag of at least 7 characters that start its `commit`. A model added with
`chap-devops models add` is not checked.

- To run the model, enable another version with `--version`.
- Report the entry to the marketplace.

## `left out models/FILE of the marketplace registry: its id is ID and its service_id is SERVICE`

The `id` of a marketplace entry must be the name of its file, and its
`service_id` must be the `id` with `-` in place of `_`. chap-devops follows
`chap-admin` and leaves out an entry that does not match. The other models
stay. Report the entry to the marketplace.

## `it registered git revision REVISION, which is not the commit of its image tag`

`chap-devops status` compares the `git_revision` that a model of this
deployment registered with the commit of its `sha-` image tag, as `chap-admin`
does after an install. If they differ, the running image may not be the
pinned one.

1. Run `chap-devops up`. It starts the pinned image.
2. If the line stays, the image of that tag was built from another commit.
   Report it to the marketplace.

## `chap-core already has a model registered as SERVICE at URL, and it is not a model of this deployment`

`chap-devops models add` asks chap-core for its registered models before it
adds one. A model that registers under the same id would take the place of
the other one. When chap-core does not answer, the model is added without
this check.

- If the other model must go, stop it, then run `chap-devops models add`
  again.
- If both must run, pass another `--service-id`. The service must register
  under that id.

## `the deployment in DIR still has containers in compose project`

```text
error: the deployment in /srv/chap still has containers in compose project chap; stop it with `docker compose -p chap down`, which keeps its volumes, then run this again
```

`chap-devops init --adopt` takes over the database of the old deployment. If
both deployments ran, they would use one database at the same time.

1. Run the `docker compose -p <project> down` command in the message. It
   removes the containers and keeps the volumes.
2. Run `chap-devops init --adopt` again.

## `found no volume PROJECT_chap-db, so this would start chap-core with an empty database`

```text
error: found no volume chap_chap-db, so this would start chap-core with an empty database; if the deployment in /srv/chap ran under another compose project name, set COMPOSE_PROJECT_NAME in its .env to that name, then run this again
```

`chap-devops init --adopt` gives the new deployment the compose project name of
the old one: the directory name, as compose makes it, or `COMPOSE_PROJECT_NAME`
in its `.env`. Docker has no database volume under that name.

1. Run `docker volume ls --filter name=chap-db` to find the database volume.
2. The part before `_chap-db` is the project name. Set `COMPOSE_PROJECT_NAME`
   to it in the `.env` of the old directory.
3. Run `chap-devops init --adopt` again.

## `could not find the template of ID in chap-core, so nothing changed`

```text
error: could not find the template of chapkit_simple_multistep_model in chap-core, so nothing changed: chap-core at http://localhost:8000 is not responding: io: Connection refused (os error 61); run `chap-devops status`; start chap-core with `chap-devops up`, or run it without --retire
```

`chap-devops models disable --retire` and `chap-devops models remove` look up the
model template in chap-core before they change anything. chap-core did not
answer, so the model is still as it was.

- If the deployment is down, run `chap-devops up`, then run the command again.
- If you do not want to change chap-core, run `models disable` without
  `--retire`, or `models remove` with `--keep-in-chap`. The model then stays
  live in the Modeling App.

## `could not retire ID in chap-core`

```text
warning: could not retire chapkit_simple_multistep_model in chap-core: ...; run `chap-devops api DELETE '/v1/crud/model-templates/30?allVersions=true'`
```

The model left the deployment, but chap-core refused the retire or stopped
answering after the lookup. The template is still live in the Modeling App.
Run the `chap-devops api` command in the warning when chap-core answers again.

## `left out models/ID.yaml of the marketplace registry`

```text
warning: left out models/chapkit_minimalist_example_py.yaml of the marketplace registry: kind: unknown variant `notebook`, expected `model` or `starter` at line 6 column 7; this chap-devops may be older than the registry, so run `chap-devops self update`
```

chap-devops could not read one model file of the marketplace registry. The
usual cause is a field or a value that the marketplace added after this
chap-devops was built. chap-devops leaves that model out and uses the others,
so the other commands still work.

- Run `chap-devops self update`. A newer chap-devops usually reads the file.
- If the warning stays after the update, the file is not valid. Report it
  against the marketplace repository.

## `` `chap-devops registry update` refreshes the marketplace registry, which needs the network; drop --offline ``

```text
error: `chap-devops registry update` refreshes the marketplace registry, which needs the network; drop --offline
```

`chap-devops registry update` only fetches the registry, so `--offline` leaves it
nothing to do. It is a usage error, and the exit code is 2. Run it again
without `--offline`.

## `ignoring the registry cache in DIR: meta.json is corrupt`

```text
warning: ignoring the registry cache in /home/me/.cache/chap-devops/registries/...: meta.json is corrupt: ...; run `chap-devops registry update` to write it again
```

A file of the cached registry cannot be read, so chap-devops does not use the cache
and the command goes on without it. The same warning names `registry.yaml`
when that file is the broken one. Run `chap-devops registry update` to write the
cache again.

## `GitHub's rate limit is used up`

```text
skip  registry pin chapkit_simple_multistep_model  could not ask https://github.com/chap-models/chapkit_simple_multistep_model
      for its default branch: GitHub's rate limit is used up (unauthenticated, 60 requests an hour
      per address); set GITHUB_TOKEN for 5000, or wait until 15:04
```

GitHub allows 60 REST requests an hour to an address that sends no
credential, and `chap-devops` spends them on questions it has to ask somebody:
chap-core's release list, the default branch and commits of every model
repository behind a `registry pin <id>` line, and both lookups of a
`chap-devops models add`. One `chap-devops doctor` on a deployment with five models is a
dozen of them. On a shared egress address - an office, a VPN, a CI runner -
the hour is gone before `chap-devops` asks for anything at all.

Nothing is broken and nothing failed: every check that needs GitHub skips with
this reason and the rest of the report is exactly as it was. Either wait until
the clock time on the line, which is when the window resets, or hand `chap-devops` a
token:

```sh
export GITHUB_TOKEN=$(gh auth token)   # or GH_TOKEN, whichever is already set
chap-devops doctor
```

That is 5000 requests an hour instead of 60. A token with no scopes is enough,
because every question `chap-devops` asks GitHub is a public read. `chap-devops doctor`'s
`github api` line says which limit is in force and how much of it is left:

```text
ok    github api  reachable, 4990 of 5000 requests left this hour (token)
```

The token is read from the environment only. `chap-devops` never stores it and never
prints it - a `-vv` trace shows `Authorization: Bearer <token>` with nothing
behind it - and no command-line flag sets one, because a token on a command
line is a token in the shell history.

A 403 that arrives *with* a token reads differently, because the advice to set
one would be wrong:

```text
GitHub refused the request (HTTP 403 with a token; check GITHUB_TOKEN's scopes or the rate limit)
```

That is an expired, revoked or misspelled token, one whose fine-grained
permissions do not include reading public repositories, or a token whose own
5000 are gone. `gh auth status` says which.

## `Can't locate revision identified by` in `chap-devops logs chap`

```text
ERROR [chap_core.database.database] Error during Alembic migrations:
Can't locate revision identified by 'b4c5d6e7f8a3'
```

The database was migrated by a newer chap-core than the one now running: the
Alembic revision it was stamped with does not exist in this build. It is what
`chap-devops update --chap-tag` warns about before a move backwards, and the reason
it asks for an answer:

```text
warning: moving chap-core from master to v2.3.1 can run an older schema against a
database migrated by the newer one; run `chap-devops backup create` first
```

chap-core logs the error and starts anyway, so `chap-devops status` says `up` while
the schema is not the one this build expects. Either go back to the newer tag

```sh
chap-devops update --chap-tag master   # or whichever it was
chap-devops restart
```

or restore the backup taken before the move (`chap-devops backup restore`) and start
again from there. See
[Switching chap-core's tag](./updating.md#switching-chap-cores-tag).

## `ghcr.io has no image`

```text
error: ghcr.io has no image chap-worker:dev, so the pull would fail; nothing was changed; pick another tag, for example a release from `chap-devops update --list-tags`
```

chap-core is two images, `chap-core` and `chap-worker`, and a tag that exists
for one of them does not have to exist for the other: at the time of writing
ghcr serves `chap-core:dev` and has no `chap-worker:dev` at all. `chap-devops update
--chap-tag` asks ghcr for both images before it moves the pin, in the dry run
too, so nothing was changed. Run `chap-devops update --list-tags`, and pick a tag
from that list. See
[Switching chap-core's tag](./updating.md#switching-chap-cores-tag).

## `the pull failed after the pins moved`

```text
error: the pull failed after the pins moved; chap-core is now pinned to v2.4.0, and
`chap-devops update --chap-tag v2.3.1 --yes` puts it back
```

The pin moved, and then the pull failed for another reason, such as a network
error or a full disk. The deployment now describes images that Docker could not
fetch; the containers keep running what they had. The lines above the error
give the reason. Fix it and run `chap-devops docker pull`, or use the command in the
line to put the pin back.

## `gives ... the compose file ..., which is not a file in this deployment's directory`

Every command reads `.chap-devops/` first, and this one stopped there:

```text
error: `.chap-devops/models.yaml` gives chapkit_simple_multistep_model the compose file `/tmp/compose.x.yml`, which is not a file in this deployment's directory; chap-devops only writes `compose.<service>.yml` there; correct the entry by hand, or run `chap-devops init --force` to rebuild the state
```

chap-devops writes every compose file into the deployment directory itself, as
`compose.yml` or `compose.<service>.yml`, and writes and removes them by that
name. A name with a `/`, a `..` or a drive in it would have it write or delete
a file somewhere else, so it is refused before anything uses it. The state was
edited by hand or came from an archive changed after it was made. Put the
plain name back (`compose.chapkit-simple-multistep-model.yml` for that model), or rebuild
the state with `chap-devops init --force`. The same message names
`.chap-devops/project.yaml` when the name is in its `compose_files` or
`rendered_files` list.

## `lists the project file ..., which is not inside a deployment directory`

`chap-devops backup restore` refused the archive before restoring anything: its
manifest names a file outside the project directory, such as `../.bashrc`.
`chap-devops backup create` never writes such a path, so the archive was changed
after it was made. Restore from another one.

## ``runs in docker (`docker_env` in its MLproject), and the container has no docker socket``

`chap-devops chap` read the model's `MLproject`, and the model starts a container of
its own. In the container that `chap-devops chap` runs, there is no docker socket
unless you give it. Run the same command again with `--docker` after `chap`:
`chap-devops chap --docker eval ...`. The flag gives the container full control of
docker on this machine. See [Models that run in docker](./chap-cli.md#models-that-run-in-docker).

## `Cannot reach the Docker daemon. The model ... requires Docker to run`

chap-core says this when a `docker_env` model runs without a docker socket. In
`chap-devops chap`, the cause is not a stopped Docker: the container did not get
the socket. chap-devops did not find `docker_env` before the run, because it could
not read the model's `MLproject` (for example, under `--offline`). Add
`--docker`.

## `needs chap-core v2.5.0 or later, and this run uses`

```text
error: built-in models need chap-core v2.5.0 or later, and this run uses v2.4.0; give a newer tag, such as `--tag v2.5.0`
```

Built-in models (`builtin:NAME`), `--run-config.as-chapkit` and
`chap simulate` are new in chap-core v2.5.0. Outside a deployment,
`chap-devops chap` uses the newest release, unless this machine has only older
images and cannot look the release up; in a deployment, the tag of the
deployment. chap-devops stops before the run, because chap at an older tag ends in
a traceback. Add `--tag v2.5.0`. In a deployment, `chap-devops update` moves it to
the newest release.

## `Rscript: not found` from `chap-devops chap`

The model needs R, and it ran in the `chap-core` image, which has no R. chap-devops
selects the worker image for a GitHub repository or a model directory, so
this happens with `--image core`, or with a chapkit URL whose service calls R
itself. Run it again with `--image worker`.

## `FileNotFoundError: [Errno 2] No such file or directory` from `chap-devops chap`

chap writes an output file into a directory that does not exist. chap does
not make directories, with or without chap-devops. Make the directory first
(`mkdir -p out`), then run the command again. A path outside the current
directory is mounted from its nearest existing parent, so it has the same
rule.

## `--offline needs a ghcr.io/dhis2-chap/chap-core image on this machine`

Outside a deployment, `chap-devops chap` asks GitHub for the newest chap-core
release. Under `--offline` it uses the newest image of that kind on this
machine instead, and there is none. Run the command once without `--offline`,
or give a tag that is on this machine with `--tag`.

## `chap exited with status N; its own message is above`

chap stopped with an error. The cause is chap's own output above this line.
chap-devops exits with the same status, so a script can read it. If the run wrote
files before it stopped, chap-devops names them.

## `the model server at ... is not running (no answer on /api/v1/info), so chap was not started`

`chap-devops chap` asked the `--model-name` URL for `/api/v1/info` before the run,
and nothing answered. chap was not started, so there is no traceback.

- A service name such as `http://my-model:8000` that chap-devops does
  not know: give the model id instead (`--model-name chapkit_simple_multistep_model`).
  chap-devops then starts the model. In a deployment, the model must be enabled
  first.
- A server elsewhere: check that it runs, and that this machine can reach
  it.

## `` `ID` is not a model of the deployment at ...; `chap-devops models enable ID` adds it ``

In a deployment, `chap-devops chap` starts only the models that the deployment has.
It does not add a model for an evaluation, because that changes the
deployment. Run `chap-devops models enable ID`, then the same command again. Or run
`chap-devops chap` outside the deployment, where it starts the model in a
`chap-devops run` group.

## `... did not answer on http://...:8000 within 300s`

chap-devops started the model for the run, and the model did not answer in time. A
first start pulls the model's image, and an R model can take some minutes.
Run the command again with a longer `--timeout`. The message names the
`chap-devops logs` command that shows why the model does not start.

## `` `http://localhost:...` is this machine, and in the container `localhost` is the container itself ``

`chap-devops chap` got a model URL on `localhost` (or `127.0.0.1`). That is the URL
`chap-devops ps` shows, and it works from your machine. chap runs in a container,
where `localhost` is the container, so the URL reaches nothing. When chap-devops
knows which model has that port, the message gives its service URL, for
example `http://auto-arima-chapkit:8000`. Else it gives the same port at
`host.docker.internal`, the name of this machine in the container. That is
the URL for a service that `chap-devops chapkit mlproject run` started outside a
deployment.

## `stopped by Ctrl-C; ... is taken back out of ...`

You pressed Ctrl-C while `chap-devops run` started the model or waited for it to
answer. chap-devops took back out what the start put in: the model that this run
enabled is out of its group again, as after a failed start, and its data stays.
The command exits with 130. Run the same `chap-devops run` again to start it. A model
that ran before the command is left running, and the message says so.

