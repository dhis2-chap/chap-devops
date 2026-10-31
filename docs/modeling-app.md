# Your first forecast in the Modeling App

A walk from a fresh deployment to an evaluated model and a three-month dengue
forecast, by clicking in the Modeling App. It takes about fifteen minutes, most
of it waiting for the model. It starts where
[Chap with a local DHIS2](./use-cases/chap-with-local-dhis2.md) ends:

```sh
chap-devops init mychap --models chapkit_simple_multistep_model --with dhis2
cd mychap
chap-devops up
chap-devops status          # until the dhis2 line says up
chap-devops dhis2 connect
chap-devops open dhis2
```

`chap-devops dhis2 connect` points the DHIS2 route `chap` at chap-core, installs the
Modeling App and the DHIS2 Climate App, and runs the DHIS2 analytics tables.
It also makes the configured models that the Modeling App lists. It took about
40 seconds. Its last line is
`` the Modeling App can reach Chap; open DHIS2 with `chap-devops open dhis2` ``.

Tested on 2026-10-19 with chap-devops v0.110.0, Modeling App 7.3.0, DHIS2
2.42.6 with the Laos demo database, chap-core v2.5.0 and Simple Multistep
(chapkit) 0.1.2. Other versions of the Modeling
App may name a button differently; the steps stay the same. `chap-devops dhis2 show`
says which Modeling App version a deployment has.

## What the demo data holds

The DHIS2 that `chap-devops` starts is seeded with a demo database from Laos:

| Data | Where it is in DHIS2 | Range |
| --- | --- | --- |
| Dengue cases | `Dengue Cases (Any) - Weekly` | 2019 to 2024, weekly |
| Rainfall | `CCH - Precipitation (CHIRPS)` | 2015 to April 2025 |
| Temperature | `CCH - Air temperature (ERA5-Land)` | 2015 to April 2025 |
| Population | `LSB: Population (Estimated-single age)` | 2016 to 2025, yearly |

for Lao PDR, its 18 provinces and their districts. The Modeling App adds the
weekly cases up to months when an evaluation is monthly.

## 1. Open the Modeling App

Log in to DHIS2 with `admin` / `district`, open the app menu (the grid of
squares at the top right), type `Modeling` in its search box and pick
**Modeling**: the menu shows only a few top apps until you search. The
dashboard is empty until the first evaluation.

![The Modeling App dashboard](images/modeling-app/01-dashboard.png)

## 2. Start an evaluation

An evaluation asks: had this model been used in the past, how close would its
forecasts have come? It trains the model on the earlier data, forecasts the
months after, and compares the forecast with what really happened, several
times over.

In **Evaluate**, **Overview**, choose **New evaluation**. The page opens on the
tab **Use saved dataset**, which needs a dataset saved before. Choose the tab
**Import from DHIS2** and fill in:

| Field | Value |
| --- | --- |
| Name | anything, for example `Multistep Laos dengue` |
| Period type | Monthly |
| From period / To period | 2019 January to 2024 December: pick the year in the box at the top of the picker, then the month |
| Organisation units | tick **Lao PDR**, pick the level **Province** below the tree, then **Confirm Selection** |
| Model | **Select model**, click **Simple Multistep Model (chapkit) [Monthly climate]** in the list, then **Use this model** |

The list cuts long names short, for example `Simple Multistep Model
(chapkit) [Mont…`. The panel on the right shows the full name of the entry you
clicked, and **Use this model** picks that entry.

Leave the **Backtest parameters** as they are: 3 forecast periods, 7 splits, a
step of 1, 1 training run, and the future-weather provider *Seasonal
climatology*.

![Selecting the model](images/modeling-app/02-select-model.png)

