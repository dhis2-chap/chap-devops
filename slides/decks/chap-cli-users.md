---
marp: true
theme: chap-devops
paginate: true
footer: chap-devops - the chap CLI without Python
title: The chap CLI, without Python
description: For people who use `uvx --from chap-core chap`: the same commands through chap-devops chap, and how it works
---

<!-- _class: title -->
<!-- _paginate: false -->
<!-- _footer: '' -->

# The chap CLI, without Python

## `uvx --from chap-core chap` becomes `chap-devops chap`

For people who use the chap CLI already

---

## What this deck covers

1. What changes for you, and what does not
2. How `chap-devops chap` runs chap: the container, the mounts, the user
3. Which image and which chap-core version it runs
4. Models: GitHub, directories, URLs, marketplace ids and built-in models
5. Models that run in docker, and an MLproject as a chapkit service
6. Your files: what is visible, what is written, what chap-devops reports
7. Every message that stops a run, and what to do
8. A full example: three models, compared
9. Troubleshooting

---

## What you do today

```sh
uv tool install chap-core --python 3.13
chap eval --model-name https://github.com/dhis2-chap/minimalist_example_r \
  --dataset-csv laos_subset.csv --output-file r.nc
chap plot-backtest r.nc --output-file r.html
```

Before the first evaluation, you need:

- Python 3.13 and uv
- R, renv and INLA, for the R models
- Docker, for the `docker_env` models
- the same versions on every machine of your team

---

## What you do with chap-devops

```sh
chap-devops chap eval --model-name https://github.com/dhis2-chap/minimalist_example_r \
  --dataset-csv laos_subset.csv --output-file r.nc
chap-devops chap plot-backtest r.nc --output-file r.html
```

- **The same commands, the same options.** Each argument after `chap` goes to
  chap unchanged.
- You need **Docker and nothing more**. chap runs in chap-core's own image.
- R, INLA and uv are in the image already.

---

## One change of words

| Today | With chap-devops |
| --- | --- |
| `chap eval ...` | `chap-devops chap eval ...` |
| `chap plot-backtest ...` | `chap-devops chap plot-backtest ...` |
| `chap export-metrics ...` | `chap-devops chap export-metrics ...` |
| `chap validate data.csv` | `chap-devops chap validate data.csv` |
| `chap --help` | `chap-devops chap` |
| `chap eval --help` | `chap-devops chap eval --help` |
| `uvx --from chap-core==2.5.0 chap ...` | `chap-devops chap --tag v2.5.0 ...` |

---

## The options of chap-devops itself

They come **between** `chap` and chap's own command:

| Option | What it does |
| --- | --- |
| `--tag TAG` | The chap-core version to run |
| `--image core` or `--image worker` | The image to run in |
| `--group NAME` | The `chap-devops run` group whose models chap can reach |
| `--stop` | Stop the model that this run started, after the run |
| `--timeout SECONDS` | How long to wait for a model that chap-devops starts (300) |
| `--docker` | Give the container the docker socket |

For example: `chap-devops chap --tag master --docker eval ...`. `chap-devops chap --help`
shows these options, and `chap-devops chap` alone shows chap's own help.

---

## Under the hood: one `docker run`

```text
docker run --rm -i [-t] --init --platform linux/amd64
  --user <your uid>:<your gid>
  -v <cwd>:<cwd> -w <cwd>
  -v <data>/chap:<data>/chap
  -e HOME=/tmp -e CHAP_RUNS_DIR=<data>/chap/runs ...
  [--network <deployment>_default]
  --label com.winterop.chap-devops.kind=cli
  ghcr.io/dhis2-chap/chap-worker:v2.5.0 chap eval ...
```

- `--rm`: the container goes when chap stops. `--init` passes Ctrl-C on.
- `--platform linux/amd64`: the chap-core images are amd64. On Apple
  silicon, they run under emulation.
- `-t` only when your terminal is a terminal, so a pipe works too.

---

## The same path, inside and outside

`-v <cwd>:<cwd> -w <cwd>` mounts your directory **at the same path**:

- `--dataset-csv laos_subset.csv` reads your file, and finds the
  `.geojson` beside it, as before.
- `--output-file r.nc` writes into your directory.
- An absolute path in the output means the same file on both sides.

`--user <uid>:<gid>` runs chap as you, so the files belong to you, not to
root.

---

## The caches: the second run is faster

`<data>` is chap-devops's data directory: `~/.local/share/chap-devops`, or
`$CHAP_DEVOPS_DATA_DIR`. chap-devops sets:

