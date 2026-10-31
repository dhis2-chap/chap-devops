# The chap CLI

chap-core has its own command line, `chap`. It evaluates a model on a dataset
of your own, plots the result and exports the scores. It is usually installed
with uv (`uvx --from chap-core chap ...`). That needs Python and uv, and for
many models also R.

`chap-devops chap` runs the same `chap` in a container. You need Docker and
nothing more:

```sh
chap-devops chap eval --model-name https://github.com/dhis2-chap/minimalist_example_r \
  --dataset-csv laos_subset.csv --output-file r.nc
chap-devops chap plot-backtest r.nc --output-file r.html
chap-devops chap export-metrics r.nc --output-file r.csv
```

For a guide from an empty machine to a comparison of two models, see
[Evaluating a model on your own data](./use-cases/evaluate-with-chap-cli.md).

Each argument after `chap` goes to chap unchanged. To read chap's own help,
run `chap-devops chap` with no arguments, or `chap-devops chap eval --help` for one
command. `chap-devops chap --help` shows only the options of chap-devops itself. These
come between `chap` and chap's own command:

| Option | What it does |
| --- | --- |
| `--tag TAG` | The chap-core version to run. See [Which version](#which-version). |
| `--image core` or `--image worker` | The image to run in. See [Which image](#which-image). |
| `--group NAME` | The `chap-devops run` group whose models chap can reach. See [Chapkit models](#chapkit-models-no-chap-devops-up). |
| `--stop` | Stop the model that this run started, after the run. See [Chapkit models](#chapkit-models-no-chap-devops-up). |
| `--timeout SECONDS` | How long chap-devops waits for a model that it starts (300). |
| `--docker` | Give the container the docker socket. See [Models that run in docker](#models-that-run-in-docker). |

For example: `chap-devops chap --tag master --docker eval --model-name ...`.

## Your files

chap-devops mounts the current directory in the container at the same path. Thus:

- a relative path such as `laos_subset.csv` or `out/r.nc` means the same file
  in the container and on your machine;
- every file chap writes in the current directory is on your machine after
  the run, and belongs to your user;
- a dataset CSV finds its GeoJSON beside it, as it does without chap-devops.

A path outside the current directory also works: an absolute path, a `../`
path, or the same after `=` (`--output-file=/data/out/r.nc`). chap-devops mounts
the nearest existing directory above the path, at the same path. chap does not
make a directory that does not exist, so make the output directory first.

Before the run, chap-devops writes on stderr which directories chap can see. After
the run, it names the files that are new or changed. With `-v`, it also gives
the next command as a hint:

```text
running `chap eval` in ghcr.io/dhis2-chap/chap-worker:v2.3.1
files: chap reads and writes in /home/me/work
...
chap finished; it wrote r.nc
hint: `chap-devops chap plot-backtest r.nc --output-file r.html` plots it
```

In a deployment, run `chap-devops chap` in a subdirectory, for example `eval/`, so
that chap's files do not mix with the compose files. chap-devops finds the
deployment from a subdirectory too. When you run it in the deployment
directory itself, chap-devops writes a line that says so.

chap's own output is not changed. chap-devops writes its lines on stderr, with
the levels of [Status and output](./status.md#levels), and `--json` does not
apply to this command. A run that only asks for help
(`chap-devops chap`, `--help`, `--version`) gets only the first line.

## Which image

chap-core publishes two images with the same tags. chap-devops selects one from
`--model-name`:

| The model | The image | Size |
| --- | --- | --- |
| A GitHub repository, or a model directory | `chap-worker` | about 12 GB |
| A chapkit service (an `http://` URL that is not GitHub, or a model id) | the image that is on this machine; else `chap-core` | |
| A built-in model (`builtin:persistence`) | the image that is on this machine; else `chap-core` | |
| No model (`--help`, `plot-backtest`, `export-metrics`, ...) | the image that is on this machine; else `chap-core` | |

The worker image has uv, R and INLA, and chap-core's own worker runs models in
it, so a model that needs R always runs there. Everything else is plain chap
and HTTP, and both images can do it, so chap-devops takes the image that this
machine has at the tag of the run, and downloads nothing:

- Only `chap-worker` is here, as in a deployment that runs chap-core: chap-devops
  uses it.
- `chap-core` is here, or neither image is: chap-devops uses `chap-core`, the smaller
  download (about 2.4 GB). A deployment without chap-core, such as `chap-devops init
  --only dhis2`, does not pull the 12 GB worker image for `chap-devops chap --help`.

`--image core` or `--image worker` overrides the choice. A model that needs R
fails in the core image with `Rscript: not found`.

The first run pulls the image. The line before the run says so, with the
size.

## Which version

- In a deployment (in its directory, or with `-C`), chap-devops uses the
  deployment's chap-core tag. The CLI and the server are then the same
  version.
- Outside a deployment, chap-devops uses the newest chap-core release.
- With `--offline`, or when the newest release cannot be read, chap-devops uses the
  newest tag on this machine: of `chap-worker` when the run needs it, and else
  of either image.
- `--tag TAG` overrides all of these, for example `--tag master`.

## Chapkit models: no `chap-devops up`

A chapkit model is a model service: chap sends it data over HTTP, and the
service trains and forecasts. `chap eval` needs only that service. It does not
need chap-core, its database or its worker. So you do not run `chap-devops up` for
an evaluation. Give the model's id, and chap-devops starts the model:

```sh
chap-devops chap eval --model-name auto_arima_chapkit \
  --dataset-csv laos_subset.csv --output-file arima.nc
```

What chap-devops does with the id:

| Where you are | The id | What chap-devops does |
| --- | --- | --- |
| In a deployment that has the model | `auto_arima_chapkit` | Starts only that model's container, with no chap-core, if it does not run. |
| In a deployment that does not have it | `chapkit_ghr_model` | Stops, and names `chap-devops models enable chapkit_ghr_model`. chap-devops does not change the deployment's models for an evaluation. |
| Outside a deployment | any marketplace id | Starts the model in a `chap-devops run` group, as `chap-devops run ID` does, and joins that group's network. `--group NAME` selects the group. |

Before the run, chap-devops checks that the model answers. Then it gives chap the
model's URL on the network, for example `http://auto-arima-chapkit:8000`,
and writes:

```text
starting auto-arima-chapkit for this run, without chap-core
model: auto_arima_chapkit at http://auto-arima-chapkit:8000 answers
```

A model that chap-devops started keeps running, so the next evaluation starts at
once. The last line says how to stop it. `--stop` stops it after the run
instead. A model that ran before the command is never stopped. `--timeout
SECONDS` (300) is how long chap-devops waits for a model that it starts.

A model that chap-devops starts in a deployment does not register with chap-core,
because chap-core does not run. If you run `chap-devops up` later, compose recreates
the model's container, and the model then registers as usual.

A word is a model id only when no file or directory of that name is in the
current directory. A directory of the same name is used as a model directory,
as chap does.

### A model URL

You can also give a URL, for example a model on another server:
`--model-name https://models.example.org`. chap-devops asks the URL for
`/api/v1/info` before the run. If it does not answer, chap-devops stops with a note,
and chap does not start:

```text
error: the model server at http://nothing:8000 is not running (no answer on /api/v1/info), so chap was not started; ...
```

The URL of a model of the deployment or group, such as
`http://auto-arima-chapkit:8000`, works like its id: chap-devops starts the model
if it does not run.

Do not use the `http://localhost:PORT` URL that `chap-devops ps` or `chap-devops status`
shows. In the container, `localhost` is the container itself, so that URL
reaches nothing. chap-devops refuses such a URL before the run, and names the
service URL of the same model when it knows it.

An evaluation of a chapkit service stores a model configuration and its
artifacts in that service's own database, as it does without chap-devops.

## Models that run in docker

A model whose `MLproject` has `docker_env` starts a container of its own. For
that, chap needs the docker socket. `--docker` gives it:

```sh
chap-devops chap --docker eval --model-name https://github.com/dhis2-chap/chap_auto_ewars \
  --dataset-csv laos_subset.csv --output-file ewars.nc
```

The socket gives the container full control of docker on this machine, so the
flag is never the default, and chap-devops writes a warning when it is given. The
model's run directory is mounted at the same path on the host and in the
container, so the paths that chap gives docker are correct.

chap-devops reads the `MLproject` before the run: from the directory, or from
GitHub. If the model uses `docker_env` and `--docker` is not given, chap-devops
stops before the run and tells you to add the flag. If chap-devops cannot read the
file (for example, with `--offline`), it does not check it.

## Built-in models

From chap-core v2.5.0, chap has models of its own. They are baselines to
compare a model with. Give them with the `builtin:` prefix:

```sh
chap-devops chap eval --model-name builtin:seasonal_median \
  --dataset-csv laos_subset.csv --output-file seasonal_median.nc
```

The names are `persistence`, `global_median` and `seasonal_median`. A
built-in model runs in chap itself, so chap-devops starts no model and uses either
image. An older tag does not have these models, and chap-devops refuses them there.

## An MLproject as a chapkit service

From chap-core v2.5.0, `--run-config.as-chapkit` makes chap start a model
directory or a GitHub repository as a chapkit service in the container. chap
then talks to it over HTTP, as it does to a deployed model:

```sh
chap-devops chap eval --model-name ./my_model --run-config.as-chapkit \
  --dataset-csv laos_subset.csv --output-file my_model.nc
```

The `MLproject` can use `uv_env`, `renv_env`, or no environment. chap refuses
`docker_env`, `conda_env` and `python_env` in this mode, so chap-devops does not ask
for `--docker`. An `MLproject` from `chapkit mlproject export` needs this mode:
without it, its commands do not find `config.yml`. To serve the same model
outside chap, use `chap-devops chapkit mlproject run`
([An MLproject model, with no image to build](use-cases/mlproject-model.md)).

## Simulate data

From chap-core v2.5.0, `chap simulate` runs the `simulate` entry point of a
model, and writes the covariates with a simulated target to a CSV file:

```sh
chap-devops chap simulate --model-name ./my_model --dataset-csv covariates.csv \
  --out-file sim/simulated.csv
```

chap-devops makes the directory of `--out-file`, as it does for `--output-file`.

## The caches

The model runs and the uv, Python and R caches are in `chap/` under the data
directory of chap-devops: `$CHAP_DEVOPS_DATA_DIR`, else `$XDG_DATA_HOME/chap-devops`, else
`~/.local/share/chap-devops`. The second run of a model thus installs nothing again. There is one directory for
each model in `chap/runs`, and the R packages can use some hundred megabytes.
To get the space back, delete the directory:

```sh
rm -rf ~/.local/share/chap-devops/chap
```

## Limits

- chap-devops runs the amd64 images. On Apple silicon, Docker runs them under
  emulation, which is slower.
- `chap ui` is not part of chap-core yet, so `chap-devops chap ui` does not work.
- On Windows, use WSL with the Linux binary, as for the rest of chap-devops.
