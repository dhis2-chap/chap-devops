# Evaluating a model on your own data

This guide goes from an empty machine to a comparison of two forecasting
models on one dataset. It uses chap-core's own command line, `chap`, through
`chap-devops chap`. You do not install Python, uv or R: `chap-devops chap` runs `chap` in
a container.

At the end you have:

- two evaluations (`.nc` files), one for each model;
- a plot of an evaluation (`.html`) that you open in a browser;
- a table that compares the two models (`.csv`).

All of these files are in your working directory, and they belong to your
user.

## What you need

- Linux or macOS. On Windows, use WSL with the Linux binary.
- Docker, and it must run. On macOS, start Docker Desktop.
- About 20 GB of free disk space, for the images that the steps download:

  | Image | Size on disk | Step |
  | --- | --- | --- |
  | `ghcr.io/dhis2-chap/chap-worker` | about 12 GB | 4, the model from GitHub |
  | `ghcr.io/dhis2-chap/chap-core` | about 2.4 GB | 5 to 7, only if you have no `chap-worker` |
  | `ghcr.io/chap-models/auto_arima_chapkit` | about 6.2 GB | 6, the marketplace model |

  chap-devops says before each run when it must download an image first. chap-devops
  uses the tag of the newest chap-core release, `v2.5.0` when this guide was
  tested. On a slow network, download the images in advance:

  ```sh
  docker pull --platform linux/amd64 ghcr.io/dhis2-chap/chap-worker:v2.5.0
  ```
- A connection to the internet for the first run.

The first run of each step takes some minutes, because Docker downloads the
images. On an Apple silicon Mac, Docker runs the images under emulation, so
each step is slower than on Linux. After the first run, an evaluation of the
example data takes less than a minute.

## Step 1: Install chap-devops

```sh
curl -fsSL https://raw.githubusercontent.com/dhis2-chap/chap-devops/main/install.sh | sh
chap-devops --version
```

It worked when `chap-devops --version` prints a version, for example `chap-devops 0.100.1`.
See [Install](../install.md) for other ways to install.

## Step 2: Check the machine

```sh
chap-devops doctor
```

It worked when the `docker cli`, `docker daemon` and `docker compose` lines
start with `ok`. A `warn` on the `os and arch` line of an Apple silicon Mac is
correct: it only tells you that the images run under emulation. If a line
starts with `fail`, do the fix that it names, then run `chap-devops doctor` again.

## Step 3: Make a working directory and get data

Make a directory for this work, and go into it:

```sh
mkdir -p ~/chap-eval
cd ~/chap-eval
```

Download chap-core's example data. It is monthly dengue cases, with rainfall,
temperature and population, for districts of Laos:

```sh
curl -fsSO https://raw.githubusercontent.com/dhis2-chap/chap-core/master/example_data/laos_subset.csv
curl -fsSO https://raw.githubusercontent.com/dhis2-chap/chap-core/master/example_data/laos_subset.geojson
```

It worked when `ls` shows `laos_subset.csv` and `laos_subset.geojson`.

**To use your own data instead**, put your CSV and its GeoJSON in this
directory. They must have the same name, for example `mydata.csv` and
`mydata.geojson`. Use the same columns as the example file:

```text
time_period,rainfall,mean_temperature,disease_cases,population,location
2010-01,37.965,20.04,1.0,75049.56,Bokeo
```

Each `location` must be a feature in the GeoJSON. In the steps below, change
`laos_subset.csv` to the name of your CSV.

## Step 4: Evaluate a model from GitHub

This model is a small R model from chap-core's examples:

```sh
chap-devops chap eval \
  --model-name https://github.com/dhis2-chap/minimalist_example_r \
  --dataset-csv laos_subset.csv \
  --output-file minimalist_r.nc \
  --backtest-params.n-splits 3 \
  --backtest-params.n-periods 3
```

What the options do:

| Option | What it does |
| --- | --- |
| `--model-name` | The model: a GitHub repository, a model directory, or the URL of a model service. |
| `--dataset-csv` | Your data. The GeoJSON with the same name is found automatically. |
| `--output-file` | The evaluation that chap writes, a NetCDF (`.nc`) file. |
| `--backtest-params.n-splits` | How many times the model trains and forecasts, each time from a later point in the data. |
| `--backtest-params.n-periods` | How many periods (here months) each forecast goes ahead. |

chap-devops writes some lines of its own before chap starts:

```text
running `chap eval` in ghcr.io/dhis2-chap/chap-worker:v2.5.0; the first run pulls it, about 12 GB
files: chap reads and writes in /home/me/chap-eval
```

Then chap writes its own log lines. At the end, chap-devops writes:

```text
chap finished; it wrote minimalist_r.nc
```

It worked when you see `chap finished; it wrote minimalist_r.nc`.

chap-devops selected the large `chap-worker` image because a model from GitHub can
need R. chap clones the model again for each run. chap-devops keeps the R packages
of a model in its cache, so the second run does not install them again. This
small model has no packages to install, so each run takes about the same
time: about 40 seconds on an Apple silicon Mac, when the image is there.

## Step 5: Plot the evaluation

Run the command that the last line of step 4 gave you:

```sh
chap-devops chap plot-backtest minimalist_r.nc --output-file minimalist_r.html
```

