# Timing chap-core: chap-devops bench

`chap-devops bench` times the requests that the Modeling App sends to chap-core
when a person opens one evaluation. Use it to find which page is slow, and to
see if it gets slower as the data grows.

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

The times are the time until the whole answer came, as the browser sees it.
A failed request counts with the time it took, because a person waited that
long. Each failed request adds a warning with what chap-core said. `--json`
gives every row with `min_ms`, `p50_ms`, `p95_ms`, `max_ms`, `bytes` and the
count of each HTTP status.

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
