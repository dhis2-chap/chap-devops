# Timing chap-core: chap-devops bench

`chap-devops bench` puts load on chap-core in two ways:

- With no `--write`, it times the requests that the Modeling App sends to
  chap-core when a person opens one evaluation. Use it to find which page is
  slow, and to see if it gets slower as the data grows. It changes nothing.
- With `--write`, it imports a dataset and runs several backtests on it at
  the same time, then checks what each one stored. See
  [Backtests under load](#backtests-under-load---write).

`chap-devops bench dataset` makes the synthetic datasets for both. See
[Synthetic datasets](#synthetic-datasets-chap-devops-bench-dataset).

```sh
chap-devops bench                 # the newest backtest, 3 times each, 1 at a time
chap-devops bench 12 --repeat 10  # backtest 12
chap-devops bench --parallel 8    # 8 requests open at the same time
```

## What it sends

First it reads the backtest (`/v1/crud/backtests/{id}/info`) and the list of
plots, and it does not time these reads. Then it sends each request below
`--repeat` times:

- the lists: `backtests`, `configured-models`, `datasets`, `jobs`;
- `backtest info` and `backtest-specification`;
- `evaluation-entry` and `actualCases`, for the whole country (`adm0`) and
  for the first 10 org units, as the chart page asks for them;
- `compatible-backtests`, the `dataset` of the backtest, and `metrics`;
- for each plot, `facet-coords` and the first `subplot`.

The requests go round by round, so with `--parallel 1` one request is never
sent two times in a row.

## The report

This example is from a `bench` before the `GZIP` column:

```text
REQUEST                                  N  FAILED  MIN     P50     P95     MAX     SIZE
backtests                                5  0       8 ms    10 ms   11 ms   11 ms   3.9 kB
evaluation-entry 10 org units            5  0       65 ms   68 ms   73 ms   73 ms   114.4 kB
dataset                                  5  0       125 ms  232 ms  245 ms  245 ms  774.8 kB
predicted_vs_actual subplot              5  0       668 ms  674 ms  748 ms  748 ms  38.2 kB
...
timed 25 requests of backtest 4, 5 times each, 1 at a time, in 16.12 s
slowest: predicted_vs_actual subplot, p50 674 ms
```

The times are the time until the whole answer came and was unpacked.
A failed request counts with the time it took, because a person waited that
long. Each failed request adds a warning with what chap-core said. `--json`
gives every row with `min_ms`, `p50_ms`, `p95_ms`, `max_ms`, `bytes`,
`wire_bytes` and the count of each HTTP status.

## gzip

`bench` asks chap-core for gzip, as the DHIS2 route asks for it. The route
unpacks the answer, so the browser gets plain JSON. But chap-core packs every
answer that goes to the route, and the times include that work. On a large
answer, the time to pack can be more than the time to make the answer.

- `SIZE` (`bytes` in `--json`) is the unpacked answer, which the browser
  gets.
- `GZIP` (`wire_bytes`) is the size on the network between chap-core and the
  DHIS2 route. It is the same as `SIZE` if chap-core did not pack the answer.
  chap-core does not pack answers under 1,000 bytes.

All versions of `bench` ask for gzip. But to compare two chap-core versions,
run the same `bench` version on both.

## One at a time and many at a time

A Modeling App page sends several requests at the same time. `--parallel 8`
does the same. If the server answers one request at a time, the fast requests
wait for the slow ones. Then their p50 goes up, but the total time does not go
down. For the evaluation above (18 org units, 72 months), on chap-core
`master` of 2026-11-05:

| | `--parallel 1` | `--parallel 8` |
| --- | --- | --- |
| total for 125 requests | 16.1 s | 14.9 s |
| `backtests` p50 | 10 ms | 737 ms |
| `configured-models` p50 | 12 ms | 748 ms |
| `predicted_vs_actual subplot` p50 | 674 ms | 2.67 s |

## Another chap-core

`--url` times a chap-core that this deployment does not run. The token comes
from `CHAP_API_TOKEN` in the shell, as for `chap-devops api`:

```sh
CHAP_API_TOKEN=... chap-devops bench --url https://chap.example.org --repeat 5
```

Run it while a backtest runs on the worker to see how much a running job
slows the pages. Use [`chap-devops top --record`](./top.md#recording-cpu-and-memory)
at the same time to see which container is busy.

## Backtests under load: --write

`--write` adds rows to chap-core's database. Do not use it on a deployment
that people use: run it on a deployment of its own.

```sh
chap-devops bench --write --model seasonal_median
chap-devops bench --write --model seasonal_median --backtests 8 --locations 300 --periods 156
chap-devops bench --write --model 12 --dataset 5   # an existing dataset
```

The steps:

1. It makes a synthetic dataset from the [dataset options](#synthetic-datasets-chap-devops-bench-dataset)
   and imports it with `make-dataset`. With `--dataset ID`, it uses that
   dataset instead.
2. It starts `--backtests` backtests (default 4) of the configured model
   `--model`, by name or id, with `create-backtest`. It starts all of them
   before it polls any of them.
3. It polls the jobs until each one has ended, for at most `--wait` seconds.
4. For each backtest that ended, it checks what chap-core stored.
5. It deletes the dataset it made and the backtests, including the
   backtests of chap-core's built-in baselines. `--keep` keeps them. It
   never deletes a dataset that `--dataset` named.

`--model` takes the name that `chap-devops api GET /v1/crud/configured-models`
shows, for example `seasonal_median` or `chapkit_ewars_model:default`. Without
it, the error lists the names.

A backtest passes when its job ended with `SUCCESS`, and chap-core stored one
forecast for each org unit, split and forecast period, and stored aggregate
metrics. The count of forecasts is the length of `evaluation-entry` at one
quantile. The expected count comes from the backtest row: `orgUnits` times
`splitPeriods` times `nPeriods`. Anything else is a failure:

- the job ended with `FAILURE`, or it still ran after `--wait` seconds;
- chap-core stored fewer or more forecasts than expected, or none;
- chap-core stored no aggregate metrics.

```text
#  STATUS   TIME     BACKTEST  FORECASTS    METRICS  RESULT
1  SUCCESS  83.50 s  26        25200/25200  13       ok
2  SUCCESS  86.65 s  23        25200/25200  13       ok
...
8 of 8 backtests of seasonal_median on dataset 4 passed, in 143.33 s
deleted dataset 4 and the 8 backtests the run made; --keep keeps them
```

`TIME` is from the start of the backtest to the poll that saw it end, so it
is exact to about 3 s. If a backtest fails, the command exits with status 1
and gives a warning with the reason from the job log. `--json` gives each
backtest with its `status`, `seconds`, `backtest_id`, `forecasts`,
`expected_forecasts`, `metrics` and `problem`.

To compare settings, run the same command before and after the change, for
example with one worker process and then with four. Use the same seed (the
default) to get the same data. If jobs are still running after `--wait`,
chap-devops keeps the rows and says so: `chap-devops jobs` shows them.

## Synthetic datasets: chap-devops bench dataset

`chap-devops bench dataset` makes a dataset with random values. The same
options always give the same dataset.

```sh
chap-devops bench dataset                        # 18 locations, 104 weeks, in .
chap-devops bench dataset --out data --locations 2000 --periods 520 --covariates 10
chap-devops bench dataset --import --period-type month --periods 72
```

| Option | Default | What it sets |
| --- | --- | --- |
| `--locations N` | 18 | the number of org units, `loc01` to `loc18` |
| `--periods N` | 104 | the number of periods for each location |
| `--period-type` | `week` | `week` (ISO weeks) or `month` |
| `--cases N` | 100 | the typical number of cases in one location and period |
| `--covariates N` | 3 | `rainfall`, `mean_temperature`, `population`, then `covariate_4` and more |
| `--seed N` | 1 | the start of the random values |

The periods always end with the last week or month of 2024. Each location
has its own size, season and population. The cases follow the season and the
rainfall of the period before, with noise, so a model has something to
learn. The values are not realistic.

Without `--import`, it writes `<name>.csv` and `<name>.geojson` to `--out`
(default `.`). The CSV has the columns that `chap eval` reads, and `chap eval`
finds the GeoJSON of the same name next to it:

```sh
chap-devops bench dataset --out data
chap-devops chap eval --model-name auto_arima_chapkit \
  --dataset-csv data/synthetic-18x104w-c100-k3-s1.csv --output-file eval.nc
```

The GeoJSON has one square for each location, on a grid in the sea west of
Africa, so no one mistakes it for a real place.

With `--import`, it sends the dataset to chap-core's `make-dataset`, waits
until chap-core has stored it, and gives the dataset id. It sends the
observations as columns (`providedDataColumns`), which needs chap-core v2.7.0
or later. `--name` sets the name. The default name says what the dataset is
made of: `synthetic-18x104w-c100-k3-s1` is 18 locations, 104 weeks, 100
cases, 3 covariates and seed 1. `bench --write` adds `-bench-<time>` to it.