| Variable | Value | What it keeps |
| --- | --- | --- |
| `CHAP_RUNS_DIR` | `<data>/chap/runs` | one directory for each model |
| `UV_CACHE_DIR` | `<data>/chap/cache/uv` | Python packages |
| `UV_PYTHON_INSTALL_DIR` | `<data>/chap/cache/python` | Python versions |
| `RENV_PATHS_ROOT` | `<data>/chap/cache/renv` | R packages |
| `RENV_CONFIG_SANDBOX_ENABLED` | `FALSE` | no read-only sandbox |
| `MPLCONFIGDIR` | `<data>/chap/cache/matplotlib` | the font cache |

With the sandbox off, `rm -rf ~/.local/share/chap-devops/chap` deletes all of it.

---

## Which image

| `--model-name` | Image | Size on disk |
| --- | --- | --- |
| a GitHub repository, a model directory | `chap-worker` | about 12 GB |
| a chapkit URL, a model id | the image on this machine; else `chap-core` | |
| `builtin:NAME` | the image on this machine; else `chap-core` | |
| no model (`plot-backtest`, `export-metrics`) | `chap-core` | about 2.4 GB |
| anything, in a deployment | `chap-worker` | already there |

- The worker image has uv, R and INLA. chap-core's own worker runs models in
  it.
- `--image core` or `--image worker` overrides the choice.
- `--image core` with a GitHub model gets a warning first: an R model fails
  there with `Rscript: not found`.

---

## Which chap-core version

1. `--tag TAG`, when you give it.
2. In a deployment: **the deployment's tag**, so the CLI and the server are
   the same version.
3. Outside a deployment: **the newest chap-core release**, from GitHub.
4. With `--offline`: the newest image of that kind on this machine.

If GitHub does not answer, chap-devops uses the newest local image, and says so:

```text
warning: the newest chap-core release could not be read; using v2.3.1, the
newest image on this machine
```

---

## The lines before the run

```text
running `chap eval` in ghcr.io/dhis2-chap/chap-worker:v2.5.0; the first run pulls it, about 12 GB
files: chap reads and writes in /home/me/work
model: auto_arima_chapkit at http://auto-arima-chapkit:8000 answers
```

- The image and its tag, and the size when it is not local yet.
- The directories that chap can see.
- The model, when chap-devops checked that it answers.

A run that only asks for help (`chap-devops chap`, `--help`, `--version`) gets
only the first line.

---

## Models: five kinds

| `--model-name` | What happens |
| --- | --- |
| `https://github.com/...` | chap clones it, and runs it in the worker image |
| `./my_model` (a directory) | chap runs it from your directory |
| `http://...` (a chapkit service) | chap-devops checks that it answers, then chap calls it |
| **`auto_arima_chapkit` (an id)** | **chap-devops starts the model, then chap calls it** |
| `builtin:seasonal_median` | chap runs it itself; chap-devops starts nothing |

A word is a model id only when no file or directory of that name is in the
current directory. A directory of the same name wins, as it does for chap.

---

## A model id: chap-devops starts the model

Outside a deployment, chap-devops starts the model in a `chap-devops run` group:

```text
starting auto-arima-chapkit (auto_arima_chapkit in ~/.local/share/chap-devops/run/default)
running `chap eval` in ghcr.io/dhis2-chap/chap-core:v2.5.0
model: auto_arima_chapkit at http://auto-arima-chapkit:8000 answers
chap finished; it wrote arima.nc
auto_arima_chapkit keeps running for the next run; `chap-devops stop auto_arima_chapkit`
stops it, and `--stop` stops it after a run
```

The container joins the group's network, where the model has its service
name. chap-devops gives chap that URL in place of the id.

---

## A model id in a deployment: no `chap-devops up`

```sh
cd ~/mychap && mkdir -p eval && cd eval
chap-devops chap eval --model-name auto_arima_chapkit \
  --dataset-csv laos_subset.csv --output-file arima.nc
```

```text
starting auto-arima-chapkit for this run, without chap-core
```

- chap-devops starts **only that model**, with `--no-deps`: chap-core, postgres and
  redis stay down.
- For this start, the model gets no registration settings, so it does not wait
  for a chap-core that does not run.
- A later `chap-devops up` recreates the model, and it registers as usual.

---

## Keep it running, or stop it

| Situation | After the run |
| --- | --- |
| chap-devops started the model | It keeps running, so the next run starts at once |
| chap-devops started it, with `--stop` | chap-devops stops it |
| the model ran before | chap-devops never stops it |

