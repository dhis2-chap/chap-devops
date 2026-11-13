# Bugs found while testing chap-devops

Bugs in the systems that chap-devops deploys, found by running them for real.
Bugs in chap-devops itself are fixed in the same session and are not listed.
Each entry names where it was seen, what happens, and what would fix it. A
bug that is fixed leaves the file.

## Modeling App

### A new model on a copied evaluation clears the data mapping

- Seen: Modeling App 7.3.2, DHIS2 2.42.7.1, chap-core `master` (2.6.0.dev0,
  `sha256:ccbfaada86a0`), 2026-10-31.
- On a copy from **Create new based on...**, **Select model** with
  `Simple Multistep Model (chapkit) [Monthly selfhistory]` shows
  `0/1 data items mapped` and `Please map all model covariates to valid data
  items`. **Start import** then starts no job.
- The copy came from `[Monthly climate]`, which has the same target, so the
  mapping of `Disease cases` could stay. The message also names covariates, and
  this model has none: the item it needs is the target.
- Fix: keep the mapping of each input that the new model also has, and name the
  target in the message.

### An evaluation of about 570 org units or more shows `Unable to load data`

- Seen: Modeling App 7.4.0, DHIS2 2.43.2.1, chap-core `master`, 2026-11-06,
  an evaluation of 2000 synthetic org units.
- **Evaluation details** shows `Unable to load data` and `There was a problem
  loading required data`. The app asks DHIS2 for the names of all the org
  units in one GET:
  `/api/43/organisationUnits?paging=false&fields=id,displayName,path,level&filter=id%3Ain%3A%5B...%5D`.
  DHIS2 (Tomcat) answers `HTTP Status 400 – Bad Request` when the request
  line is longer than about 8 kB: with 7-character ids, 800 passed and 1000
  failed.
- A DHIS2 id has 11 characters, and the app encodes each comma as `%2C`, so
  the limit is at about 570 org units. The 500-location evaluation loaded.
- Fix: ask for the org units in pages of about 200 ids, or filter by the
  parent and level instead of by id.

## chap-core (`master`)

### Plot requests grow with the locations, and from 500 they kill the API worker

- Seen: chap-core `master` `sha256:d1c31a63198239f1f44037fc0e9b`, worker
  `sha256:69c5363b7d14`, 2026-11-06, synthetic monthly data, 60
  months, 3 covariates, 7 splits, Simple Multistep, `chap-devops bench
  --repeat 3`. One request at a time, p50 in seconds:

  | Locations | `predicted_vs_actual subplot` | `sample_bias` `facet-coords` | `evaluation-entry adm0` | `dataset` |
  | --- | --- | --- | --- | --- |
  | 18 | 0.67 | 0.18 | 0.03 | 0.06 |
  | 100 | 3.60 | 1.08 | 0.12 | 0.49 |
  | 500 | 17.6 | 5.38 | 0.58 | 2.59 |
  | 2000 | 70.5 | 21.9 | 2.27 | 9.62 |

- At 2000 locations one `predicted_vs_actual` subplot is 3.7 MB, and
  `dataset` is 41.7 MB. More periods (240), larger counts (1e6) and more
  covariates (10) change these times by less than 1.5 times.
- A DHIS2 route has `responseTimeoutSeconds: 30`. At about 35 ms per location,
  the Modeling App likely cannot load the `predicted_vs_actual` plots from
  about 900 locations (not tested through DHIS2).
- With 8 requests at a time, 9 requests failed at 500 locations and 24 at
  2000. The API log has `WORKER TIMEOUT (pid:2422)` and `Worker (pid:2422) was
  sent SIGABRT!`: an `async def` handler (`get_evaluation_entries`,
  `get_actual_cases`, `get_dataset`) ran blocking database and pandas work for
  more than 30 s, gunicorn killed the only worker, and every open request
  failed with it.
- Fix: make the blocking handlers plain `def`, so FastAPI runs them in its
  thread pool; compute the plot data once per backtest and store it; send the
  Modeling App the data for one panel, not the whole set.

