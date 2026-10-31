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

## DHIS2 demo database (Laos, 2.42)

### The CHAP output elements are weekly, the forecasts monthly

- The `CHAP Dengue Cases (Any) - Weekly Quantile ...` and `... Outbreak Alert`
  elements belong to a weekly data set. A monthly forecast imports into them
  (DHIS2 2.42.6 accepts the values), but a monthly forecast has no monthly
  elements to go to.
- Fix: add monthly CHAP output elements to the demo database.

## chap-core

### A Helm chart release is the latest release of the repository

- Seen: `chap-1.2.0`, published 2026-10-29, with `helm/chart-releaser-action`
  v1.7.0 in `.github/workflows/release-chart.yaml`.
- `GET /repos/dhis2-chap/chap-core/releases/latest` answers `chap-1.2.0`, and
  the Releases page marks it as Latest in place of v2.5.1. Its one asset is
  `chap-1.2.0.tgz`. No image has this tag, so a client that follows the latest
  release gets `ghcr.io/dhis2-chap/chap-core:chap-1.2.0: not found`.
- chap-devops now reads the release list and takes the highest `vX.Y.Z`.
- Fix: set `mark_as_latest: false` on the chart-releaser step.

## Open Climate Service (`main`)

### A failed read from the CHIRPS3 server is a 500, with no retry

- `POST /ingestions` answered 500 (`RasterioIOError: Read failed`,
  `TIFFFillTile ... got 0 bytes`) once; the same request passed 4 seconds
  later.
- Seen again 2026-10-30, `sha256:9f454cbca239`: 1 run of 15 answered 500 with
  `'/vsicurl/https://data.chc.ucsb.edu/.../chirps-v3.0.rnl.2024.01.03.cog' not
  recognized as being in a supported file format`.

## chap-models

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