It worked when chap-devops writes `chap finished; it wrote minimalist_r.html`. Open
the file in a browser:

```sh
open minimalist_r.html        # macOS
xdg-open minimalist_r.html    # Linux
```

The plot shows the real cases and the forecasts of each split, one panel for
each location.

## Step 6: Evaluate a second model, from the marketplace

A model from the [marketplace](../models.md) is a model service: a container
with an HTTP API. Give its id to `--model-name`, and chap-devops starts it for you:

```sh
chap-devops chap eval \
  --model-name auto_arima_chapkit \
  --dataset-csv laos_subset.csv \
  --output-file auto_arima.nc \
  --backtest-params.n-splits 3 \
  --backtest-params.n-periods 3
```

chap-devops starts the model, waits until it answers, and then runs chap. After
the first line, Docker Compose writes the lines of the network, the volume
and the containers that it makes:

```text
starting auto-arima-chapkit (auto_arima_chapkit in /home/me/.local/share/chap-devops/run/default)
...
running `chap eval` in ghcr.io/dhis2-chap/chap-worker:v2.5.0
files: chap reads and writes in /home/me/chap-eval
model: auto_arima_chapkit at http://auto-arima-chapkit:8000 answers
...
chap finished; it wrote auto_arima.nc
auto_arima_chapkit keeps running for the next run; `chap-devops stop auto_arima_chapkit` stops it, and `--stop` stops it after a run
```

It worked when you see `chap finished; it wrote auto_arima.nc`. The first
time, Docker downloads the image of the model, which takes some minutes.

chap only talks HTTP to a model service, so the smaller `chap-core` image is
enough. chap-devops uses an image that is already on this machine, so the run
downloads no chap image. After step 4, that is `chap-worker`. If the
`chap-core` image at the same tag is also on this machine, chap-devops uses
`chap-core`, and the first line names it.

The model keeps running, so a second evaluation of it starts at once.
`chap-devops ps` lists it. Find more ids with `chap-devops models list`.

## Step 7: Compare the two models

```sh
chap-devops chap export-metrics minimalist_r.nc auto_arima.nc --output-file comparison.csv
```

It worked when chap-devops writes `chap finished; it wrote comparison.csv`. chap can
write some `RuntimeWarning` lines about `log1p` before that. They do not stop
the run.

`comparison.csv` has one row for each model. Open it in a spreadsheet. Some
of the columns:

| Column | What it measures | Better when |
| --- | --- | --- |
| `crps` | How far the forecast distribution is from the real cases | lower |
| `mae` | The mean absolute error of the forecasts | lower |
| `rmse` | The root mean square error of the forecasts | lower |
| `coverage_10_90` | How often the real value is in the 10-90% forecast interval | near 0.8 |

## Step 8: Clean up

Stop the model service, and remove its data:

```sh
chap-devops stop auto_arima_chapkit --purge
```

It worked when chap-devops writes:

```text
stopped auto_arima_chapkit
removed volume default-662918_ck_auto_arima_chapkit_data
removed group default
```

The number in the volume name is different on your machine. The group
`default` held only this model, so chap-devops removes it too.

Your `.nc`, `.html` and `.csv` files stay in `~/chap-eval`. To get the disk
space of the caches back, delete chap-devops's chap directory:

```sh
rm -rf ~/.local/share/chap-devops/chap
```

To remove the images too, list them, then remove each one with
`docker image rm`:

```sh
docker image ls 'ghcr.io/dhis2-chap/*'
docker image ls 'ghcr.io/chap-models/*'
docker image rm ghcr.io/dhis2-chap/chap-worker:v2.5.0
```

## If something goes wrong

| chap-devops writes | What to do |
| --- | --- |
| `` `http://localhost:...` is this machine `` | Give the model id instead, as in step 6. |
| `the model server at ... is not running` | The URL does not answer. Give a model id, and chap-devops starts the model. |
| `is not a model of the deployment` | In a deployment, add the model first with `chap-devops models enable ID`. |
| ``runs in docker (`docker_env` in its MLproject)`` | The model starts a container of its own. Run the same command with `chap-devops chap --docker ...`. See [Models that run in docker](../chap-cli.md#models-that-run-in-docker). |
| `FileNotFoundError: [Errno 2] No such file or directory` | chap cannot find an input file. Make sure that the CSV and the GeoJSON with the same name are there, then run again. |
| `Rscript: not found` | The model needs R. Add `--image worker` after `chap`. |
| `chap exited with status N` | chap stopped with an error. The cause is in chap's own lines above. |
| `docker could not start ...` | Docker could not download or start the image. Run `chap-devops doctor`. |

More messages are in [Troubleshooting](../troubleshooting.md).

## Next

- To evaluate the models of a [deployment](../concepts.md), run `chap-devops chap`
  in a subdirectory of it (`mkdir eval && cd eval`), and give the model id:
  `chap-devops chap eval --model-name auto_arima_chapkit ...`. The deployment does
  not need to run: chap-devops starts only that model, without chap-core.
- To use another chap-core version, add `--tag`, for example
  `chap-devops chap --tag master eval ...`.
- For chap's own options, run `chap-devops chap eval --help`.
- [The chap CLI](../chap-cli.md) tells how `chap-devops chap` mounts your files,
  selects the image and keeps its caches.