### Deleting a large backtest or dataset runs past the worker timeout and deletes nothing

- Seen: chap-core v2.7.0 (`ghcr.io/dhis2-chap/chap-core:latest` on
  2026-11-10), a deployment of its own, with `chap-devops bench --write
  --model seasonal_median --locations 300 --periods 156 --backtests 8`.
- The 8 backtests and the 4 backtests of the baselines had 25,200 forecasts
  each, 302,400 rows in all. `DELETE /v1/crud/backtests?ids=22,...,33`
  stopped with `WORKER TIMEOUT (pid:26)` and `Worker (pid:26) was sent
  SIGABRT!` in the log, the client got `Peer disconnected`, and every
  backtest was still there. The dataset could then not be deleted (409).
  While the delete ran, the one API worker answered nothing else.
- The dataset of that run (300 locations, 156 weeks, 4 features: 187,200
  observations) does the same: `DELETE /v1/crud/datasets/4` ended with
  `WORKER TIMEOUT (pid:574)`, and the dataset is still there.
  `DataSet.observations` also has `cascade_delete=True`.
- `delete_backtest` and `delete_backtest_batch` (`rest_api/v1/routers/crud.py`)
  call `session.delete(backtest)`, and the dataset delete does the same with
  its dataset. `Backtest.forecasts` has
  `cascade_delete=True`, so SQLAlchemy loads every forecast row into memory
  and deletes them one at a time. Both handlers are `async def`, so the work
  also blocks the event loop.
- Fix: delete the children with one statement each (`delete(BacktestForecast)
  .where(BacktestForecast.backtest_id.in_(ids))`, the same for the metrics),
  and `delete(Observation).where(Observation.dataset_id == id)` for a
  dataset; or set `passive_deletes=True` with `ON DELETE CASCADE` on the
  foreign keys;
  and make the handlers plain `def`. chap-devops now deletes one backtest per
  request, which keeps each request smaller, but one backtest of this size
  still takes a long time.

### `facet-coords` of the sample bias plots takes 0.2 to 22 s to answer `{}`

- Seen: as above. `sample_bias_by_horizon` and `sample_bias_by_time_period`
  `facet-coords` answer 2 bytes (`{}`) after 186 to 294 ms locally, 1.2 to
  1.8 s on the shared server (v2.6.0), and 21.9 s at 2000 locations.
- The handler builds the whole flat evaluation to find that the plot has no
  facets.
- Fix: return the empty facets of a plot without facet dimensions before the
  data is loaded.

### A job stays `STARTED` for ever when the worker restarts under it

- Seen: chap-core `master` as above, 2026-11-06. A `create_backtest_from_data`
  job (18 org units, Simple Multistep) ran when `docker stop` stopped the worker
  (`Warm shutdown`, then killed after 10 s); the worker was back 30 s later.
- 14 minutes later, `GET /v1/jobs` still says `STARTED`. The worker log has
  no line about the task after its restart: it is not delivered again and not
  marked failed. The Modeling App shows the evaluation as running.
- A worker restart is normal: an update, a deploy, the out-of-memory killer.
- Fix: set `task_acks_late` and `task_reject_on_worker_lost` so the task goes
  back to the queue, or mark the `STARTED` jobs of a dead worker as failed when
  a worker starts.

### A model that stops answering fails the job with a raw `httpx` traceback

- Seen: as above, `chapkit_simple_multistep_model` 0.1.5, the same backtest.
  | What happened to the model | Job | After |
  | --- | --- | --- |
  | `docker kill` | `FAILURE` | 10 s |
  | `docker restart` (back in 2 s) | `FAILURE` | 10 s |
  | `docker network disconnect` | `FAILURE` | 5 min 12 s |
  | `docker pause` (accepts, never answers) | `FAILURE` | 25 min 22 s |
- The job log in each case is a Python traceback that ends in
  `httpx.ConnectError: [Errno -2] Name or service not known` (for the pause:
  `httpx.ReadTimeout: timed out`). It does not name the model or the step
  (train, predict, which split).
