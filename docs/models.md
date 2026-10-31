# Models and the marketplace

## Where the catalogue comes from

The catalogue is the YAML in the model-marketplace repository, read from

```text
https://raw.githubusercontent.com/dhis2-chap/model-marketplace/main/registry.yaml
```

`--registry-url` points somewhere else, for a fork or a mirror. `chap-devops init
--registry-url URL` records it as `registry_url` in `.chap-devops/project.yaml`, and
every later command in that deployment uses the recorded one; typing the flag
again overrides it for that one run. The
marketplace has no JSON API, so the index is one request and each model file it
lists is one more; the requests are sequential, which is fast enough for a
handful of small files and makes a failure easy to attribute.

Resolution order for every command that needs the catalogue:

1. a cached copy younger than 24 hours,
2. the network,
3. a stale cached copy, when the network fails,
4. the snapshot compiled into the binary.

`chap-devops registry show` prints which of those was used:

```text
  url     https://raw.githubusercontent.com/dhis2-chap/model-marketplace/main/registry.yaml
  source  cache (54 minutes old)
  models  6
  cache   /home/me/.cache/chap-devops/registries/https_raw_githubusercontent_com_dhis2_chap_model_marketplace_mai-f6e5270ab795f1bd

  chapkit_rwanda_malaria_bym_model
  chapkit_simple_multistep_model
  auto_arima_chapkit
  chapkit_ghr_model
  chapkit_minimalist_example_py
  chapkit_minimalist_example_r

6 models in the catalogue (cache (54 minutes old))
```

With `-v`, a hint names the next command:

```text
hint: `chap-devops models info ID` describes one
```

`chap-devops registry update` forces a fetch and refreshes the cache, as does
`chap-devops update`. Both say what changed against the cached copy: the new
and removed models, and each changed model with its stable version before and
after. If nothing changed, the line says so. `--offline` never touches the network, so a laptop on a plane
still resolves the catalogue from the cache or the embedded snapshot;
`chap-devops update` and `chap-devops registry update` refuse to run that way, because a
refresh needs the network.

The cache lives in `$CHAP_DEVOPS_CACHE_DIR`, else `$XDG_CACHE_HOME/chap-devops`, else
`~/.cache/chap-devops`; `--cache-dir` overrides it for one invocation. A private
marketplace and the public one never share a cache entry, because the entry is
keyed by the registry URL. Every cache read failure is a warning, never an
error: a missing, truncated or hand-edited cache just means the fetch or the
embedded snapshot answers instead.

The embedded snapshot is the last resort, so `chap-devops` works on a machine that
has never had network access. It is refreshed with `make vendor`.

## Channels and versions

Each marketplace entry lists versions, and the channels `stable` and `latest`
each point at one of them.

```sh
chap-devops models enable chapkit_simple_multistep_model                    # stable, the default
chap-devops models enable chapkit_simple_multistep_model --channel latest
chap-devops models enable chapkit_simple_multistep_model --version 0.1.1    # an exact pin
```

A model that follows a channel records both the channel and the version it
resolved to, and `chap-devops update` re-resolves the channel. A model enabled with
`--version` is listed as pinned by `update` and never moved. `--channel` and
`--version` cannot be combined.

The `stable` channel takes only a version with `status: verified`, as
`chap-admin install` does. If the stable version of an entry is `unstable` or
`deprecated`, enabling it on the channel is refused, and the message names the
`--version` that takes it anyway. A yanked version is refused in every form.