**Simple Multistep Model (chapkit)** is the model `chap-devops` started with
`--models chapkit_simple_multistep_model`. It has two entries, one for each
configuration in its marketplace entry: *Monthly climate* uses rainfall,
temperature and relative humidity, and *Monthly selfhistory* uses only the
case history. The list shows one entry for each configured model, as
`<model name> [<variant name>]`. Each model enabled with `chap-devops models
enable` adds its own entries. chap-core 2.4 and later does not make these
entries itself: `chap-devops up --wait`, `chap-devops dhis2 connect` and `chap-devops models
configs sync` make them. If a model is not in the list, run `chap-devops status`. If it
shows the model as `registered, not configured`, run `chap-devops models configs sync`.
To add an entry of your own with other option values, run `chap-devops models
configs add`. See [Adding](./models.md#adding).
`chap-devops models test --all` makes each model train and predict. See
[Configured models](./models.md#configured-models).

Under each name, the list shows the author of the model. The list also holds
models that chap-core ships configured on its own and runs inside its worker,
such as **CHAP-EWARS Model** and **Monthly CHAP-EWARS model**. Some of them
have the status *Deprecated*. From chap-core v2.5.0, it also has the three
built-in baselines, **Persistence**, **Global median** and **Seasonal
median**. chap-core runs them beside each evaluation, so you can compare a
model with them. None of these are models that `chap-devops` started. The two
`(chapkit)` entries have the status *Limited*.

![Organisation units: Lao PDR at the Province level](images/modeling-app/03-org-units.png)

## 3. Map the data

**Configure sources** asks which DHIS2 data each input of the model comes from.
Search by name in each box:

| Model input | DHIS2 data item |
| --- | --- |
| Disease cases | `Dengue Cases (Any) - Weekly` |
| Rainfall | `CCH - Precipitation (CHIRPS)` |
| Mean temperature | `CCH - Air temperature (ERA5-Land)` |
| Mean relative humidity | `CCH - Relative humidity (ERA5-Land)` |

A search for the dengue cases also finds the items
`CHAP Dengue Cases (Any) - Weekly Quantile ...` and
`NCLE: 7. Dengue cases (any)`. The `CHAP` items hold forecasts, not cases.
Pick `Dengue Cases (Any) - Weekly` exactly.

![Mapping the model inputs to DHIS2 data](images/modeling-app/04-map-data.png)

**Save**, then **Start dry run**. It checks the data without starting
anything, and should say *All 18 locations can be successfully imported*.

![The dry run: 18 valid locations](images/modeling-app/05-dry-run.png)

Close it and choose **Start import**. The app moves to **Jobs**, where the
evaluation runs. With Simple Multistep it took about 24 seconds on an Apple
Silicon Mac (M2 Max), where the model runs under emulation. `chap-devops jobs` in the
terminal shows the same job; its type is `create_backtest_from_data`.

## 4. Read the result

Back in **Evaluate**, **Overview**, open the evaluation. The chart shows the
real cases (orange) and, for the period picked under **Split period**, the
model's forecast with its 50% and 80% prediction intervals. The list on the left
switches between all of Laos and one province.

![An evaluation result](images/modeling-app/06-evaluation.png)

**Evaluation metrics**, lower on the right, scores the forecasts over every
split and province: lower is better for *CRPS (log1p)*, *MAE* and *RMSE*, and
*Coverage 10-90* should be close to its target of 0.8 (80% of the real values
inside the 80% interval). **Show all 13 metrics** lists the others.

![Evaluation metrics](images/modeling-app/07-metrics.png)

To compare two models, run a second evaluation on the same data:

1. On the first evaluation, choose **Create new based on...**. It asks which
   settings to copy; leave them all ticked and choose **Create**.
2. The copy has the name, provinces, model, data mapping and periods of the
   first, and its name ends in `(Copy)`.
3. On the copy, **Select model** picks another model, for example
   **Simple Multistep Model (chapkit) [Monthly selfhistory]**. This clears the
   data mapping (seen with Modeling App 7.3.2), and **Start import** does
   nothing until it is mapped again.
4. Choose **Configure sources**, pick `Dengue Cases (Any) - Weekly` for
   **Disease cases**, and choose **Save**. Then choose **Start import**.
5. In **Evaluate**, **Compare**, pick the first evaluation in the left box and
   the copy in the box beside it.

The evaluation of *Monthly selfhistory* took about 20 seconds. The comparison shows the two side by side, province
by province; the box **Location(s)** starts with ten provinces selected. More
marketplace models come with `chap-devops models enable ID` and `chap-devops up`
(`chap-devops models list` shows the ids).

![Two evaluations side by side](images/modeling-app/08-compare.png)

## 5. Make a forecast

On the evaluation, **Create prediction setup** keeps the model, the data
mapping and the provinces under a name: type one in **Setup name**, leave
**Alert policy** at *No alert policy* and **Set default import mapping** off,
and **Save**. The app then opens the setup.
Once the evaluation has a setup, the same button says **Predict**.

![Creating a prediction setup](images/modeling-app/09-prediction-setup.png)

On the setup, choose **Run prediction**. The page fills in a name for the
run. In **Training period**, set the last training period to the last month
with case data: **2024 December** for the demo data. Then choose **Run
prediction** again. The app goes back to the setup, where the run shows as
*Running*. The forecast covers the three months after that month.

![Running a prediction](images/modeling-app/10-run-prediction.png)

The job took about 7 seconds in `chap-devops jobs`. The row in the app said
*Success* within a minute, without a reload.
**Go to last run** shows one chart per province:
the real cases up to December 2024 and the forecast for January to March 2025.

![A forecast per province](images/modeling-app/11-forecast.png)

**Import**, on the right of the forecast, opens a page that writes the
forecast back into DHIS2 as data, where dashboards and maps can use it. It
needs five data elements for the forecast's quantiles. Each box is empty at
first; open it, and it lists the elements of the demo database under
**Suggested**. Pick `CHAP Dengue Cases (Any) - Weekly Quantile High` to
`... Quantile Low`.

Leave **Use alert outputs** off: it is off for a setup with no alert policy
(seen with Modeling App 7.3.2). If you turn it on, the page also needs an
**Outbreak indicator**: **Clear and import** does nothing until it has one.
Pick `CHAP Dengue Cases (Any) - Weekly Outbreak Alert`; type `Outbreak` to find
it.
**Clear and import** asks once more before it writes, then the app goes back
to the setup and says *Prediction data posted successfully*. DHIS2 2.42.7
accepted the monthly values into those elements, but their data set is weekly. For a monthly forecast, create monthly data
elements in DHIS2's Maintenance app and pick those.

## When something goes wrong

| What you see | What to do |
| --- | --- |
| Typing `Modeling` in the app menu finds nothing | `chap-devops dhis2 connect` has not run, or failed; run it and read its last line. |
| The model list does not have the model `chap-devops` enabled | `chap-devops status`: the model must be `registered`. If it says `registered, not configured`, run `chap-devops models configs sync`. Otherwise the line under the table says why. |
| `Oops! Sorry, an unexpected error`, or `Unnamed evaluation` rows, after the deployment was recreated | The browser holds a login to the DHIS2 that was removed: open DHIS2 again and log in. See [Troubleshooting](./troubleshooting.md#oops-sorry-an-unexpected-error-or-unnamed-evaluation-in-the-modeling-app). |
| **Clear and import** does nothing | Scroll down: with **Use alert outputs** on, the page says `Outbreak indicator is required`. Pick `CHAP Dengue Cases (Any) - Weekly Outbreak Alert`, or turn **Use alert outputs** off. |
| A job in **Jobs** says it failed | `chap-devops jobs`, then `chap-devops jobs logs ID` with the id it prints: the model's own error is at the end. |

More on connecting DHIS2 and Chap: [DHIS2](./dhis2.md). The Modeling App's own
guide is at
[chap.dhis2.org](https://chap.dhis2.org/chap-modeling-platform/modeling-app/).