- A model that hangs holds one worker process for 25 minutes: each poll of
  its job waits the 300 s read timeout (`Polling job ... failed (4/5),
  retrying: timed out`), and the job fails after the fifth.
- A restart of the model in the middle of a backtest loses the whole backtest,
  with no retry, although the model answers again 2 s later.
- Fix: catch the transport errors in `chapkit_rest_api_wrapper.py` and fail
  with one sentence that names the model, its URL and the step; retry a
  connection error a few times before the job fails.

### Each run of a worker-side model keeps its own 0.9 GB environment

- Seen: a shared server, chap-core master worker, 2026-11-06. `/data/runs` holds
  24 GB in 21 run directories (stable: 10 GB). `mstl_multistep_model` has 13
  runs and 12 GB. One run directory has a `.venv` of 798 MB, copied rather
  than linked from `UV_CACHE_DIR=/data/uv` (11 GB). The disk is 73% full.
- Locally, the first run of `auto_regressive_weekly` downloaded numpy, scipy,
  pandas, tensorstore, google-api-python-client and more before it trained.
- Nothing removes a run directory after the job.
- Fix: build one environment per model commit and use it for every run, or
  use `UV_LINK_MODE=hardlink` with the cache on the same file system; remove
  a run directory when its job ends.

### The worker starts one process for each CPU it sees

- Seen: chap-core `master`, 2026-11-09, `compose.yml`, `compose.ghcr.yml` and
  the Helm chart (`charts/chap/values.yaml`).
- The worker starts with `celery -A chap_core.rest_api.celery_tasks worker
  --loglevel=info`, with no `--concurrency` and no setting for it. Celery then
  starts one child process for each CPU. In a container this is usually the
  number of CPUs of the host, not the CPU limit of the container.
- Each job loads its own copy of the dataset. At 2000 locations the worker
  used 9.1 GiB and 1,706% CPU with a backtest and 3 baselines at the same time.
- Fix: read the concurrency from one setting in the compose files and the
  Helm chart, with a low default, and document how to size it by the memory
  of one job.

### A facility-scale import as rows holds the API worker for 28 of its 30 s

- Seen: chap-core `master` (2.8.0.dev0, `94e16246`), built natively for
  arm64 with `chap-devops init --source`, 2026-11-13; and chap-core v2.7.0
  (`62bae69c`), amd64 under emulation on Apple Silicon.
- `chap-devops bench dataset --import --locations 1500 --periods 365
  --covariates 4` sends `make-dataset` with `providedData`: one object for
  each value, 2,737,500 objects, 227 MB. This is how the Modeling App sends a
  dataset.
- Native `master`: the request answered after 28.3 s, and the job took
  76.4 s more. gunicorn stops an API worker that does not answer it for
  30 s, so this is 2 s from a failure.
- v2.7.0 under emulation: the API log has `[CRITICAL] WORKER TIMEOUT
  (pid:25)` and `Worker (pid:25) was sent SIGABRT!`, and the client gets
  `Peer disconnected` after 19 s. The same data as `providedDataColumns`
  (`--format columns`) was stored in 92.9 s: request 12.6 s, job 80.3 s.
- Cause (found in the chap-core session): FastAPI parses and checks the
  body on the event loop, and `get_job_request` parses it a second time.
  The uvicorn worker sends gunicorn a heartbeat only while the loop runs,
  so a loop that is blocked long enough gets the worker killed.
- `chap-devops bench --write --predict` sends the prediction run
  (`POST /v1/crud/prediction-setups/{id}/run`) as rows too, so it has the
  same risk at this size.
- Fix: parse and check large bodies off the event loop, and parse them once.
  CLIM-1889.

### A prediction refuses two whole years of weekly data

- Seen: chap-core v2.7.0 (revision `62bae69c`), 2026-11-13, a synthetic
  dataset of 18 org units and 104 weeks (2023W1 to 2024W52), model
  `seasonal_median`. The code is the same on `master` (`94e16246`).
- A backtest on the dataset passes. A prediction from that backtest, through
  `POST /v1/crud/prediction-setups/{id}/run` with the same values, fails:
  `ValueError: Training data must cover at least two whole years`.