Every version pins one commit of the model's own repository, and the image
built from it; whether that commit is still the newest build the repository
has published is what [`chap-devops doctor`](./doctor.md#project-checks) reports as
one `registry pin <id>` line per enabled model, because a pin that lags is for
the marketplace to move and not something a deployment can fix.

## Starters

Entries with `kind: starter` are scaffolding for writing your own model, not
forecasting models. They are hidden from `chap-devops models list` and from the
browser by default, `--starters` and `--all` show them, and enabling one needs
`--allow-starter`.

The marketplace calls them starters, not templates, because a model template
is something else in chap-core: what a model service registers as. The
`--template` of `chap-devops models new` is another meaning again: the type of
a new model project, as in [A new model project](#a-new-model-project).

## Looking at the catalogue

```sh
chap-devops models list                 # models enabled in this deployment are marked
chap-devops models list --enabled       # only this deployment's
chap-devops models list --all           # starters too
chap-devops models search malaria
chap-devops models info chapkit_simple_multistep_model
```

```text
ID                                SERVICE                           NAME                STATUS        STABLE  LATEST  PORT
chapkit_simple_multistep_model    chapkit-simple-multistep-model    Simple Multistep    limited data  0.1.2   0.1.2   via chap-core
auto_arima_chapkit                auto-arima-chapkit                Auto-ARIMA          experimental  1.0.2   1.0.2   -
chapkit_ghr_model                 chapkit-ghr-model                 GHRmodel            experimental  0.1.3   0.1.3   -
chapkit_rwanda_malaria_bym_model  chapkit-rwanda-malaria-bym-model  Rwanda Malaria BYM  not for use   0.1.3   0.1.3   -

4 listed, 1 enabled in this deployment
```

Every listing is in the same order: by `STATUS`, from the models that have
been validated down to the ones nobody should run, and by name inside each
step of that scale. Starters come after models when `--all` or `--starters`
asks for them, and a search narrows the catalogue without reordering it.

The `PORT` column is the host port a model publishes, `via chap-core` for one
this project enables without a host port of its own - chap-core's proxy is
then the only way in - or `-` for one this project does not enable.

## What the STATUS column means

`STATUS` is the model author's own
[chapkit](https://github.com/dhis2-chap/chapkit) assessment of the model, not
a marketplace verdict and not a measure of whether it runs. It is the one
column to read before enabling anything:

| Status | Colour | What the author is saying |
| --- | --- | --- |
| `production` | green | Validated and ready for production use. |
| `testing` | yellow | Ready for more rigorous testing on diverse data. |
| `limited data` | orange | Shows promise on limited data; needs manual configuration and careful evaluation. |
| `experimental` | red | A highly experimental prototype, not validated, only for early experimentation. |
| `not for use` | gray | Not intended for use: deprecated, or kept for backwards compatibility. |

`--json` keeps emitting the colour as `assessed_status`, so a script that
matches on `orange` goes on working, and adds the words above as
`assessed_status_label`. `chap-devops models info ID` prints the colour and the
whole sentence:

```text
  status        orange, shows promise on limited data, needs manual
                configuration and careful evaluation
```

`chap-devops models info ID` prints everything the marketplace says about one entry:
its image and runtime, the period types and forecast horizon it supports, the
covariates it requires and defaults to, the channels and the version table,
the maintainers and the attribution, and any named configurations. For a model
this project has enabled with no host port, it also prints the chap-core proxy
URL that reaches it.

Both `ID` forms work everywhere an id is taken: the marketplace id
(`chapkit_simple_multistep_model`, with underscores) and the service id
(`chapkit-simple-multistep-model`, with hyphens, which is also the Compose service name
and the DNS name).

## Enabling and disabling

```sh
chap-devops models enable ID [ID..] [--channel stable|latest | --version X]
                       [--port N|auto] [--bind ADDR] [--data-dir PATH]
                       [--user USER:GROUP] [--allow-starter]
chap-devops models disable ID [--purge] [--retire]
chap-devops models expose ID [--port N|auto] [--bind ADDR]
chap-devops models unexpose ID
chap-devops up                            # apply any of them
```

`models enable` takes several ids, separated by spaces or commas, as
`chap-devops init --models` does. `--channel`, `--port auto`, `--bind` and
`--allow-starter` then apply to each model. `--version`, `--port N`,
`--data-dir` and `--user` are for one model, and `enable` refuses them with
more than one id.

`--bind ADDR` sets the host address of the model port; see
[Which address a model port is published on](./ports.md#which-address-a-model-port-is-published-on).

`enable` and `disable` edit `.chap-devops/models.yaml` and then re-render the compose
files; `expose` and `unexpose` only rewrite the overlay's `ports:`, so they
never re-resolve the version and are safe on a deployment running a build you
do not want moved. The browser is the same: moving a port there keeps the
version the model is pinned to, and only a toggle or a new channel resolves the
marketplace again.

`disable` also stops and removes the model's container when one is running,
before the definition goes away, and says so:

```text
disabled chapkit_simple_multistep_model
stopped and removed 2 containers (chapkit-simple-multistep-model, chapkit-simple-multistep-model-init); host port 5001 is free again
kept volume mychap-1ab2c3_ck_chapkit_simple_multistep_model_data; remove it with `chap-devops models disable chapkit_simple_multistep_model --purge` or `docker volume rm mychap-1ab2c3_ck_chapkit_simple_multistep_model_data`
```

The port is therefore free straight away, rather than at the next `chap-devops up`,
which would have removed the container as an orphan but only once it was run.
There is nothing left for `chap-devops up` to apply. With `-v`, two hints follow:
the overlay that was removed (`removed compose.chapkit-simple-multistep-model.yml`) and
`` `chap-devops status` shows what runs now ``.

### Retiring a model in chap-core

A plain `disable` does not change chap-core. The model template and its
configured models stay live, so the Modeling App still lists the model. This
keeps a disable easy to undo: `enable` and `up` bring the model back as it was.

`disable --retire` also retires the model in chap-core, which is what
`chap-admin uninstall` does. chap-core then archives every version of the
template and every configured model of it. The rows stay for the backtests and
predictions that use them.

```text
disabled chapkit_simple_multistep_model
retired chapkit_simple_multistep_model in chap-core; its configured models are archived
```

If you enable the model again, chap-devops creates the configurations of the
marketplace entry again. A configured model that you made with
`chap-devops models configs add` stays archived. Use `--retire` when the model
leaves the deployment for good.

`--retire` needs chap-core to answer. chap-devops looks up the template before
it changes anything. If chap-core does not answer, the command stops and the
model stays enabled.

### The data volume

What `disable` does not take is the model's data: the named volume
`<compose project>_ck_<id>_data` stays exactly as it was, so enabling the model
again finds everything it had. That is why the line above names it - when
docker has it: a model that was never started has no volume, and then there
is no line, because nothing was kept. Nothing
else does any more - the overlay that declared the volume has just been
removed, so `chap-devops down --volumes` no longer knows about it, and a volume
nobody names again is kept for as long as the machine lasts.

`--purge` is how the data goes with the model:

```sh
chap-devops models disable chapkit_simple_multistep_model --purge
```

```text
disabled chapkit_simple_multistep_model
stopped and removed 2 containers (chapkit-simple-multistep-model, chapkit-simple-multistep-model-init)
removed volume mychap-1ab2c3_ck_chapkit_simple_multistep_model_data
```

The volume is removed after the container, which is the only order docker
allows: a volume a container still has mounted cannot be removed. Everything
about it is best-effort and reported rather than fatal - `volume <name> not
found` for a deployment that was never started, and `volume <name> could not
be removed: <docker's own words>` for anything else. `--json` carries the same
answer as two lists, `purged` and `kept_volumes`.

`--purge` also works on a model that is not enabled any more, which is the
state the kept-volume line leaves you in:

```text
chapkit_simple_multistep_model is not enabled here, so only its data volume was looked for
removed volume mychap-1ab2c3_ck_chapkit_simple_multistep_model_data
```

`chap-devops doctor`'s `volumes` line finds the ones that were forgotten: a volume
under this deployment's name that belongs to no enabled model and no enabled
component is reported as a leftover, with the same two ways to remove it. See
[Doctor](./doctor.md).

`expose` and `unexpose` say what changed:

```text
exposed chapkit-simple-multistep-model on http://localhost:5001
run `chap-devops up` to apply
```

```text
unexposed chapkit-simple-multistep-model; it stays registered with chap-core and reachable at http://localhost:8700/v2/services/chapkit-simple-multistep-model/run/
run `chap-devops up` to apply
```

If the model is already where the command puts it, nothing changes, and there
is no `chap-devops up` line:

```text
chapkit-simple-multistep-model is already exposed on http://localhost:5001; nothing changed
```

The port that the running container of the model publishes is not a port in
use for this check. So `--port 5001` or `--port auto` on the port of the model
itself keeps the port, and does not fail or move the model. If the record has
the port and the running container does not publish it yet, a second line
names the step:

```text
chapkit-simple-multistep-model is already exposed on http://localhost:5001; nothing changed
its running container does not publish this port yet; run `chap-devops up` to apply it
```

```text
chapkit-simple-multistep-model was already unexposed; nothing changed, and it is reachable at http://localhost:8700/v2/services/chapkit-simple-multistep-model/run/
```

An `enable` of a model that is already enabled at the same version and port
also changes nothing:

```text
chapkit_simple_multistep_model v0.1.2 is already enabled at http://localhost:8700/v2/services/chapkit-simple-multistep-model/run/; nothing changed
```

With `--json`, such a model is in the `unchanged` list, not in `enabled`.

## Configured models

chap-core does not run a model service directly. It runs a **configured
model**: a model template together with a set of option values and
covariates. A backtest, a prediction and the model picker of the Modeling App
all name a configured model.

Each configured model has a name, the variant name. The Modeling App shows it
in brackets after the model name: `<model name> [<variant name>]`. chap-core
stores it after the template: `<service id>:<variant name>`, and the bare
service id for the variant name `default`.

`chap-devops models configs` is the command group for the configured models:

| Command | What it does |
| --- | --- |
| `chap-devops models configs [list] [ID]` | Lists the configured models that chap-core has. |
| `chap-devops models configs add ID` | Adds a configured model. |
| `chap-devops models configs update ID NAME` | Gives a configured model new values. |
| `chap-devops models configs archive ID NAME` | Archives a configured model. |
| `chap-devops models configs export [ID]` | Writes the configured models in the marketplace format. |
| `chap-devops models configs sync [ID..]` | Creates the configured models of the marketplace entry. |

In each command, `ID` is the marketplace id or the service id of an enabled
model, and `NAME` is the variant name. Each command asks chap-core, so
chap-core must run. If chap-core does not answer, the command stops and names
`chap-devops status`.

### Listing

```sh
chap-devops models configs                                      # every enabled model
chap-devops models configs list chapkit_simple_multistep_model  # one model
chap-devops models configs --all                                # with the archived ones
```

```text
chapkit_simple_multistep_model
NAME                 COVARIATES                                        OPTIONS                                           SOURCE
monthly_climate      rainfall,mean_temperature,mean_relative_humidity  n_samples=100 n_target_lags=6 rf_max_depth=10...  marketplace
monthly_selfhistory  -                                                 n_samples=100 n_target_lags=6 rf_max_depth=10...  marketplace
short_lags           rainfall                                          n_target_lags=3 rf_max_depth=5                    by hand

3 configured models of 1 model
```

Each model gets a table. NAME is the variant name. OPTIONS shows the option
values that chap-core stores; an option without a value uses the model
default. A long list of values ends in `...`; `--json` has all of them. SOURCE says where the configured model comes from:

- `marketplace`: a configuration of the marketplace entry.
- `service default`: the configuration `default` of a model without a
  marketplace entry.
- `by hand`: a configured model that you added.
- `models test`: a configured model that a test left.

`--all` adds the archived configured models and the column ARCHIVED. `--json`
gives every field, with the option values whole. If chap-core has no
configured model of a model, the line names the way out:

```text
auto_arima_chapkit: chap-core has no configured model of it; `chap-devops models configs sync auto_arima_chapkit` creates them
```

### Adding

```sh
chap-devops models configs add chapkit_simple_multistep_model --name short_lags \
  --set n_target_lags=3 --set rf_max_depth=5 --covariates rainfall
```

```text
created configured model short_lags of chapkit_simple_multistep_model
```

- `--name` is the variant name.
- `--set KEY=VALUE` gives the value of one option. Repeat it for more than
  one. Write a list with commas, or as a JSON list. The booleans are `true`
  and `false`. `null` is a value only where the option allows it.
- `--covariates` gives the additional covariates, separated by commas.

chap-devops checks each key against the options of the template, and each value
against the kind of the option, before it asks chap-core. chap-core does not
check the values of a chapkit model, and chapkit accepts keys that it does
not know. So a mistake would otherwise show only when a run fails. A bad key
or value stops the command with exit code 2:

```text
error: `lag` is not an option of chapkit_simple_multistep_model; its options are n_samples (integer), n_target_lags (integer), rf_max_depth (integer), rf_min_samples_leaf (integer)
error: `two` is not a value of `n_target_lags`, which takes an integer, such as `3`
```

chap-devops also checks the covariates. A required covariate is refused, because
every run gets it. A model that does not allow free covariates takes only the
defaults of its marketplace entry.

chap-devops refuses a variant name that a configured model has and that is not
archived. chap-core would show the new configured model and hide the old one,
and give no message.

With no options at a terminal, `add` opens the form of `chap-devops ui`: one field
for the variant name, one field for each option with its kind, default and
description, and one field for the covariates. An empty field uses the
default. Esc closes the form and writes nothing:

```text
left the form; nothing was changed
```

Without a terminal (in a pipe or with `--json`), `add` with no options stops
with exit code 2 and names `--name` and `--set`.

`--from FILE` adds every configuration in a file of the marketplace format:
an [export](#exporting), or a complete marketplace entry. chap-devops checks all of
them before it creates the first. chap-core sets `prediction_periods` for
each run, so chap-devops does not store the value in the file.

### Changing

```sh
chap-devops models configs update chapkit_simple_multistep_model short_lags --set rf_max_depth=8
chap-devops models configs update chapkit_simple_multistep_model short_lags --unset rf_max_depth
chap-devops models configs update chapkit_simple_multistep_model short_lags   # the form
```

```text
updated configured model short_lags of chapkit_simple_multistep_model; chap-core keeps the old values as an archived configured model
```

chap-core has no request that changes a configured model. chap-devops creates a
configured model with the same variant name and the new values, then archives
the old one. chap-core keeps the old row for the backtests and predictions
that use it, but it does not list it again, so `list --all` does not show the
old values.

- `--set` gives a new value. The other values stay.
- `--unset KEY` removes a value, so the model default applies.
- `--covariates` replaces the old list.

With no options at a terminal, `update` opens the form with the current
values. If the new values are the same as the current values, chap-devops writes
nothing:

```text
configured model short_lags of chapkit_simple_multistep_model has these values already; nothing changed
```

If the archive step fails, the new values are live, and a warning gives the
`chap-devops api DELETE` command that archives the old row.

### Archiving

```sh
chap-devops models configs archive chapkit_simple_multistep_model short_lags
```

```text
archived configured model short_lags of chapkit_simple_multistep_model
```

chap-core does not delete a configured model. It archives it: chap-core
keeps the row for the backtests and predictions that use it, `list --all`
shows it, and the Modeling App shows it as Archived. If you add the same
variant name with the same values again, chap-core makes the archived row
live again.

You can also archive a configuration of the marketplace entry. `chap-devops models
configs sync` creates the marketplace configurations of a model only when
chap-core has no live configured model of the version the model runs. While
another one is live, `sync` does not change the model. When you archive the
last one, chap-devops says so:

```text
chapkit_simple_multistep_model has no other configured model now, so `chap-devops models configs sync` and `chap-devops up --wait` create its configured models again
```

### Exporting

```sh
chap-devops models configs export chapkit_simple_multistep_model > configurations.yaml
chap-devops models configs export chapkit_simple_multistep_model --out configurations.yaml
```

The output is the `configurations:` block of a marketplace entry. For each
configured model it has the variant name, a `description` and a `config`.
`config` is the flat object that chapkit takes: the option values,
`additional_continuous_covariates` and `prediction_periods`.

This is the configured model `short_lags` of the export, after the
`update` above:

```yaml
configurations:
  short_lags:
    description: The configured model short_lags of chapkit_simple_multistep_model.
    config:
      additional_continuous_covariates:
      - rainfall
      n_target_lags: 3
      prediction_periods: 12
      rf_max_depth: 8
```

chap-core does not store `prediction_periods`. chap-devops takes it from the
marketplace configuration with the same name. If there is none, chap-devops takes
the default from the config schema of the service, and else the chapkit
default of 3. `-v` says which source each value comes from. The keys of
`config` are in alphabetical order.

An export is for one model, because the block goes under one marketplace
entry. If the deployment runs more than one model, give the ID. With `--out`,
chap-devops reports the file:

```text
wrote 3 configurations of chapkit_simple_multistep_model to configurations.yaml
```

To use the configurations in another deployment, run `chap-devops models configs
add ID --from configurations.yaml` there. To publish them, put the block in
the marketplace entry of the model.

### Sync

Up to chap-core 2.3, a registration made a configured model. chap-core 2.4
and later makes none. A model that chap-devops starts registers, and without a
configured model nothing can run it.

`chap-devops models configs sync` does what `chap-admin install` does after a model
service has registered:

1. It stores the model template from the service
   (`POST /v1/crud/model-templates/from-service`).
2. It creates one configured model for each entry under `configurations` in
   the marketplace entry of the model. The option values come from the
   `config` of that entry. The covariates come from the config, or from
   `covariates.defaults` when the config has none. `prediction_periods` is
   removed, because chap-core sets it for each run.
3. A marketplace entry without configurations gets one configured model,
   `default`. A model added with `chap-devops models add`, or a model that runs an
   image other than its marketplace entry, also gets one `default` with the
   defaults of the service.

```sh
chap-devops models configs sync                                 # every model this deployment enables
chap-devops models configs sync chapkit_simple_multistep_model  # one model, by marketplace id or service id
```

```text
auto_arima_chapkit: created configured model monthly
chapkit_simple_multistep_model: chap-core has a configured model of it already
```

The command is idempotent for each registered version. A model that has a
configured model of the version it registered with is left alone, so a second
run adds nothing. An older chap-core that still makes configured models itself
gets no second set. A model that chap-core has not registered yet gets a line
that says so. Directly after `chap-devops up`, expect this line for a few seconds:

```text
chapkit_simple_multistep_model: not registered with chap-core; run `chap-devops models configs sync` again once `chap-devops status` shows it registered
```

A model that could not be configured gets a warning with the reason, and the
command exits non-zero.

If a configured model fails after the template was stored, chap-devops undoes
the step, as `chap-admin install` undoes a failed registration. It retires the
template version that it stored, and with it the configured models that the
step made. Only that version is retired. If an earlier version can run,
chap-core makes it live again, so a failed update leaves the model as it was.
The next run starts again from the template. Without the undo, one configured
model of three counts as configured, and no later run adds the other two.

```text
warning: chapkit_ewars_model: could not create its configured models: ... (after monthly_climate); the template version it stored is retired again, with the configured models made so far; run `chap-devops models configs sync chapkit_ewars_model` again, and `chap-devops logs chap` says why
```

In `--json`, the model has the state `failed` and an `undo` object: `retired`,
or `not-retired` with the error.

You do not usually run the command yourself. `chap-devops up --wait`, `chap-devops dhis2
connect` and `chap-devops models test --backtest` do the same step, and say which
configured models they created:

```text
created configured models in chap-core for chapkit_simple_multistep_model (monthly_climate, monthly_selfhistory)
```

`chap-devops status` shows a model without a configured model as `registered, not
configured`, and adds a line with the way out:

```text
auto-arima-chapkit: chap-core has no configured model for it, so nothing can run it; run `chap-devops models configs sync`
```

### Testing a configuration

`chap-devops models test --backtest` runs a backtest of one configured model of each
model. To test a configured model that you added, give its variant name with
`--config`:

```sh
chap-devops models configs add chapkit_simple_multistep_model --name short_lags \
  --set n_target_lags=3 --set rf_max_depth=5 --covariates rainfall
chap-devops models test chapkit_simple_multistep_model --backtest --config short_lags
```

```text
testing 1 model (through chap-core: a dataset, a backtest and its scores)
chapkit-simple-multistep-model    pass      13s   crps 6.4  mae 8.7  rmse 10.3
  configured model: short_lags (id 17)

1 of 1 model passes
```

Without `--config`, the backtest takes the configured model `default`, or else
the lowest-numbered one. See [The backtest level](#the-backtest-level). The
line under the row always names the configured model that it used.

`--config` counts only a configured model of the version that the model
registered with, and not an archived one. If the model has no configured model
of that name, the test skips it and names the ones that it has:

```text
chapkit-simple-multistep-model    skip       0s   chap-core has no configured model weekly for chapkit-simple-multistep-model 0.1.2; it has monthly_climate, monthly_selfhistory, short_lags
  run `chap-devops models configs list chapkit_simple_multistep_model` to see its configured models
```

With `--all` or more than one id, `--config` applies to each model. A model
that has a configured model of that name is tested with it. Each other model
is a skip with the names that it has. `--config` without `--backtest` stops
with exit code 2.

## Testing a model

`chap-devops status` and `chap-devops doctor` can both be entirely green while a model
cannot produce a single prediction. Registration is a heartbeat: it says the
service is alive and talking to chap-core, not that its runtime works, that the
account it runs as can write, or that the covariates it declares are the ones
it reads. `chap-devops models test` is the check neither of them can make, because it
is the only one that makes the model do the work.

```sh
chap-devops models test ID..                 # one or more, by marketplace id or service id
chap-devops models test --all                # every model this deployment enables
chap-devops models test --all --backtest     # the whole way round, through chap-core
chap-devops models test SERVICE_ID --backtest  # a model registered from outside, such as one run from its checkout
```

The model level needs no chap-core, so it also tests a model in a deployment
without one; `--backtest` needs one.

Models are tested one after another. A model that could not be tested at all
is a skip, and a skip is counted rather than treated as a bad answer. The exit
code is non-zero when one of the models **failed**, or when every model was
skipped: a run that tested no model proved nothing, and it says so in a
warning.

### The model level

The default. For each model, `chap-devops` runs chapkit's own end-to-end test inside
that model's container:

```text
docker compose exec -T <service> chapkit test --url http://127.0.0.1:8000 --timeout 300
```

chapkit reads the service's own configuration schema, creates a config from it,
generates data matching the covariates, period type and geometry the service
declares, and then validates, trains and predicts against it.

```text
testing 4 models (model level; add --backtest to go through chap-core)
auto-arima-chapkit                  pass   12s   1 training, 1 prediction
chapkit-ghr-model                   pass   24s   1 training, 1 prediction
chapkit-rwanda-malaria-bym-model    pass   20s   1 training, 1 prediction
chapkit-simple-multistep-model      pass    5s   1 training, 1 prediction

4 of 4 models pass
```

**What it proves:** the image runs, the service answers, the account the
container runs as can write to its data directory and its workspace, the
runtime has the libraries the model needs, and the model can turn a config and
a frame into a prediction. **What it does not touch:** chap-core. This is
therefore the level to reach for when chap-core is the thing that is broken,
and the level that answers in seconds rather than minutes.

A failure names the phase and the first sentence of the model's own error, out
of the stderr chapkit quotes back:

```text
chapkit-rwanda-malaria-bym-model    FAIL   14s   predict: Error in file(file, "rb") : cannot open file 'model.rds': Permission denied
  run `chap-devops models test chapkit_rwanda_malaria_bym_model -vv` for the full output, and `chap-devops logs chapkit-rwanda-malaria-bym-model` for the service's own
```

`-vv` prints the whole output of `chapkit test`. `-v` shows only the hints.

#### More data for a test

By default, `chapkit test` creates 1 config, trains once and predicts once.
The training data has 250 rows: 50 periods for 5 locations, which is about 4
years of monthly data, or 1 year of weekly data. Some models need more:

- A weekly model often needs more than 1 year of training data.
- A model with a long context window, for example many lags, needs a long
  history for each prediction.

For these models, give more rows. chap-devops gives the options to `chapkit test`:

```sh
chap-devops models test chapkit_simple_multistep_model --rows 600 --predict-rows 300
chap-devops models test chapkit_simple_multistep_model --configs 2 --trainings 2
```

With `--configs 2 --trainings 2`, each config trains twice, and each trained
model predicts once:

```text
testing 1 model (model level; add --backtest to go through chap-core)
chapkit-simple-multistep-model    pass   18s   4 trainings, 4 predictions

1 of 1 model passes
```

These options are for the model level only; `--backtest` refuses them.

A model that could not be tested is skipped. The line under the row gives the
way out:

```text
testing 1 model (model level; add --backtest to go through chap-core)
auto-arima-chapkit    skip    0s   its container is not running
  run `chap-devops up`

0 pass, 1 skipped
warning: no model was tested, because every one was skipped; the line under each row says why
```

| Skip | What to do |
| --- | --- |
| `its container is not running` | `chap-devops up`. |
| ``the image has no `chapkit test`; the service reports chapkit 1.0.0`` | The image predates the command. `chap-devops update` moves the pin; `--backtest` tests it through chap-core instead. |
| ``an MLproject service has no `chapkit test` `` | The image serves an `MLproject` file with `chapkit mlproject run`, and chapkit has `chapkit test` only in a chapkit service project. Use `--backtest`, or make the model a chapkit service with [`chap-devops chapkit mlproject migrate`](./chapkit-cli.md#migrate-an-mlproject-model). |
| `chap-core has no configured model for <service> <version>` | `--backtest` only. Run `chap-devops models configs sync ID`, then the test again. |
| `chap-core has no configured model NAME for <service> <version>; it has ...` | `--config` only. Use one of the names, or add the configured model with `chap-devops models configs add ID`. |
| `chap-core did not list its configured models, so chap-devops cannot find --config NAME` | `--config` only. Run `chap-devops status`. |
| `no answer in 5m` | The model is wedged or genuinely slow. The line under the row names `--timeout` with twice the limit. The same number is handed to chapkit as its per-job deadline. |

### The backtest level

`--backtest` goes the whole way round, the path the Modeling App takes. For
each model, `chap-devops` fetches sample data from the model itself through
chap-core's proxy (`GET .../api/v1/ml/$generate-sample-data?kind=train`,
36 periods over 5 org units, in the period type the service declares),
transposes that frame into chap-core observations, posts it as a dataset,
waits for the dataset job, runs a small rolling backtest over it (3 periods, 2
splits, stride 1) and reads the scores off the result.

Before the first model, `--backtest` creates the configured models that the
models of this deployment do not have yet, as [`chap-devops models
configs sync`](#configured-models) does. A model registered from outside the
deployment is not configured by this step.

The backtest names one of chap-core's configured models by its id, chosen from
`GET /v1/crud/configured-models`. Only a configured model of the version that
the service registered with counts, and only one that chap-core has not
archived. A service that registers with a new version keeps the configured
models of the old version, but chap-core does not run them. Without
`--config`, the backtest takes the first of these that exists:

1. The configured model named after the service: the variant name `default`.
2. The lowest-numbered `<service>:<variant>`.
3. A `<service>:test_config_...` that an earlier test left behind.

`--config NAME` names the configured model to backtest. NAME is the variant
name, as `chap-devops models configs` shows it, or the full name `<service>:<variant>`.
See [Testing a configuration](#testing-a-configuration).

The backtest first creates the configured models that the marketplace entry
gives, as `chap-devops models configs sync` does. A service that still has no live
configured model of its version is skipped, not backtested. The skip names
the version, and the line under it names `chap-devops models configs sync`. It has
this form:

```text
SERVICE    skip    0s   chap-core has no configured model for SERVICE VERSION
  it is registered, and chap-core has nothing to run it with; run `chap-devops models configs sync ID`, then `chap-devops models test ID --backtest`
```

That configured model's `additionalContinuousCovariates` are the columns the
backtest hands the model, and the sample data does not always carry them:
`$generate-sample-data` makes the service's required covariates and a pair of
climate ones, but not the ones a configuration adds - the simple multistep
model's defaults include `mean_relative_humidity`, and without it the backtest
fails inside the model with `KeyError: "['mean_relative_humidity'] not in
index"`. So at least as many `feature_N` columns are asked for as the
configuration names, and each covariate the frame lacks takes over a spare one:
the same kind of synthetic seasonal series under the name the model reads.
`-vv` says which column stood in for which.

Geometry is asked for whatever the service declares (`include_geo=true`).
chap-core's dataset always carries a GeoJSON collection, so a model that says
it needs no geometry would otherwise be handed features with none in them - and
a model that builds a neighbour graph fails on an empty polygon where it would
have been perfectly happy with no geometry at all. Real polygons are never
worse: a model that ignores geometry ignores these too. chapkit puts each
location's id in `properties.id` and nothing at the top level, and chap-core
matches org units on the feature's top-level `id` and silently drops the ones
it cannot match, so `chap-devops` sets it before posting.

```text
testing 2 models (through chap-core: a dataset, a backtest and its scores)
chapkit-rwanda-malaria-bym-model    pass      38s   crps 16.7  mae 26.2  rmse 30.6
  configured model: monthly (id 20)
chapkit-simple-multistep-model      pass      13s   crps 5.4  mae 7.3  rmse 9.0
  configured model: monthly_climate (id 14)

2 of 2 models pass
```

The line under each row names the configured model that the backtest used:
its variant name and its id in chap-core. So the scores are always of a
configured model that you can see. `--json` has the same as `configured_model`,
with `id`, `name` and `variant`. If chap-core did not list its configured
models, the backtest sends the service id, and the line says `(by name, as
chap-core could not list them)`. `id` is then `null`.

**What it proves:** everything the model level does, plus that chap-core can
reach the model, that the covariates and period type the model declares are the
ones chap-core builds a dataset from, that the org units survive the round trip,
and that a real forecast comes back with scores on it. The three printed are
chap-core's `crps`, `mae` and `rmse`; `--json` hands back all fourteen.

A job that fails prints chap-core's own reason - the same line
[`chap-devops jobs logs`](./jobs.md) digs out of the `--- stderr ---` section - and
the command that shows the whole log:

```text
chapkit-ghr-model    FAIL   1m 12s   Error in predict_chap() : the inla program crashed
  run `chap-devops jobs logs 9a84e02f-6a0e-4a3b-9a63-1f2b5a1f3f21`
```

Scores are not a verdict on the model: a two-split backtest over generated data
says the pipeline works, not that the model is any good.

### What it leaves behind, and what it cleans up

Both levels put something in a database, and both delete it again unless
`--keep` says not to.

| Level | What it creates | What is deleted |
| --- | --- | --- |
| model | one config named `test_config_<ulid>` and a few artifacts, in the **model service's own** database | The service's configs and artifacts are listed before the run and again after it, and exactly what appeared is deleted. Deleting a config cascades to the artifacts linked to it. |
| `--backtest` | one dataset and one backtest row in **chap-core's** database, plus the jobs that made them. From chap-core v2.5.0, also one backtest for each built-in baseline | The backtests first, then the dataset, and nothing else. The jobs stay, because that is the record of what was run; they show up in [`chap-devops jobs`](./jobs.md). |

chap-core's model proxy is read-only by design, so a delete cannot go through
it: the model level lists through the proxy and deletes with the `curl` every
chapkit image carries for its healthcheck (`docker compose exec -T <service>
curl -X DELETE ...`). If a delete does not work, the command says so and names
what was left, so it can be removed by hand.

`--keep` keeps it, and says what was kept. At the model level the line names
the config; it goes away with the volume of the model
(`chap-devops models disable ID --purge`):

```text
chapkit-simple-multistep-model: kept test_config_01M4CVD3GTBZGX2GGQS7131PXQ and 3 artifacts in its database; remove it with `chap-devops docker exec chapkit-simple-multistep-model curl -fsS -X DELETE http://127.0.0.1:8000/api/v1/configs/01M4CVD3GTBZGX2GGQS7131PXQ`
```

With `--backtest`, the line names the two deletes:

```text
kept backtest 8 and dataset 7; remove them with `chap-devops api DELETE /v1/crud/backtests/8` then `chap-devops api DELETE /v1/crud/datasets/7`
```

From chap-core v2.5.0, a backtest also starts one `run_baseline` job for each
built-in baseline, and each job writes a backtest on the same dataset.
chap-core leaves these out of the backtest lists, but it does not delete a
dataset while they point at it. So chap-devops waits up to a minute for the
baselines of the run, and deletes their backtests with the backtest of the
model, in one request. The line then names them all:

```text
kept backtests 25, 26, 27 and 28 and dataset 7; remove them with `chap-devops api DELETE '/v1/crud/backtests?ids=25,26,27,28'` then `chap-devops api DELETE /v1/crud/datasets/7`
```

Both lines are closing lines of the command. They go to stdout, and `--json`
carries them in `messages`.

A backtest chap-core ran also leaves a configuration in the model service's own
database, the way any backtest started from the Modeling App does. That one is
chap-core's, not the test's, so it is left alone.

### What each level needs

| Level | Requirement |
| --- | --- |
| model | The model's container running, and an image with the `chapkit` CLI in it. Every chapkit-built image has one; the marketplace images ship chapkit 2.0 or 2.1. chap-core need not be up at all - it is only asked for the model's declared period type, and for which chapkit the service reports when there is none to run. |
| `--backtest` | chap-core up, the model registered with it, and chapkit **1.1.0 or newer** in the image, which is where `$generate-sample-data` arrived. An older one is skipped with the version it reports. |

### Flags

| Flag | What it does |
| --- | --- |
| `--all` | Test every model in `.chap-devops/models.yaml`. Cannot be combined with an id. |
| `--backtest` | Run the backtest level instead of the model level. |
| `--config NAME` | Backtest the configured model with this variant name. Needs `--backtest`. With more than one model, it applies to each model. |
| `--seed N` | Seed the generated data, so two runs compare. Without it every run is fresh data. |
| `--timeout SECONDS` | How long one model gets: 300 at the model level, 900 with `--backtest`. At the model level the same number is chapkit's per-job deadline. |
| `--configs N`, `--trainings N`, `--predictions N` | The model level only. How many configs `chapkit test` creates, how many trainings each config gets, and how many predictions each trained model gets. The default of each is 1. |
| `--rows N`, `--predict-rows N` | The model level only. The rows (locations times periods) of the training data, 250 by default, and of the history and the future data of each prediction, 150 by default. See [More data for a test](#more-data-for-a-test). |
| `--keep` | Do not delete what the run created. |
| `-vv` | Stream `chapkit test`'s whole output as it runs, and narrate every request. This is what to add to a failure. |
| `--json` | One object per model: `id`, `service_id`, `level`, `result`, `seconds`, `summary`, `detail`, and - for a backtest - `job_id`, `backtest_id`, `configured_model` and the whole `metrics` object. |

## A new model project

`chap-devops models new DIR` creates a model project in a new directory. The project
builds into an image that `chap-devops models add` and `chap-devops run` take. Python, uv
and the chapkit CLI are not necessary on the machine: the templates are in
chap-devops, and the build runs in Docker.

```sh
chap-devops models new dengue-lags
chap-devops models new rain-model --template mlproject-r
```

```text
created dengue-lags in dengue-lags (fn-py template, service id dengue-lags)
next: build the image as `dengue-lags/README.md` says, then run `chap-devops run dengue-lags:dev`
```

With `-v`, two hints follow: the files it wrote, and the base image.

### The types

There are two kinds of project. A chapkit service has a `main.py` that builds
the service with chapkit. An MLproject has an `MLproject` file and scripts, and
no chapkit code: the image serves it with `chapkit mlproject run`.

| `--template` | Kind | The model is in | Base image |
| --- | --- | --- | --- |
| `fn-py` (the default) | chapkit service | Python, in `main.py` | `chapkit-py` |
| `shell-py` | chapkit service | Python scripts | `chapkit-py` |
| `shell-r` | chapkit service | R scripts | `chapkit-r` |
| `shell-r-tidyverse` | chapkit service | R scripts, with tidyverse and fable | `chapkit-r-tidyverse` |
| `shell-r-inla` | chapkit service | R scripts, with INLA | `chapkit-r-inla` |
| `mlproject-py` | MLproject | Python scripts | `chapkit-py-data-cli` |
| `mlproject-r` | MLproject | R scripts | `chapkit-r-cli` |
| `mlproject-r-tidyverse` | MLproject | R scripts, with tidyverse and fable | `chapkit-r-tidyverse-cli` |
| `mlproject-r-inla` | MLproject | R scripts, with INLA | `chapkit-r-inla-cli` |

The base images are `ghcr.io/dhis2-chap/<name>:latest`. A `-cli` image has
chapkit in it, which `chapkit mlproject run` needs. `chapkit-py-data-cli`
also has numpy, pandas, scipy, scikit-learn, statsmodels, joblib, pyyaml and
xarray, so a Python MLproject runs with no install of its own. A chapkit service installs
its own chapkit with `uv sync` in the build, from the range in its
`pyproject.toml` (`chapkit>=2.4.1,<3`). The INLA images are amd64 only, so
those Dockerfiles pin `linux/amd64`.

`--with-validation` adds the `$validate` hooks (`on_validate_train` and
`on_validate_predict`) to `main.py`. An MLproject has no `main.py`, so
`models new` refuses the option for the `mlproject-*` types.

### What the project holds

| File | Types | What it holds |
| --- | --- | --- |
| `main.py` | chapkit service | The options (the `Config` class), the service id and the metadata; for `fn-py`, also the model |
| `MLproject` | MLproject | The name, the options (`user_options`), and the train and predict commands |
| `scripts/train.py`, `scripts/predict.py` | `shell-py`, `mlproject-py` | The model |
| `scripts/train.R`, `scripts/predict.R` | the R types | The model |
| `pyproject.toml`, `.python-version` | all but the R MLproject types | The Python dependencies |
| `Dockerfile` | all | The image |
| `README.md` | all | The build, run and test commands for this project |
| `.github/workflows/publish.yml` | all | Publishes the image to GHCR on a push to `main` and on a `v*.*.*` tag |
| `.gitignore`, `.dockerignore` | all | |

Every type has the same example model: for each location, it predicts the
mean of `disease_cases` in the last `window` periods. `window` is an option,
with 3 as the default. With the same `--seed`, every type gives the same
backtest scores, so a change of type does not change the result.

The names come from the directory name:

- The **service id** is the name in lowercase, with `-` between the words, such
  as `dengue-lags`. The model registers with chap-core under this id. It is
  the `id` in `main.py`, or the `name` in `MLproject`. The README uses it as
  the image name too, so `chap-devops models add` needs no `--service-id`.
- The **model id** is the same with `_`, such as `dengue_lags`. This is the id
  that `chap-devops models add` gives the image.

A name can have letters, digits, spaces, `-`, `_` and `.`. A name that starts
with a digit gets `model-` in front of its service id.

The author in the model metadata comes from git: `user.name` is the
`author`, and `user.email` is the `contact_email`, in `main.py` or in
`MLproject`. If git has no value, the project gets a placeholder (`Your Name`,
`your.email@example.com`) for you to edit. With `-v`, `models new` says which
of the two it used.

### From the project to a backtest

In the project:

```sh
cd dengue-lags
git init && git add . && git commit -m "feat: first version"
docker build --platform linux/amd64 --build-arg GIT_REVISION=$(git rev-parse HEAD) -t dengue-lags:dev .
```

chap-core stores a model only if the image has a git revision, so the build
needs a commit and `GIT_REVISION`. Without `uv.lock`, the build resolves the
Python dependencies itself. Run `uv lock` and commit the file to make every
build get the same versions.

Then, in a deployment with chap-core:

```sh
chap-devops models add dengue-lags:dev
chap-devops up --wait
chap-devops models test dengue_lags --backtest
chap-devops models configs add dengue_lags --name long --set window=6
chap-devops models test dengue_lags --backtest --config long
```

A model-level test (`chap-devops models test` without `--backtest`) runs
`chapkit test` in the container. An MLproject image does not have it, and the
row says `skip` with ``an MLproject service has no `chapkit test` ``. To make an
MLproject a chapkit service, use `chap-devops chapkit mlproject migrate DIR`; see
[The chapkit CLI](./chapkit-cli.md#migrate-an-mlproject-model).

After a change, raise the version (`version` in `main.py`, or in `MLproject`)
before you build again. chap-core stores each version of a model only once,
and refuses a new git revision under a version it has. Then build, run
`chap-devops restart`, and test again. See
[`is stored from revision ..., but its source now reports revision ...`](./troubleshooting.md#is-stored-from-revision--but-its-source-now-reports-revision-).

To run the model service alone, with no deployment: `chap-devops run dengue-lags:dev`.

## Models outside the marketplace

A model the catalogue does not list - a new one, a private one, a fork of your
own - is added to one deployment with `chap-devops models add`:

```sh
chap-devops models add https://github.com/my-org/chapkit_dengue_model
chap-devops models add ghcr.io/my-org/chapkit_dengue_model:sha-1eb8cf1
chap-devops models add ghcr.io/my-org/chapkit_dengue_model@sha256:31163f6a...
chap-devops models add quay.io/my-org/dengue_model:v1.2      # any other registry
chap-devops models add docker.io/my-org/dengue_model:v1.2    # Docker Hub, with its host
chap-devops models add my-dengue-model:dev        # built on this machine, never pulled
```

An image reference can name any registry: ghcr.io, Docker Hub, quay.io, or a
registry of your own. Write Docker Hub images with `docker.io/` in front. A name
with no registry host is a local image (see below). The repository URL form
uses ghcr, because that is where the publish workflow of the model
repositories writes its builds.

A reference with no registry host is an image in the local image store, such as
one just built with `docker build --platform linux/amd64 -t my-dengue-model:dev .`.
It is read from the local store rather than a registry, rendered with
`pull_policy: never`, and may share a marketplace model's service id while that
model is disabled. See
[A model image you built yourself](./use-cases/local-model-image.md).

The two forms differ in one thing, and it is the important one:

- A **repository URL** follows the default branch. `chap-devops models add` asks
  GitHub for that branch and its newest commits, and ghcr for the newest of
  those commits that has a published `sha-<short commit>` build - which is the
  tag the model repositories' publish workflow writes. `chap-devops update` moves the
  pin to whatever the newest published build is then.
- An **image reference** pins exactly what it names, tag or `@sha256:` digest,
  and `chap-devops update` reports it as pinned and leaves it alone.

Either way the entry is a full definition, so everything else treats it like a
marketplace model: it is enabled by the same write path, rendered into the same
kind of overlay, listed by `models list`, described by `models info`, moved (or
not) by `chap-devops update`, seen by `chap-devops doctor` and shown in the browser.

```text
$ chap-devops -v models add https://github.com/my-org/chapkit_dengue_model --port 5001
added chapkit_dengue_model (chapkit-dengue-model)
hint: source: https://github.com/my-org/chapkit_dengue_model
hint: image: ghcr.io/my-org/chapkit_dengue_model:sha-b1d6c31
hint: pin: sha-b1d6c31 (commit b1d6c31)
hint: follows: main (`chap-devops update` moves the pin)
hint: data dir: /work/data (from the image config)
hint: user: 10001:10001 (from a docker probe)
enabled chapkit_dengue_model sha-b1d6c31 on http://localhost:5001
hint: chapkit_dengue_model is in `compose.chapkit-dengue-model.yml`
the service must register with chap-core as `chapkit-dengue-model`; if its own MLServiceInfo.id differs, `chap-devops status` shows it as unmanaged - run `chap-devops models remove chapkit_dengue_model`, then add it again with `--service-id <that id>`
run `chap-devops up` to apply
```

Every line says where its answer came from, because none of them came from a
reviewed catalogue entry:

| Value | Where it comes from |
| --- | --- |
| id | `--id`, else the repository or image name in snake_case. Not `persistence`, `global_median` or `seasonal_median`: chap-core v2.5.0 and later has models of these names, and refuses a service that uses one. |
| service | `--service-id`, else the id with hyphens. Not one of the names chap-devops uses itself - `chap`, `worker`, `redis`, `postgres`, `ocs`, `s3`, `s3-init`, `dhis2`, `dhis2-db`, `dhis2-prep`, `chap-devops`, `marketplace` - which a model would be merged into or written over; `models add` and `sync` refuse them. |
| name | `--name`, else the repository or image name. |
| pin | The newest published `sha-` build of the branch, or the tag or digest that was named. |
| data dir | `--data-dir`, else `<WorkingDir>/data` from the image config, else `/work/data`. A `--data-dir` is an absolute path of letters, digits, `/`, `.`, `_` and `-`: it goes into the rendered YAML and the init container's `chown` as it is. |
| user | `--user` (a name or an id, with a group after a `:`, such as `1000:1000`), else the image's own `User` when it is numeric or an account `chap-devops` knows, else the numbers a `docker run ... id -u` probe reads out of the image, else the name with a warning. |
| amd64 | Whether ghcr publishes the image for amd64 alone; `--runtime-amd64` records it either way. |

The data directory and the user are resolved the same way for a marketplace
model, and from the same two fields of the same image config; see
[Data directories and users](#data-directories-and-users). The difference is
only where the answer is recorded: `.chap-devops/models-manual.yaml` holds a manual
model's definition, `.chap-devops/models.yaml` holds every enabled model's.

`--port N|auto` and `--bind ADDR` publish a host port, exactly as on
`models enable`, and are the flags about the deployment rather than about the
model.

### A source the marketplace already lists

`models add` is for models the catalogue does not have, but a URL copied from
a model's page usually names one it does. Without `--id`, such a source is
enabled as the marketplace model, with the catalogue's reviewed pin:

```sh
chap-devops models add https://github.com/chap-models/chapkit_simple_multistep_model
chap-devops models add ghcr.io/chap-models/chapkit_simple_multistep_model:sha-6c1d9ee   # the same, pinned to 0.1.1
```

```text
https://github.com/chap-models/chapkit_simple_multistep_model is the marketplace model chapkit_simple_multistep_model; enabling that (pass `--id <other>` to add it beside it instead)
enabled chapkit_simple_multistep_model v0.1.2 at http://localhost:8700/v2/services/chapkit-simple-multistep-model/run/
run `chap-devops up` to apply
```

A repository URL matches the entry with that `repository` (case, a trailing
slash and `.git` aside) and follows its stable channel; an image matches only
when its tag is one the entry's versions publish, and pins that version. Any
other tag of the same image is a build the catalogue has not reviewed, so it is
refused:

```text
error: the marketplace already lists chapkit_simple_multistep_model (Simple Multistep); run `chap-devops models enable chapkit_simple_multistep_model`, or pass `--id auto` (or `--id <other>`) to add this one beside it
```

A local image (`my-model:dev`) is never matched:
running your own build of a listed model is what `models add` is for.

To keep a separate entry anyway - a fork's builds, a branch the catalogue does
not follow - pass `--id`. `--id auto` takes the source's own id, or that id
with the lowest free `_2`, `_3`, ... suffix when the marketplace or an earlier
`models add` already uses it.

```text
$ chap-devops models add https://github.com/chap-models/chapkit_simple_multistep_model --id auto
added chapkit_simple_multistep_model_2 (chapkit-simple-multistep-model-2)
enabled chapkit_simple_multistep_model_2 sha-10b2bc7 at http://localhost:8700/v2/services/chapkit-simple-multistep-model-2/run/
run `chap-devops up` to apply
warning: this is the image of the marketplace model chapkit_simple_multistep_model, which registers with chap-core as `chapkit-simple-multistep-model`, so `chapkit-simple-multistep-model-2` may never register; to run chapkit_simple_multistep_model, run `chap-devops models remove chapkit_simple_multistep_model_2`, then `chap-devops models enable chapkit_simple_multistep_model`
```

`--id auto` changes the id and the service id here, not the code in the
image. The image above still registers as `chapkit-simple-multistep-model`, so `chap-devops
status` never shows `chapkit-simple-multistep-model-2` as registered. Any `--id` on the
repository or an image of a marketplace model gives the same warning. `--service-id chapkit-simple-multistep-model` is
refused, because the marketplace model uses that name. Use `--id` for a fork
that sets its own `MLServiceInfo.id`.

The **service id has to match the id the service registers with chap-core**
(chapkit's `MLServiceInfo.id`). It is the Compose service name, the DNS name
and the name the registration resolves to, all at once. Where a model's own id
differs from its repository name, pass `--service-id`; `chap-devops status` shows the
mismatch as a registered service nobody manages next to a model that never
arrived.

The uid probe is the one step that needs Docker: an image that runs as an
account name (`USER app`) says nothing about what that name resolves to, and
the overlay's init container chowns the data volume from busybox, which
resolves no names at all. So `chap-devops models add` pulls the image once and asks
it. Pass `--user <uid>:<gid>` to skip that.

### Where it is recorded

The definition goes in `.chap-devops/models-manual.yaml`, beside the enabled set:

```yaml
# Managed by chap-devops; change it with the chap-devops commands, not by hand.
chapkit_dengue_model:
  service_id: chapkit-dengue-model
  display_name: chapkit_dengue_model
  repository: https://github.com/my-org/chapkit_dengue_model
  image: ghcr.io/my-org/chapkit_dengue_model
  tag: sha-b1d6c31
  commit: b1d6c312a83f07aa1a4e66fce05ae7f4eccb8188
  follow: main
  data_dir: /work/data
  user: 10001:10001
  runtime_amd64: true
  added: 2026-09-24
```

That file is the definition; `.chap-devops/models.yaml` still says which models are
on. So `chap-devops models disable chapkit_dengue_model` keeps the definition and
`chap-devops models enable chapkit_dengue_model` brings the model back at the recorded
tag, data directory and user, without asking the network anything. The host
port is not in the definition: if the model had one, give `--port` again. It is
carried over by `chap-devops init --force` and included in `chap-devops backup create`.
A deployment that has added nothing has no such file.

Both listings mark these entries, because a local definition is not a reviewed
catalogue entry:

```text
ID                                SERVICE                           NAME                            STATUS        STABLE       LATEST       PORT           KIND
chapkit_simple_multistep_model    chapkit-simple-multistep-model    Simple Multistep                limited data  0.1.2        0.1.2        via chap-core  model
chapkit_simple_multistep_model_2  chapkit-simple-multistep-model-2  chapkit_simple_multistep_model  not for use   sha-10b2bc7  sha-10b2bc7  via chap-core  manual
```

`chap-devops models info chapkit_simple_multistep_model_2` says the same in its own words - a
`kind` of `manual`, a `source` line naming `chap-devops models add` and the day it
was added, and a `follows` line naming the branch or reading
`nothing (pinned)` - and leaves out every field a
marketplace entry would have filled in (the horizon, the covariates, the
period types, the assessment) rather than reporting them as zeroes. The
browser shows `manual` in its `KIND` column.

### Removing one

```sh
chap-devops models remove ID [--purge] [--keep-in-chap]
```

`remove` is the way back: it disables the model if it is enabled - stopping its
container and removing its overlay, exactly as `models disable` does - and then
drops the definition. `--purge` takes the data volume as well, and means the
same thing here as it does there.

`remove` also retires the model in chap-core, as `models disable --retire`
does (see [Retiring a model in chap-core](#retiring-a-model-in-chap-core)).
The removal is permanent, so the template goes as well. `--keep-in-chap`
leaves chap-core as it is. If chap-core does not answer, `remove` stops before
it changes anything. Start chap-core, or pass `--keep-in-chap`.

A marketplace model has no local definition to remove, and says so:

```text
error: chapkit_ghr_model is a marketplace model, so there is no local definition to remove; run `chap-devops models disable chapkit_ghr_model`
```

### What it needs, and what it refuses

`chap-devops models add` reads GitHub's REST API and the registry of the image;
both are public for the Chap model repositories, so neither needs a credential.
A registry other than ghcr gives its pull token through the challenge on
`/v2/`, and chap-devops asks it for an anonymous one. A private registry that
wants credentials for every pull is not read. Run `docker login` and
`docker pull --platform linux/amd64` the image first; then the image config
comes from the local image. GitHub allows 60
requests an hour to an address that sends none, and `chap-devops` sends
`GITHUB_TOKEN` (or `GH_TOKEN`) where the environment names one, for 5000 an
hour instead - it is never stored and never printed, and `chap-devops doctor`'s
`github api` line says how much of the hour is left. The
repository form cannot work under `--offline` and says so, naming the image
reference to pass instead. The image form works offline as long as the image's
`linux/amd64` variant is in the local image store, because then `docker image
inspect` can answer what the registry would have; the error names the `docker
pull --platform linux/amd64` that puts it there.

Refused rather than guessed at: a bare image name (`chapkit_dengue_model`), an
image reference with no tag or digest,
an id or service name the marketplace already uses (`--id auto` picks a free
id), and an id this deployment
has already added. The marketplace always wins a collision, so an id it
publishes later shadows nothing: the local entry is reported and ignored until
it is removed or renamed.

## The browser

`chap-devops ui` opens two pages: **models**, the marketplace catalogue, and
**components**, what this deployment is made of. `Tab` goes to the next page
and `shift-Tab` back; one `s` saves both and leaves the browser, so the two
are a single sitting rather than two commands.

The models page is the catalogue as one table across the width of the terminal,
with a one-line summary of the row under the cursor under it:

```text
chap-devops · models                                               registry: cache · 59 min   6 models · 1 enabled · 0 pending
╭ Marketplace ─────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│     MODEL                  ID                                 STATUS         VERSION  PORT                           │
│──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────│
│ ▸ ✓ Simple Multistep       chapkit_simple_multistep_model     ● limited data 0.1.2    via chap-core                  │
│     Auto-ARIMA             auto_arima_chapkit                 ● experimental 1.0.2                                   │
│     GHRmodel               chapkit_ghr_model                  ● experimental 0.1.3                                   │
│     Rwanda Malaria BYM     chapkit_rwanda_malaria_bym_model   ● not for use  0.1.3                                   │
│──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────│
│ Simple Multistep  ● limited data  0.1.2 (sha-10b2bc7)  enabled, via chap-core  requires -               i for details│
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
 [j/k] move  [tab] page  [space] toggle  [i] info  [p] port  [s] save  [ctrl+k] commands  [?] help  [q] quit
```

The summary line names the model, what the author's assessment means, what the
row is pinned to, whether this deployment runs it and what data it needs, with
`i for details` held at the right-hand edge. A terminal too narrow for all of
it cuts the covariates first, marked with `~`; the hint never goes. Everything
the line leaves out - the summary text, the defaults, the period types, the
forecast horizon and the service user - is behind `i`.

The first column is the cursor, the second what the row will be: `✓` for a
model this deployment runs, `+` for one you have switched on, `-` for one you
have switched off. `STATUS` is the author's own chapkit assessment, the same
five words [the table above](#what-the-status-column-means) explains; `VERSION`
is what the row's channel resolves to; and `PORT` is how the model is reached.
A `KIND` column appears when the list holds something that is not a plain
marketplace model: a starter, or an entry `chap-devops models add` created. The
rows are in the same maturity order `chap-devops models list` uses, with starters
last when `t` shows them; nothing done in the browser reorders them, so the
cursor stays on the row it was on.

The keys, none of which need Alt on a Norwegian keyboard:

```text
j / k / arrows     move            space            enable or disable
g / G              first / last    i / Enter        the full details
PageUp / PageDown  jump a page     p                publish a host port: a number, auto, or none
ctrl-u / ctrl-d    jump a page     P                take the host port away
Tab / shift-Tab    change page     v                pick the channel (stable, latest)
s                  save            t                show or hide starters
u                  discard         /                filter (Enter keeps, Esc clears)
q                  quit            Esc              clear the filter, or quit
?                  help            ctrl-k / ctrl-p  the command palette
o                  open in a browser: the model's repository, or the component's web interface
c                  the model's image reference, on the status line
m                  the configured models of the model in chap-core
y / n              answer the quit confirmation
```

`ctrl-n` also moves down and `ctrl-c` always leaves. While filtering, the
arrow keys still move the cursor and `ctrl-u` empties the filter without
leaving it. An active filter shows in the title as `· filter <text>` and in
the box title as how many entries it left. The key bar gives up its entries
from the least useful end when the terminal is too narrow for all of them.

The `PORT` column reads `via chap-core` for an enabled model with no host port
of its own, the port number for one that publishes one, and `auto` for a row
that has asked for a port that is not picked until you save. A row you have
switched on or off reads `will be enabled` or `will be disabled` until you
save.

### The configured models page

`m` on a model row opens the [configured models](#configured-models) of the
model in chap-core, with the columns NAME, COVARIATES, SOURCE and OPTIONS.
The keys on this page:

```text
j / k / arrows     move
a                  add a configured model
e                  change the values of the configured model under the cursor
d                  archive the configured model under the cursor
r                  ask chap-core again
Esc / q / m        go back to the models page
```

`a` and `e` open the form. It has one field for each user option of the
model, with its kind, its default and its description, and one field for the
covariates. The form of `a` also has a field for the variant name. The form
of `e` starts with the current values. An empty field uses the default.

- `Tab` and the arrows move between the fields.
- `Enter` saves the form.
- `Esc` closes the form and writes nothing.

chap-devops writes nothing to chap-core until you answer `y` to the question. For
`d`, the question says that chap-core keeps the configured model, and that the
Modeling App shows it as Archived.

The page needs a chap-core that runs and a model that the deployment runs.
If chap-core does not answer, the page says:

```text
chap-core at http://localhost:8700 is not running; start it with `chap-devops up`, then press r
```

If the model is not enabled, or the deployment has no chap-core, the footer
line gives the way out.

### Publishing a host port

`p` opens a dialog over the list, prefilled with the port the row has today or
with `auto`:

```text
╭ Host port for Simple Multistep ────────────────────────────────────────╮
│ port  › 80_                                                            │
│ now: via chap-core · range 5001-5999 · api port 8140 is taken          │
│ port 80 is outside this deployment's range 5001-5999                   │
│                                                                        │
│ [enter] apply  [esc] cancel  [auto] any free port  [none] no host port │
╰────────────────────────────────────────────────────────────────────────╯
```

It takes a port number, `auto` for the lowest free port in the project's
range, or an empty line or `none` to take the port away again. A number it
cannot accept - outside the range, chap-core's own API port, or one another
model in the list is already asking for - keeps the dialog up with the reason
in it, as above, so the number is corrected rather than typed again. The two
checks only a save can make - a port another compose file claims, and a port
something on this machine is listening on - happen when the selection is
applied, exactly as they do for `chap-devops models expose`. `P` takes the port away
without the dialog.

### Picking a channel

`v` opens the same kind of dialog, listing both channels with the version each
one resolves to and a `✓` on the one the row follows today:

```text
╭ Channel for Simple Multistep ───────────────────────────╮
│ ▸ ✓ stable  0.1.2                                       │
│     latest  0.1.2                                       │
│                                                         │
│ [enter] apply  [esc] cancel  [j/k] move  [s/l] pick one │
╰─────────────────────────────────────────────────────────╯
```

`j` and `k` move between them, `s` and `l` jump straight to one, `Enter` takes
it and `Esc` leaves the row alone. Changing the channel re-resolves the
version when the selection is saved; changing only the port does not.

A row pinned with `--version` follows no channel. The `VERSION` column shows
the pin, and the dialog puts no `✓` on `stable` or `latest`. A third line,
which the cursor cannot reach, shows the pin:

```text
│   ✓ pinned  0.1.1                                       │
```

The summary strip shows the pin too. A save keeps the pin unless you pick a
channel.

### The components page

`Tab` from the models page opens what this deployment is made of, with the same
columns [`chap-devops components list`](./components.md) prints:

```text
chap-devops · components                                             registry: embedded   4 components · 2 enabled · 0 pending
╭ Components ──────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│     COMPONENT    STATE    REACH                    WHAT IT IS                                                        │
│──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────│
│ ▸ ✓ chap-core    enabled  http://localhost:8700    Chap itself: chap-core, its worker, Valkey and PostgreSQL         │
│   ✓ ocs          enabled  http://localhost:8790    Open Climate Service: climate data, reachable at http://ocs:9000  │
│     s3           off      -                        RustFS, an S3-compatible object store OCS will keep objects in    │
│     dhis2        off      -                        DHIS2 and its own database, for a deployment that wants one       │
│                                                                                                                      │
│                                                                                                                      │
│                                                                                                                      │
│──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────│
│ chap-core  enabled here  http://localhost:8700  compose.yml + compose.chap-devops.yml  Chap itself: chap-co~  i for details│
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
 [j/k] move  [tab] page  [space] toggle  [i] info  [o] open  [p] port  [s] save  [ctrl+k] commands  [?] help  [q] quit
```

The marker column reads the same way it does on the models page: `✓` for a
component this deployment has, `+` for one you have switched on this session,
`-` for one you have switched off. `STATE` follows it - `enabled`, `off`, and
`adding` or `removing` while a change is pending - so the row says both what is
recorded and what saving would do to it. `REACH` is where the component answers
from this machine, or `internal` for one that publishes no host port, and `-`
for one that is off. The strip under the table is the row the cursor is on:
whether this deployment has it, where it answers, which compose file it is
rendered into, and what it is, with the same `i for details` held at the
right-hand edge.

```text
chap-devops · components                                             registry: embedded   4 components · 3 enabled · 1 pending
╭ Components ──────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│     COMPONENT    STATE    REACH                    WHAT IT IS                                                        │
│──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────│
│   ✓ chap-core    enabled  http://localhost:8700    Chap itself: chap-core, its worker, Valkey and PostgreSQL         │
│   ✓ ocs          enabled  http://localhost:8790    Open Climate Service: climate data, reachable at http://ocs:9000  │
│ ▸ + s3           adding   internal                 RustFS, an S3-compatible object store OCS will keep objects in    │
│     dhis2        off      -                        DHIS2 and its own database, for a deployment that wants one       │
│                                                                                                                      │
│                                                                                                                      │
│──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────│
│ 1 pending change  nothing is written until you save                                                                  │
│ + s3                      enable, on the compose network                                                             │
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
 [j/k] move  [tab] page  [space] toggle  [i] info  [o] open   [s] save 1 change   [u] discard  [?] help  [q] quit
```

`space` toggles the component under the cursor, `p` sets its host port and `P`
takes it away, `o` opens its web interface in a browser, and `i` or `Enter`
opens the details. The pending counter in the
header, the summary strip and the quit confirmation all count both pages, so a
component change cannot be lost by saving from the models page - and `u`
discards both pages, because it means "nothing I did this session" rather than
"nothing on this page".

The port prompt here is narrower than the models one: it takes a port number,
or an empty line or `none` to take the port away, and **not** `auto`. `auto`
means "the lowest free port in this project's model range", and a component
publishes a well-known port of its own that is not in that range, so the prompt
says what to type rather than picking a number nobody asked for. `p` on
`chap-core` is refused outright, because its host port is the API port, which
`CHAP_API_PORT` in `.env` sets: the footer names that line instead. `p`
on a component that is off asks you to enable it first.

`o` opens the row's web interface in a browser - chap-core's API documentation,
the OCS web interface, DHIS2's own - and says in the footer when there is none
to open: a component this deployment does not have, one that publishes no host
port (naming the address it is reached at inside the deployment), one this
session has only just enabled and not saved, and the object store, which speaks
the S3 API and serves no interface a browser is useful for. It opens what this
deployment publishes **now**, so a port changed in this session and not yet
saved is not the port it opens. `Open web interface` in the palette does the
same thing. It is the same command as
[`chap-devops open`](./components.md#reaching-a-component-from-a-browser), which is
the one that can also say whether the container is running.

`i` opens the details, which say where the component's compose file and data
volume are, and - for OCS - which of its settings this page deliberately does
not edit:

```text
╭ ocs ────────────────────────────────────────────────────────────────────────────────────────── enabled here ╮
│ Open Climate Service: climate data, reachable at http://ocs:9000                                            │
│                                                                                                             │
│ state       enabled                                                                                         │
│ reach       http://localhost:8790                                                                           │
│ compose     compose.ocs.yml · rendered from .chap-devops/components.yaml by `chap-devops sync`                          │
│ volume      ocs_data · kept when the component is disabled                                                  │
│ config      ocs/climate-service.yaml · yours to edit; chap-devops changes only its read_only and plugins_dir keys │
│                                                                                                             │
│ not on this page:                                                                                           │
│   `chap-devops components enable ocs --base-url URL`                                                              │
│     the public origin OCS builds its STAC and openEO links from                                             │
│   `chap-devops components enable ocs --read-only`                                                                 │
│     refuse ingestion over HTTP (--read-write allows it again)                                               │
╰─────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
```

Those two are one-way settings on a file rather than a field on a row:
`--base-url` is a URL to type correctly rather than pick, and `--read-only`
edits `ocs/climate-service.yaml`, which is the operator's file. A dialog that
looked like it owned them would be a dialog that quietly rewrote your config,
so the overlay names the commands instead. See
[Behind a reverse proxy](./components.md#behind-a-reverse-proxy) and
[Read-only instances](./components.md#read-only-instances).

DHIS2's overlay is the longest one, because a DHIS2 is the component with the
most that cannot be seen from its row:

```text
╭ dhis2                                                                 enabled here ╮
│ DHIS2 and its own database, for a deployment that wants one                        │
│                                                                                    │
│ state       enabled                                                                │
│ reach       http://localhost:8780                                                  │
│ compose     compose.dhis2.yml · rendered from .chap-devops/components.yaml by `chap-devops     │
│             sync`                                                                  │
│ volume      dhis2_home, dhis2_db, dhis2_dump · kept when the component is disabled │
│ config      dhis2/dhis.conf · yours to edit, and DHIS2 will not start without it   │
│ seed        https://databases.dhis2.org/climate/laos/2.42/laos.sql.gz · restored   │
│             once, into the database the first `chap-devops up` creates                   │
│ cache       dhis2_dump holds the downloaded dump rather than data; the dhis2-dump  │
│             one-shot fetches it again when the volume is empty                     │
│ first start minutes, not seconds: DHIS2 migrates its schema on the way up, and     │
│             `chap-devops logs dhis2` is where that shows                                 │
│                                                                                    │
│ not on this page:                                                                  │
│   `chap-devops components enable dhis2 --seed SPEC`                                      │
│     the dump a database being created is restored from: default, none, a URL, or a │
│     path in the deployment directory                                               │
│   `v` on this row, or `chap-devops components enable dhis2 --tag TAG`                    │
│     the DHIS2 version, 2.42 here; it migrates a schema forward only, so run `chap-devops │
│     backup create` first                                                           │
╰────────────────────────────────────────────────────────────────────────────────────╯
```

Four things only the overlay says. That `dhis2/dhis.conf` is yours **and** that
DHIS2 does not start without it. That one of the three volumes is a download
cache rather than data. What the database will be restored from, and that a
restore only ever happens to a database being created - so changing the seed on
a deployment that has already started means removing the volume. And that the
first start takes minutes, which is the difference between reading the first
`chap-devops status` as slow or as broken.

The two settings at the bottom are a key in `.chap-devops/components.yaml` and a
`chap-devops sync`. No flag changes the seed after `init`: it is a one-chance setting.
The image tag also has `--tag` on `chap-devops components enable dhis2`, and `v` on
the `dhis2` row picks it, since DHIS2 is the one component with versions to
choose between. See
[Changing the DHIS2 version](./dhis2.md#changing-the-dhis2-version).

### The details, and the command palette

`i` or `Enter` opens the full entry over the list, in the order the Chap
Modeling App presents a model: what it does, what it is, who wrote it, and
only then how this deployment runs it.

```text
╭ Rwanda Malaria BYM  chapkit_rwanda_malaria_bym_model              not enabled here ╮
│ Spatio-temporal Bayesian model for malaria incidence in Rwanda at sector (ADM3)    │
│ level: BYM spatial effects, RW1 temporal effects and an IID space-time interaction │
│ with lagged climate covariates, fitted with R-INLA. Built for one country's data — │
│ it needs geometry and the full climate covariate set.                              │
│                                                                                    │
│ version     0.1.3 (sha-1ee5ea1) · verified · channels stable, latest               │
│             0.1.2 (sha-a7b2892) · verified                                         │
│ author      Similien NDAGIJIMANA · HISP Centre, University of Oslo ·               │
│             knut.rand@dhis2.org                                                    │
│ status      ● gray, not intended for use, deprecated or kept for backwards         │
│               compatibility                                                        │
│ period      monthly · horizon 1 to 24 periods                                      │
│ target      disease cases                                                          │
│ covariates  population, rainfall, mean_temperature, relative_humidity · defaults - │
│             · free extras allowed · geometry required                              │
│                                                                                    │
│ reach       via chap-core (through chap-core's /run/ proxy)                        │
│ image       ghcr.io/chap-models/chapkit_rwanda_malaria_bym_model:sha-1ee5ea1       │
│ runtime     ghcr.io/dhis2-chap/chapkit-r-inla (amd64 only)                         │
│                                                                                    │
│ maintainers mortenoh, edvinstava                                                   │
│ repository  https://github.com/chap-models/chapkit_rwanda_malaria_bym_model        │
│ citation    Climate Health Analytics Platform. 2025. "Kigali Malaria BYM Model".   │
│             HISP Centre, University of Oslo.                                       │
╰────────────────────────────────────────────────────────────────────────────────────╯
```

The title carries the name, the marketplace id and whether this deployment
runs it - `enabled, via chap-core`, `enabled on port 5010` or `not enabled
here`; the id is the part that gives way when the three do not fit. `version`
is the pin, how far that release is trusted and which channels point at it,
with the rest of the published history under it when there is more than one.
`target` is what chap-core forecasts, the same for every model in the
catalogue. A model this deployment runs also lists its data directory and its
service user with where that user came from. A model with a citation in the
marketplace also gets a `citation` row, with the attribution as the marketplace
gives it. Long values - the summary, the assessment, the citation - wrap under
their label rather than being cut.

`j` and `k` scroll it when it is taller than the terminal, `o` opens the
repository in a browser, `c` puts the image reference on the status line, and
`esc`, `i` or `q` closes it again. A component's overlay binds `o` too, where it
opens that component's web interface, and binds no `c`: a component has no image
reference of its own.

`ctrl-k` or `ctrl-p` opens a command palette: type to narrow it, `up` and
`down` to move, `Enter` to run, `esc` to leave. Its first entry is always the
other page - `Go to the components page`, or `Go to the models page` - and the
rest follow whichever page you are on, so on the components page it offers
`Enable or disable the ocs component`, `Set a host port for the ocs component`
and `Open the web interface of the ocs component` where the models page offers
the channel, the starter filter and `Open the repository of <model>`.

It offers everything the keys do, plus three the keys do not: refreshing the
catalogue from the marketplace without leaving the browser, the way
`chap-devops registry update` does, opening this documentation - the chapter for the
page you are on - and `Save a screenshot (SVG)`, which writes the frame the
palette just closed over to `chap-devops-ui-<date>-<time>.svg` in the directory
`chap-devops ui` was started in and names the file on the status line. The two port
commands are separate, so "set a host port" never takes one away.

Every component is also in the palette by name, from either page - `Turn the
dhis2 component on or off` and one such entry per component - because that is
where somebody looking for OCS, an object store or a DHIS2 finds out the browser
has them at all. Running one turns it on or off and moves the components page's
cursor to it, without moving the page you are on.

### Saving

With something unsaved, the summary strip lists what saving would write, one
line per change, and the key bar grows a filled `[s] save N changes` and a
`[u] discard` next to it. Both pages are in that count and in that one save:
`s` writes the model changes through the same path as `chap-devops models enable` and
the component changes through the same path as `chap-devops components enable`. Then
the browser closes and prints what changed:

```text
updated chapkit_simple_multistep_model v0.1.2 on http://localhost:5001
enabled the s3 component
run `chap-devops up` to apply
```

A save writes the files and starts nothing: run `chap-devops up` to apply it. The
help line of `s` says `` save the changes; `chap-devops up` applies them ``, and the
palette entry says `` Save the changes; `chap-devops up` applies them ``. With nothing pending, `s` prints `no
changes to save` and closes the browser.

Nothing is written until you save, and quitting with unsaved changes asks
first, on either page and for either kind of change: `Quit anyway? y / n`.
`chap-devops ui` owns the terminal, so it rejects `--json`.

`chap-devops init --interactive` opens the same browser to pick the initial model
set instead of reading `--models`.

## How a model overlay looks

Each marketplace model is a [chapkit](https://github.com/dhis2-chap/chapkit)
container listening on port 8000 with `/health` and `/api/v1/info`. This is the
overlay `chap-devops` writes for Simple Multistep. The file carries two header lines
and nothing else by way of explanation: what it does is below.

```yaml
# Generated by chap-devops from .chap-devops/; edit there and run `chap-devops sync`.
# chapkit_simple_multistep_model 0.1.2 (https://github.com/chap-models/chapkit_simple_multistep_model)
services:
  chapkit-simple-multistep-model:
    labels:
      io.github.dhis2-chap.chap-devops.role: model
      io.github.dhis2-chap.chap-devops.model: "chapkit_simple_multistep_model"
      io.github.dhis2-chap.chap-devops.kind: init
    restart: unless-stopped
    image: ghcr.io/chap-models/chapkit_simple_multistep_model:${CHAPKIT_SIMPLE_MULTISTEP_MODEL_IMAGE_TAG:-sha-10b2bc7}
    platform: linux/amd64
    init: true
    expose:
      - "8000"
    environment:
      SERVICEKIT_ORCHESTRATOR_URL: http://chap:8000/v2/services/$$register
      # Uncomment if chap has SERVICEKIT_REGISTRATION_KEY set:
      # SERVICEKIT_REGISTRATION_KEY: ${SERVICEKIT_REGISTRATION_KEY:-}
    read_only: true
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    user: 1001:1001
    volumes:
      - type: tmpfs
        target: /tmp
        tmpfs:
          size: 2000000000
      - type: volume
        source: ck_chapkit_simple_multistep_model_data
        target: /app/data
    depends_on:
      chapkit-simple-multistep-model-init:
        condition: service_completed_successfully
      chap:
        condition: service_healthy

  chapkit-simple-multistep-model-init:
    labels:
      io.github.dhis2-chap.chap-devops.role: model
      io.github.dhis2-chap.chap-devops.model: "chapkit_simple_multistep_model"
      io.github.dhis2-chap.chap-devops.kind: init
    image: busybox:1.38
    command: ["sh", "-c", "chown -R 1001:1001 /app/data"]
    user: "0:0"
    volumes:
      - type: volume
        source: ck_chapkit_simple_multistep_model_data
        target: /app/data
    restart: "no"

volumes:
  ck_chapkit_simple_multistep_model_data: {}
```

### What an overlay contains

The overlay does seven things.

- **Labels both containers** with their role, the marketplace id and the
  deployment's kind, so `docker ps --filter label=io.github.dhis2-chap.chap-devops.model`
  finds every model on the machine. See
  [Container labels](./concepts.md#container-labels).
- **Pins the image.** `image: <repo>:${<ID>_IMAGE_TAG:-sha-<commit>}`, where the
  default is the `image_tag` of the version the channel resolves to. The
  deployment stays reproducible, and a different build is one `.env` line away.
- **Self-registration.** The service registers itself with chap-core through
  `SERVICEKIT_ORCHESTRATOR_URL: http://chap:8000/v2/services/$$register` (the
  `$$` is a literal `$` for Compose). The Compose service name, the DNS name
  and the marketplace `service_id` are the same string, which is what makes the
  registration resolvable. Set `SERVICEKIT_REGISTRATION_KEY` in `.env` to
  require a shared secret. In a deployment without chap-core the whole
  `environment:` block is left out, so the service registers nowhere; see
  [Model services without chap-core](./components.md#model-services-without-chap-core).
- **Publishes no host port.** The service gets `expose: ["8000"]` and nothing
  else, unless the deployment has no chap-core, where a model gets a port by
  default. See [Ports](./ports.md).
- **Hardening**, matching the posture of the base services: `init: true`,
  `read_only: true`, `no-new-privileges`, `cap_drop: [ALL]`, a 2 GB tmpfs at
  `/tmp`, and a named volume for the model's data directory (the only writable
  path besides `/tmp`).
- **Runs it as the account its image declares.** `user: <uid>:<gid>`, in
  numbers, for an image that drops to an account of its own. An image that
  runs as root gets no `user:` line at all: the line exists to override the
  image, and overriding root with root only risks taking a permission the
  image's own binaries need. See
  [Data directories and users](#data-directories-and-users).
- **Hands the data volume to the model user**, through the init container
  below - again, only where the model is not root.
- **Ordering.** `depends_on`: the init container with
  `condition: service_completed_successfully` (where there is one) and `chap`
  with `condition: service_healthy`, so a model only starts once its volume is
  writable and chap-core can accept its registration. Without chap-core only
  the init container is waited for.

Three things an overlay deliberately leaves out:

- no `networks:` key, so the service joins the compose default network only -
  which reaches chap-core but not PostgreSQL or Valkey;
- no `healthcheck:`, because chapkit images ship their own;
- no `depends_on` between models, because they are independent of each other.

## The init container, and why

Docker seeds a fresh named volume from whatever the image has at the mount
point, ownership included, so an image that never creates its data directory
yields a root-owned volume the unprivileged model cannot write to, and chapkit
dies on `sqlite3.OperationalError: unable to open database file`.

Compose cannot `chown` a volume, so the overlay of a model that runs as an
unprivileged account ships a one-shot `<service_id>-init` container (busybox,
as root, `restart: "no"`) that chowns the mount point, recursively, to the
model's **numeric** uid:gid before the model starts. Busybox resolves no `chapkit`
account, which is why the numbers matter, and why the `user:` line above
carries the same two numbers rather than the name. It is the one service in a
deployment that is deliberately not `restart: unless-stopped` (the `"no"` is
quoted because bare `no` is YAML's `false`).

**Every** model overlay gets one, root included, where the chown is to `0:0`.
A volume docker has just created is root-owned already, so on a fresh
deployment a root model's init container does nothing - but a volume that
already exists carries whoever owned it last. If a model that ran as
`1000:1000` now runs as root (a new image, or a `--user` override), its volume
is owned by 1000, and the model cannot write to it: the overlay drops every
capability, and `CAP_DAC_OVERRIDE` is the one that lets root ignore the
permission bits. One busybox one-shot costs a second on `chap-devops up` and repairs
that change, which is worth more than the line it saves.

The container has a second job: because it mounts the same named volume at the
same path as the model itself, `chap-devops backup` reads and writes model data
through it, whether the model is running, stopped, or brought down entirely.
See [Backup and restore](./backup.md).

## Data directories and users

Both differ per image, and both are read off the image itself when the model
is enabled - `chap-devops models enable`, `chap-devops init --models`, the browser's
toggle. The answers go into `.chap-devops/models.yaml`, so `chap-devops sync` renders the
overlay from what is recorded and never asks the network: the same state
renders the same bytes on any machine.

| Value | Where it comes from, in order |
| --- | --- |
| data dir | `--data-dir`, else `<WorkingDir>/data` from the image config, else the built-in table, else `/work/data`. |
| user | `--user`, else `config.User` from the image config on its registry, else the same field from an image already pulled here (`docker image inspect --platform linux/amd64`), else the built-in table. |

`chap-devops models info <id>` and the browser's details overlay name which of those
answered: `image config`, `docker probe`, `--user` or `table`. A run that could
reach neither the registry nor a local copy says so, because the table is a snapshot
this binary was built with and an image can change what it runs as.

What the marketplace images declare today:

| Image | Data directory | User |
| --- | --- | --- |
| The simple multistep model | `/app/data` | `chap`, rendered as `1001:1001` |
| The Rwanda BYM model | `/work/data` | `chapkit`, rendered as `1000:1000` |
| GHRmodel | `/work/data` | `app`, probed as `10001:10001` |
| Everything else | `/work/data` | `root` |

**An image that runs as root gets no `user:` line**; its init container is
rendered like any other model's and chowns the volume to `0:0`. Three of the
seven marketplace images end their Dockerfile on `USER root`, and forcing an
unprivileged uid on one of them takes away a permission its own binaries need:
the Rwanda BYM model up to 0.1.1 kept its INLA binaries root-owned and mode
744, so every prediction failed with `inla.run: Permission denied`. That is
why the image is asked rather than assumed: 0.1.2 rebuilt it to run as
`chapkit`, and re-enabling the model is what picks that up. See
[`Permission denied` from a model's own binaries](./troubleshooting.md#permission-denied-from-a-models-own-binaries).

The init container needs an account name as numbers: `chapkit` is uid/gid 1000
and `chap` is 1001. GHRmodel's `app` is not one of them, so enabling it pulls
the image once and asks it, exactly as `chap-devops models add` does. A `--user` that
is already numeric is passed through, and a name nothing could resolve falls
back to `1000:1000` with a warning from `chap-devops sync`. `--data-dir` and `--user` override everything above, for an image
whose config says something the deployment has to contradict.

`chap-devops doctor` re-checks each enabled model against the image on this machine
and warns when the two have drifted apart, which is what a marketplace model
moved to a new tag by hand looks like.

A model added with `chap-devops models add` is resolved the same way at the moment it
is added, and carries its answers in `.chap-devops/models-manual.yaml`. See
[Models outside the marketplace](#models-outside-the-marketplace).

## The amd64 pin, and why

The overlay of every marketplace model pins `platform: linux/amd64`, and the
overlay of a model added with `chap-devops models add` pins it only when its image is
published for amd64 alone (or `--runtime-amd64` says so). The marketplace images are
published for amd64, as is chap-core itself, so an arm64 host such as Apple
silicon pulls that variant and runs it under emulation instead of failing with
`no matching manifest for linux/arm64`.