```text
auto_arima_chapkit ran before this run, so --stop left it running
```

`--timeout SECONDS` (300) is how long chap-devops waits for a model that it
starts. A first start pulls the model's image (1 to 7 GB), and chap-devops shows
that pull.

---

## A chapkit URL: checked before chap starts

chap-devops asks the URL for `/api/v1/info`, from a container on the same network.
If nothing answers, chap is not started:

```text
error: the model server at http://nothing:8000 is not running (no answer on
/api/v1/info), so chap was not started; `nothing` is not a model of this
deployment or group. Give a model id instead (`--model-name
auto_arima_chapkit`) and chap-devops starts it, or start one with `chap-devops run ID`;
the `models:` line lists the URLs that answer
```

For a server elsewhere, the message ends with "check that the server runs and
that this machine can reach it".

---

## Not `localhost`

`chap-devops ps` shows a model at `http://localhost:5001`. That URL works on your
machine. In the container, `localhost` is the container itself:

```text
error: `http://localhost:5001` is this machine, and in the container
`localhost` is the container itself; use `--model-name
http://auto-arima-chapkit:8000`, the same model on the network
```

- When chap-devops knows which model has that port, the message names its
  service URL.
- For a service on this machine, use `http://host.docker.internal:PORT`.
  In the container, that name is this machine.

---

## Models that run in docker

A model with `docker_env` in its `MLproject` starts a container of its own.
chap-devops reads the `MLproject` first (from GitHub, or from your directory):

```text
error: `https://github.com/dhis2-chap/chap_auto_ewars` runs in docker
(`docker_env` in its MLproject), and the container has no docker socket;
run it again with `chap-devops chap --docker ...`
```

```sh
chap-devops chap --docker eval --model-name https://github.com/dhis2-chap/chap_auto_ewars \
  --dataset-csv laos_subset.csv --output-file ewars.nc
```

---

## What `--docker` does

- It mounts `/var/run/docker.sock` in the container. Docker Desktop, Colima
  and OrbStack all answer that path.
- It adds the socket's group to the container user, as the container sees it
  (on Docker Desktop, group 0).
- The run directories are at the same path on both sides, so the paths that
  chap gives docker are real paths on your machine.

```text
warning: --docker gives the container control of docker on this machine
```

The socket is full control of docker. So `--docker` is never the default.

---

## Built-in models

From chap-core v2.5.0, chap has baselines of its own. Give them with the
`builtin:` prefix:

```sh
chap-devops chap eval --model-name builtin:seasonal_median \
  --dataset-csv laos_subset.csv --output-file seasonal_median.nc
```

- The names are `persistence`, `global_median` and `seasonal_median`.
- chap runs them itself, so chap-devops starts no model, and either image works.
- An older tag does not have them, and chap-devops refuses them there.

---

## An MLproject as a chapkit service

From chap-core v2.5.0, `--run-config.as-chapkit` starts a model directory or a
GitHub repository as a chapkit service in the container:

```sh
chap-devops chap eval --model-name ./my_model --run-config.as-chapkit \
  --dataset-csv laos_subset.csv --output-file my_model.nc