- `validate_training_data` in `chap_core/validators.py` compares timestamps:
  `end_timestamp < start_timestamp + 2 * delta_year`. 104 ISO weeks end on
  2024-12-29, 4 days before 2025-01-02, so two whole ISO years fail. 105
  weeks pass.
- Fix: compare whole periods (104 weeks or 24 months) instead of timestamps,
  and say in the message how many periods the data has and how many it needs.
  CLIM-1887.

## DHIS2 routes (hosted DHIS2 2.43.2)

### One route request in ten waits 30 s and ends in an empty 504

- Seen: a hosted DHIS2 2.43.2 (`13927ae`) behind a KEDA HTTP interceptor,
  Modeling App 7.4.0, route `chap` over HTTPS to a chap-core master on another
  server, `responseTimeoutSeconds: 30`, 2026-11-06.
- From a logged-in browser, one request at a time:
  - 40 plain DHIS2 API requests (`/api/me`, `/api/dataElements`, ...): 0 failed.
  - 40 route requests (`configured-models`, `jobs`, `backtests`,
    `/system/info`, `datasets`): 4 failed, each a `504` with an empty body
    after 30.07 to 30.30 s. Other runs: 3 of 60, 2 of 20.
- The proxy log in front of chap-core has every one of these requests, answered
  `200` by chap master in 2 to 75 ms. The 504 has the header
  `x-keda-http-cold-start: false`. So the answer is lost after chap sends it,
  between DHIS2 and the browser.
- The same test elsewhere, 60 route requests each, 0 failed:
  - a local DHIS2 2.42.7.1, route to `http://chap:8000` (max 79 ms);
  - a local DHIS2 2.43.2.1, route to `http://host.docker.internal:8710`
    (max 175 ms);
  - the same local 2.43.2.1, route to
    the same HTTPS URL that the hosted DHIS2 uses (max 285 ms; no token, so
    48 answers were 401 and 12 were 200);
  - the same local 2.43.2.1, with `auth` of type `api-token` like the hosted
    route: to the local chap-core (60 times 200, max 673 ms), to the same
    HTTPS URL with a dummy token (max 172 ms), and to an HTTPS test server
    that sends 20 and 32 kB bodies (24 answers, max 488 ms);
  - the same with `auth` of type `api-headers` and a Bearer header (max 111 ms).
- So the DHIS2 version, the `api-token` auth, HTTPS and the body size do not
  cause it alone, and the chap-core server answered every lost request. Not
  tested: the real token with real chap answers from outside the hosting
  platform. The likely place is the hosting platform: on the way out of the
  DHIS2 pod, or in the KEDA interceptor in front of it.
- For a person in the Modeling App, a page that sends 8 to 25 requests likely
  waits 30 s for one panel, which then shows an error.
- Fix: likely for the hosting platform. Look at the egress of the DHIS2 pod (NAT or
  connection tracking that drops a reused connection) and at the KEDA HTTP
  interceptor timeouts.

## DHIS2 demo database (Laos, 2.42)

### The CHAP output elements are weekly, the forecasts monthly

- The `CHAP Dengue Cases (Any) - Weekly Quantile ...` and `... Outbreak Alert`
  elements belong to a weekly data set. A monthly forecast imports into them
  (DHIS2 2.42.6 accepts the values), but a monthly forecast has no monthly
  elements to go to.
- Fix: add monthly CHAP output elements to the demo database.

## Open Climate Service (`main`)

### A failed read from the CHIRPS3 server is a 500, with no retry

- `POST /ingestions` answered 500 (`RasterioIOError: Read failed`,
  `TIFFFillTile ... got 0 bytes`) once; the same request passed 4 seconds
  later.
- Seen again 2026-10-30, `sha256:9f454cbca239`: 1 run of 15 answered 500 with
  `'/vsicurl/https://data.chc.ucsb.edu/.../chirps-v3.0.rnl.2024.01.03.cog' not
  recognized as being in a supported file format`.
- Seen again 2026-11-03, `sha256:37d445d46b4f`, with chap-core v2.6.0: the
  e2e ingestion answered 500 with `TIFFFillTile:Read error ... got 0 bytes`.
- Fix: retry a failed read of a remote tile before the request fails.

## chapkit (`main`)

### A deployment cannot set how many jobs a model service runs at the same time

- Seen: chapkit `main`, 2026-11-09, all 42 repositories in chap-models.
- The scheduler limit comes from `max_concurrency` in the code of the model
  (`src/chapkit/api/service_builder.py`). The default is 1, "to avoid SQLite
  write lock issues". No model in chap-models sets it, and no environment
  variable changes it.
- So a benchmark that sends one model several backtests runs them one after
  the other on every server. A change needs a new release of the model.
- Fix: read the default from an environment variable, keep 1 when it is not
  set, and let a value in the code of the model win.

## chap-models

### Models set their thread count in different ways, often in code

- Seen: the default branch of all 42 repositories in chap-models, 2026-11-09.

  | Model | Thread count |
  | --- | --- |
  | `mstl_arima` | `StatsForecast(..., n_jobs=1)` in `chap_mstl_arima/model.py`: always one core |
  | `rwanda_random_forest` | `n_jobs=-1` in `train.py`: every job takes every core |
  | `epidemiar_example_model` | `detectCores()` in `settings.R`: the cores of the host |
  | `chapkit_simple_multistep_model` | not set, so one core |
  | the INLA models | `num.threads` not set, so INLA decides |
  | `chapkit_ghr_model` | config option `nthreads`, default 4 |
  | `chtorch` | config option `num_workers`, default 0 |

- R `detectCores()` and Python `os.cpu_count()` count the CPUs of the host,
  not the CPU limit of the container.
- On a shared server, models that take every core compete with each other and
  with the worker. Models that use one core cannot use a large server.
- Fix: one convention for all models. The deployment sets the thread count in
  an environment variable, a config option can change it, and the default is
  1. Put it in the model templates and the docs for model developers.

### `auto_regressive_weekly` needs `population`, but does not say so

- Seen: configured model `auto_regressive_weekly` (weekly_ar_model
  `ede034ac`), chap-core `master` as above, 2026-11-06.
- `GET /v1/crud/configured-models` lists no required covariates for it, so
  chap-core takes a dataset without `population`. The job then sets up its
  environment, trains, and fails after 5 min 38 s with `KeyError:
  'population'` (100 locations, 520 weeks).
- Fix: declare `population` as a required covariate of the model, so chap-core
  and the Modeling App refuse the data at once.

### `chapkit_simple_multistep_model` trains on one core and needs 12 GiB at 2000 locations

- Seen: v0.1.5, chap-core `master` as above, 2026-11-06, synthetic monthly
  data with 3 covariates, 7 splits, `top --record` every 3 s.
- `RandomForestRegressor` is built without `n_jobs` (`train.py:29`), so
  training uses one core (100% CPU) on a machine with 12. chapkit also runs
  one job at a time (`max_concurrency` 1), so a benchmark that sends this
  model several backtests runs them one after the other.
- Memory of the model container went to 12.4 GiB at 2000 locations, 60
  months and 3 covariates.
- Fix: a thread count from the convention in "Models set their thread count
  in different ways", and a `max_concurrency` that fits the memory of the
  model.

### `chapkit_simple_multistep_model` forecasts negative case counts

- Seen: v0.1.4 (`sha-82dbe6a`), `[Monthly climate]`, a prediction for
  202501 to 202503 on the Laos demo database, 2026-10-31.
- After **Import**, DHIS2 holds negative values: `Quantile Low` down to -27.67
  (52 values) and `Quantile Mid Low` down to -13.12 (37 values). The chart of
  01 Vientiane Capital goes down to -7.60.
- Fix: the model clips its quantiles at 0, or chap-core does when it stores
  them.

- IMPA-TECH-SARIMA fails in chap itself (`NoPredictionsError`).
- rwanda_random_forest fails at train when served with `mlproject run`.
- chap_pymc needs the packages of its `uv_env`, which a `-cli` image does not
  have.