```

- The `MLproject` can use `uv_env`, `renv_env` or no environment.
- chap refuses `docker_env`, `conda_env` and `python_env` in this mode. So
  chap-devops does not ask for `--docker`.
- An `MLproject` from `chapkit mlproject export` needs this mode.

---

## Your files: what chap-devops does for you

- **Output directories:** chap-devops makes the directory of each `--output-file`
  that does not exist. chap does not, and it fails with a traceback.
- **Paths outside the directory:** `../out/r.nc`, `/data/r.nc` or
  `--output-file=/data/r.nc`. chap-devops mounts the nearest existing directory
  above the path, at the same path.
- **`--out-file`** (`chap simulate`): chap-devops makes its directory too.
- **The deployment directory:** if you run `chap-devops chap` there, chap-devops tells
  you that a subdirectory (`mkdir eval && cd eval`) keeps chap's files apart
  from the compose files.

---

## What chap-devops reports after the run

```text
chap finished; it wrote arima.nc
`chap-devops chap plot-backtest arima.nc --output-file arima.html` plots it
```

- chap-devops looks at the mounted directories before and after the run, and names
  the files that are new or changed.
- It also names the files that the arguments name, wherever they are.
- For a `.nc` file, it gives the command that plots it.
- When chap fails, chap-devops exits with chap's own status:

```text
error: chap exited with status 1; its own message is above
```

---

## Every message that stops a run

| chap-devops says | What to do |
| --- | --- |
| `--json does not apply` | Drop `--json`: the output is chap's own |
| `` `./x` is not a directory here `` | Give a directory, a GitHub URL or a model id |
| `is not a model of the deployment` | `chap-devops models enable ID`, then run again |
| `there is no marketplace model` | `chap-devops models search WORD` finds the id |
| ``there is no `chap-devops run` group called`` | `chap-devops ps` lists the groups |
| `no directory above it exists to mount` | Make the directory first |
| `--offline needs a ... image on this machine` | Run once without `--offline`, or give `--tag` |
| `` `chap plot-dataset` shows its plot in a browser `` | Use `plot-backtest`, which writes a file |

---

## chap's commands in a container

| Command | In `chap-devops chap` |
| --- | --- |
| `eval` | yes, with every kind of model |
| `plot-backtest` | yes, it writes a file |
| `export-metrics` | yes |
| `validate` | yes |
| `test` | yes |
| `simulate` | yes, from v2.5.0: it writes a CSV with `--out-file` |
| `plot-dataset` | **no**: it opens a browser, and a container has none |

chap-devops refuses `plot-dataset` before the run and names `plot-backtest`.

---

## A full example: three models, compared (1)

Get the data, and check it:

```sh
mkdir -p ~/compare && cd ~/compare
curl -fsSO https://raw.githubusercontent.com/dhis2-chap/chap-core/master/example_data/laos_subset.csv
curl -fsSO https://raw.githubusercontent.com/dhis2-chap/chap-core/master/example_data/laos_subset.geojson
chap-devops chap validate laos_subset.csv
```

```text
Validation passed: no issues found.
```

---

## A full example: three models, compared (2)

```sh
chap-devops chap eval --model-name https://github.com/dhis2-chap/minimalist_example_r \
  --dataset-csv laos_subset.csv --output-file out/r.nc \
  --backtest-params.n-splits 3 --backtest-params.n-periods 3
chap-devops chap eval --model-name auto_arima_chapkit \
  --dataset-csv laos_subset.csv --output-file out/arima.nc \
  --backtest-params.n-splits 3 --backtest-params.n-periods 3
chap-devops chap --stop eval --model-name chapkit_simple_multistep_model \
  --model-configuration-yaml multistep.yaml \
  --dataset-csv laos_subset.csv --output-file out/multistep.nc \
  --backtest-params.n-splits 3 --backtest-params.n-periods 3
```

chap-devops makes `out/` with the first run. The third run stops its model after.
The data has no humidity, so `multistep.yaml` gives Simple Multistep only
`additional_continuous_covariates: [rainfall, mean_temperature]`.

---

## A full example: three models, compared (3)

```sh
chap-devops chap export-metrics out/r.nc out/arima.nc out/multistep.nc \
  --output-file out/comparison.csv
chap-devops chap plot-backtest out/arima.nc --output-file out/arima.html
chap-devops stop auto_arima_chapkit
```

- `comparison.csv` has one row for each model: `crps`, `mae`, `rmse`,
  `coverage_10_90` and more. Lower is better for the errors.
- chap can print `RuntimeWarning` lines about `log1p`. They do not stop the
  run.

---

## Troubleshooting

| You see | Cause and fix |
| --- | --- |
| `Cannot reach the Docker daemon. The model ... requires Docker` | A `docker_env` model ran without the socket. Add `--docker`. |
| `Rscript: not found` | An R model in the `chap-core` image. Drop `--image core`. |
| `FileNotFoundError: [Errno 2]` | An output directory that chap-devops could not make. Make it, then run again. |
| `did not answer on http://...:8000 within 300s` | A slow first start. Give a longer `--timeout`. |
| `docker run exited with status 125` | The image did not pull. Check the tag, or run `chap-devops doctor`. |

Each message is in the book's Troubleshooting chapter, with more detail.

---

## Start now

```sh
curl -fsSL https://raw.githubusercontent.com/dhis2-chap/chap-devops/main/install.sh | sh
chap-devops doctor
chap-devops chap
```

- The full guide, from install to a comparison of two models:
  **https://dhis2-chap.github.io/chap-devops/use-cases/evaluate-with-chap-cli.html**
- The chapter about `chap-devops chap`:
  **https://dhis2-chap.github.io/chap-devops/chap-cli.html**
- A model in the MLproject format: `chap-devops chapkit mlproject run DIR`
  serves it with no build. See the **MLproject models** deck.
