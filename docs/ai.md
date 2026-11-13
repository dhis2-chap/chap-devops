# For AI assistants

This page is written for an AI assistant that helps someone install and run
something with `chap-devops`. If a person pointed you here, follow the steps below in order. Keep
your answers short and concrete, and give one command at a time.

## What Chap is

Use this when the person asks what they are installing, or seems unsure why
they would want it.

[Chap](https://chap.dhis2.org) forecasts
cases of climate-sensitive diseases such as dengue and malaria. It learns from
past case counts together with climate data (rainfall, temperature) and
population, and predicts the next months for each district or province. Before
trusting a model it runs an *evaluation*: it pretends to stand at several past
points in turn, forecasts the months after each, and scores how close each
forecast came. Ministries of health use
it together with [DHIS2](https://dhis2.org), the health information system
where their case data already lives, through the **Modeling App** inside DHIS2.

`chap-devops` is the installer. It runs Chap, its forecasting models, a DHIS2 with
demo data and the other pieces on one computer with Docker, so the person can
use them without setting each one up by hand.

A few words the person will see:

| Word | Meaning |
| --- | --- |
| chap-core | Chap's backend: the service that trains, evaluates and runs the models. The Modeling App is its interface in DHIS2. |
| model | One forecasting method, for example Simple Multistep. Each runs in its own container. |
| DHIS2 | The health information system. `chap-devops` can run a demo one with data from Laos. |
| Modeling App | Chap's user interface, installed inside DHIS2. |
| evaluation | Testing a model on past data: it forecasts from several past points and scores each forecast. Also called a backtest. |
| prediction | A forecast for the coming months. |
| OCS | Open Climate Service: downloads and serves climate data. Optional. |

## How to help

1. **Ask what they want to do**, in one question, and match the answer to a
   group in [The options](#the-options). Do not show all nineteen options.
   - *Learn Chap, try forecasting, see what it does* (a student, an analyst, a
     public health person): **option 1**. It is the only one with a user
     interface they can click through. If their computer cannot give Docker
     8 GB of memory, option 2 instead (6 GB).
   - *Run Chap for a DHIS2 that already exists*: option 5.
   - *One piece on its own* (climate data, one model, a DHIS2): options 6 to 10.
   - *Develop a model, chap-core or DHIS2*: options 11 to 18.
   - *Two deployments at once*: option 19.
   - *Evaluate a model on their own data file, without DHIS2*: option 20.

   If they are unsure, recommend option 1.
2. **Check the machine** with [Before any option](#before-any-option).
3. **Give the commands for their option**, in order, one block at a time. Wait
   for them to say it worked before giving the next block. Every option has an
   "It worked when" line: use it to decide.
4. **When it works, say what to do next.** Option 1 continues in the Modeling
   App, and so do options 3 and 4 when they added DHIS2: walk them through [Your first forecast in the Modeling
   App](./modeling-app.md). Option 2 continues with
   [What to try next without DHIS2](#what-to-try-next-without-dhis2).
5. **If something fails**, ask them to paste the exact output. Then look for the
   first line that starts with `error:` or `warning:`. chap-devops puts the fix on the
   same line, usually a command in backticks: tell them to run that command.
   If that does not help, search [Troubleshooting](./troubleshooting.md) for
   the exact message. If chap-devops says it does not know a command or an option,
   the installed `chap-devops` is older than this page: `chap-devops self update` installs
   the newest release, and `chap-devops <command> --help` is always right for the
   installed one.
6. **Never** tell them to edit the `compose*.yml` files by hand (chap-devops rewrites
   them), and never ask them to paste the `.env` file (it holds passwords).

When an option has a "More" link, that page has the details. You only need it
if the person asks for something the steps here do not cover.

## If you run the commands yourself

If you have a shell on the person's machine and run the commands rather than
telling the person what to type:

- Run each block as written, top to bottom. Where an option shows
  alternatives, it says so and you pick one. Check its "It worked when" line
  yourself instead of waiting for the person to say it worked.
- If your shell does not keep its folder between commands, put
  `-C mychap` (the folder from `chap-devops init`) on every command in place of the
  `cd`: `chap-devops -C mychap up`.
- `chap-devops status` exits `0` only when everything the deployment declares is up
  and registered, and non-zero otherwise, so poll it by its exit code where
  a step says to run it every minute. A first DHIS2 start can take 20
  minutes.
  `chap-devops status --json` gives the same report as one JSON document; every
  command takes `--json`.
- The first `chap-devops up` downloads the images that take most of the disk in
  [Before any option](#before-any-option). Give it a long timeout, or run it
  in the background, and do not start a second one while it runs. Running
  `chap-devops up` again after it has finished or timed out is safe: it continues
  from where it stopped.
- `chap-devops dhis2 connect` waits by itself for DHIS2 to start, up to 20 minutes;
  it prints a line on stderr while it waits.
- Nothing prompts you: without a terminal, a question becomes an `error:`
  with the flag that answers it. `chap-devops self update` is the exception: without
  a terminal it replaces the binary and does not ask, so ask the person before
  you run it. `chap-devops up --replace` stops another
  deployment holding the ports, `chap-devops down --volumes --yes` deletes data;
  ask the person before either. `chap-devops models add --yes` and
  `chap-devops run --yes` run an image that nobody reviewed; ask the person
  before you add `--yes`.
- Credentials go in the `.env` file in the folder. Ask the person to add them
  there themselves, and do not print or read that file.
- Do not run `chap-devops ui` (an interactive screen), `chap-devops up --attach` or
  `chap-devops logs -f` (they never return). `chap-devops logs SERVICE` without `-f`
  prints and returns.
- `chap-devops open NAME` opens a browser on the machine it runs on. Run
  `chap-devops open NAME --no-browser` instead: it prints the address and opens nothing,
  and you give the person that address (or open it in your own browser tool).
  `--json` never opens a browser either.

## Before any option

The person needs Docker: Docker Desktop on macOS and Windows, Docker Engine on
Linux. On Windows, chap-devops runs inside WSL 2: Docker Desktop with its WSL
integration turned on for the distribution, and every command below typed in
the WSL shell. Check two numbers before they start:

| | With DHIS2 (options 1, 8, 9, 10, 17, and 3 or 4 with `dhis2` added) | Without DHIS2 |
| --- | --- | --- |
| Memory for Docker | 8 GB or more | 6 GB or more |
| Free disk | about 30 GB | about 25 GB |

Most of the disk is the model and worker images, which the first `chap-devops up`
downloads: expect it to take a while on a slow connection. On Docker Desktop
the memory is set under Settings, Resources. Then:

```sh
curl -fsSL https://raw.githubusercontent.com/dhis2-chap/chap-devops/main/install.sh | sh
chap-devops doctor
```

On Windows, run those two lines in the WSL shell, which installs the Linux
binary; do not use the native Windows zip, which is published untested. See
[Install](./install.md#windows).

It worked when `chap-devops doctor` shows no `fail` lines, and its `disk space`
line shows at least the number in the table above. `chap-devops doctor` passes with
less, so compare the number yourself. On an Apple Silicon Mac a `warn` line
about `linux/amd64` and emulation is normal and can be ignored: the models
run, only slower.

`chap-devops doctor` shows the `memory` line only in a folder with DHIS2. For an
option with DHIS2, run `chap-devops doctor` again in the new folder after
`chap-devops init`. Its `memory` line, for example `31.3 GB available to docker`,
must show at least 8 GB.

Every option below creates a new folder. Tell the person to run the commands
from the folder where they want it created, and then to `cd` into it as shown.
Several options use the same folder name (`mychap`): if one from an earlier
option is still there, `chap-devops init` says it `already contains a chap-devops deployment`.
Either pick another name in the `chap-devops init` line (and the `cd`), or remove the
old one first as shown in [Every option: stop, start, remove](#every-option-stop-start-remove).

## The options

| # | Option | Pick this if the person wants to... |
| --- | --- | --- |
| | **Use Chap** | |
| 1 | [Chap in DHIS2, with the Modeling App](#1-chap-in-dhis2-with-the-modeling-app) | learn Chap: evaluate a model and make a forecast by clicking |
| 2 | [Chap with forecasting models](#2-chap-with-forecasting-models) | run Chap without DHIS2, from the terminal or its API |
| 3 | [Chap with climate data](#3-chap-with-climate-data) | use Chap together with Open Climate Service |
| 4 | [Chap with every model](#4-chap-with-every-model) | try all the forecasting models, not just one |
| 5 | [Chap for a DHIS2 that already exists](#5-chap-for-a-dhis2-that-already-exists) | connect Chap to a DHIS2 someone else runs |
| | **One piece on its own** | |
| 6 | [Climate data only](#6-climate-data-only) | run Open Climate Service, no Chap |
| 7 | [One forecasting model only](#7-one-forecasting-model-only) | call one model's API directly, no Chap |
| 8 | [DHIS2 only](#8-dhis2-only) | have a DHIS2 to play with, no Chap |
| 9 | [A particular DHIS2 version](#9-a-particular-dhis2-version) | run DHIS2 2.41, 2.43 or the next, unreleased one |
| 10 | [Climate data and DHIS2, no Chap](#10-climate-data-and-dhis2-no-chap) | have OCS and a DHIS2 together, without Chap |
| | **Develop** | |
| 11 | [A new model of my own](#11-a-new-model-of-my-own) | start a new model, in Python or R, from a template |
| 12 | [My own model](#12-my-own-model) | run a model they wrote themselves |
| 13 | [My own model, while I work on it](#13-my-own-model-while-i-work-on-it) | develop a model in its folder and have Chap use it |
| 14 | [A model image I built](#14-a-model-image-i-built) | run a model image they built with `docker build`, without publishing it |
| 15 | [My own chap-core, with the models](#15-my-own-chap-core-with-the-models) | develop chap-core itself and have the models register with it |
| 16 | [chap-core built from my checkout](#16-chap-core-built-from-my-checkout) | test their chap-core changes with everything else around them |
| 17 | [My own DHIS2, with Chap](#17-my-own-dhis2-with-chap) | connect a DHIS2 they run themselves to Chap from chap-devops |
| 18 | [A DHIS2, with my own chap-core](#18-a-dhis2-with-my-own-chap-core) | have chap-devops run DHIS2 in front of the chap-core they are developing |
| | **More than one** | |
| 19 | [Two of these at the same time](#19-two-of-these-at-the-same-time) | keep one option running while starting another |
| | **The chap CLI** | |
| 20 | [Evaluate a model on my own data](#20-evaluate-a-model-on-my-own-data) | run `chap eval` on their CSV without installing Python or uv |
| | **The chapkit CLI** | |
| 21 | [Make my MLproject model a chapkit service](#21-make-my-mlproject-model-a-chapkit-service) | change a model with an `MLproject` file to a chapkit service, without installing Python or uv |
| 22 | [Serve my MLproject model, with no build](#22-serve-my-mlproject-model-with-no-build) | evaluate and test a model with an `MLproject` file, without an image |

### 1. Chap in DHIS2, with the Modeling App

Needs 8 GB of memory for Docker. The DHIS2 comes with demo data from Laos:
weekly dengue cases for 2019 to 2024 in 18 provinces, with rainfall,
temperature and population.

```sh
chap-devops init mychap --models chapkit_simple_multistep_model --with dhis2
cd mychap
chap-devops doctor
chap-devops up
chap-devops status
```

Before `chap-devops up`, check the `memory` line of `chap-devops doctor`: 8 GB or more.
The first time, DHIS2 takes several minutes to start; the `dhis2` line of
`chap-devops status` says `starting` until DHIS2 answers, then `up`. Then:

```sh
chap-devops dhis2 connect
chap-devops open dhis2
```

`chap-devops dhis2 connect` waits for DHIS2 to finish starting, then takes about a
minute: it connects DHIS2 to Chap, gives each model its configured models, installs
the Modeling App and the Climate App, and prepares the data the Modeling App
reads. Its last line is ``the Modeling App can reach Chap; open DHIS2 with
`chap-devops open dhis2` ``. It worked when DHIS2 opens in the browser, the login `admin` /
`district` works, and typing `Modeling` in the app menu (the grid icon at the
top right) finds **Modeling**.

If `chap-devops dhis2 connect` says that a model is `not registered with chap-core`,
wait a minute. Then run `chap-devops status`. When it shows the model as
`registered`, run `chap-devops models configs sync`.

Next: [Your first forecast in the Modeling App](./modeling-app.md), which takes
them from here to an evaluation and a forecast in about fifteen minutes.

More: [Chap with a local DHIS2](./use-cases/chap-with-local-dhis2.md).

### 2. Chap with forecasting models

chap-core and one model, without DHIS2: less memory, but no user interface
beyond chap-core's API page.

```sh
chap-devops init mychap --models chapkit_simple_multistep_model
cd mychap
chap-devops up
chap-devops status
```

It worked when `chap-devops status` says chap-core is `up` and the line under the
table says the models are registered (`1 model registered`, or `all 2 models
registered` with more). Models can take a minute or two after `chap-devops up`: if
some say `not registered`, wait a minute and run `chap-devops status` again.
When they are registered, run `chap-devops models configs sync`, as the last
line of `chap-devops up` says. Then `chap-devops open chap-core` opens
chap-core's API page in the browser.

Next: [What to try next without DHIS2](#what-to-try-next-without-dhis2).

More: [Chap with forecasting models](./use-cases/chap-with-models.md).

### 3. Chap with climate data

```sh
chap-devops init mychap --models chapkit_simple_multistep_model --with ocs,s3
cd mychap
chap-devops up
chap-devops status
```

It worked when `chap-devops status` shows chap-core and `ocs` as `up`. Some climate
datasets need an account with Copernicus or the Earth Data Hub; the keys go in
the `.env` file in the folder, where they are already listed, commented out.
Add `,dhis2` to `--with` for the Modeling App as well (8 GB of memory), then
continue as in option 1 from `chap-devops dhis2 connect`. OCS covers Laos, like the
demo DHIS2, until the person changes it.

Next, when they want climate data: [Getting climate data](#getting-climate-data).

More: [Chap with climate data from OCS](./use-cases/chap-with-ocs.md).

### 4. Chap with every model

Like option 2, with every marketplace model. The model images are 1 to 7 GB
each. `chap-devops init` pulls one of them to read its user, and the first
`chap-devops up` pulls the others. Both can take many minutes on a slow connection.

```sh
chap-devops init mychap --models all
cd mychap
chap-devops up
chap-devops status
chap-devops models test --all
```

It worked when `chap-devops status` ends with `all 4 models registered` (one per
marketplace model; `all` leaves out the two starters) and
`chap-devops models test --all` ends with `4 of 4 models pass`. When the
marketplace has more models, the numbers are higher.
The tests take one to two minutes. If some models say `not registered`, wait
a minute and run `chap-devops status` again.

After a plain `chap-devops up`, the table shows each model as
`registered, not configured`, and that is normal. `chap-devops models test --all`
does not need a configured model, but a backtest and the Modeling App do.
Run `chap-devops models configs sync` to create them. Then `chap-devops status` shows each
model as `registered`.

Add `--with dhis2` to the `chap-devops init` line to compare them in the Modeling
App (8 GB of memory, then as in option 1 from `chap-devops dhis2 connect`, which
also creates the configured models).

More: [Chap with forecasting models](./use-cases/chap-with-models.md#every-model-in-the-marketplace).

### 5. Chap for a DHIS2 that already exists

The person needs the DHIS2 address, an admin account on it, and an address at
which that DHIS2 can reach this computer (not `localhost`).

```sh
chap-devops init mychap --models chapkit_simple_multistep_model --api-token
cd mychap
chap-devops up
chap-devops dhis2 use https://THEIR-DHIS2 --chap-url https://ADDRESS-OF-THIS-MACHINE
chap-devops dhis2 connect
```

It worked when `chap-devops dhis2 connect` finishes without `error:`. It only sets
the route on their DHIS2: its `skipped:` lines name the apps to install and
the analytics run, which are for whoever runs that DHIS2 to decide (`chap-devops dhis2
apps`, `chap-devops dhis2 analytics`). The first time, `chap-devops dhis2 use` warns and
`chap-devops dhis2 connect` stops with ``error: chap-devops has no credentials for this
DHIS2``. Then ask the person to put `DHIS2_ADMIN_USERNAME` and
`DHIS2_ADMIN_PASSWORD` (or `DHIS2_API_TOKEN`) in the `.env` file in the
folder, and run `chap-devops dhis2 connect` again. If
`--chap-url` is an `http://` address and the DHIS2 is 2.42 or later, it may say
`DHIS2 refused the route`: whoever runs that DHIS2 has to add the `--chap-url`
address, with no path, to `route.remote_servers_allowed` in its `dhis.conf`
and restart it; the error line names the exact value.

If `chap-devops dhis2 connect` says that a model is `not registered with chap-core`,
wait a minute. Then run `chap-devops status`. When it shows the model as
`registered`, run `chap-devops models configs sync`.

More: [Chap for a DHIS2 that runs elsewhere](./use-cases/chap-for-external-dhis2.md).

### 6. Climate data only

```sh
chap-devops init climate --only ocs,s3
cd climate
chap-devops up
chap-devops status
chap-devops open ocs
```

It worked when `chap-devops status` shows `ocs` as `up` and the browser opens OCS.

To put some data in (public rainfall data, no account needed):

```sh
curl -X POST http://localhost:8790/ingestions -H 'Content-Type: application/json' -d '{"dataset_id": "chirps3_precipitation_daily", "start": "2024-01-01", "end": "2024-01-03", "publish": true}'
```

It worked when the answer contains `"status":"completed"`.

Next: [Getting climate data](#getting-climate-data).

More: [An OCS server on its own](./use-cases/ocs-alone.md).

### 7. One forecasting model only

No folder needed: from any directory that is not a deployment,

```sh
chap-devops run chapkit_simple_multistep_model
```

It worked when the last lines say `running chapkit_simple_multistep_model on
http://localhost:5001 (answered in ...)` and name the `chap-devops stop` that stops
it. With `-a`, chap-devops stays in the foreground and shows the model's log, and
Ctrl-C stops the model. The model's API is at that URL, and its documentation at `/docs` under it.
It is reachable from this machine only. A first run pulls the image, which can
take several minutes; the command waits for it. Other models: `chap-devops models
list` shows their ids, and `chap-devops run` takes any of them, a GitHub repository
URL or a ghcr image the same way. `chap-devops ps` lists what runs, `chap-devops top` shows
it live, and `chap-devops stop chapkit_simple_multistep_model` stops it.

More than one works the same way: one `chap-devops run` per model, each on its own
port, and `chap-devops ps` lists them all. `--group NAME` keeps a set apart, and
`chap-devops stop --group NAME --purge` removes that set with its data.

For a folder of its own instead, with `chap-devops status` and the rest:

```sh
chap-devops init onemodel --only none --models chapkit_simple_multistep_model
cd onemodel
chap-devops up
chap-devops status
```

It worked when `chap-devops status` shows the model as `up`. Its API is at the
port in the REACH column, usually `http://localhost:5001`, and
`chap-devops open chapkit_simple_multistep_model` opens its documentation.

More: [Running one model](./run.md),
[A chapkit model service on its own](./use-cases/model-alone.md),
[Any number of model services, no folder](./use-cases/models-with-run.md).

### 8. DHIS2 only

Needs 8 GB of memory for Docker. The first start takes several minutes.

```sh
chap-devops init dhis --only dhis2
cd dhis
chap-devops up
chap-devops status
```

Run `chap-devops status` every minute until the `dhis2` line says `up`, then
`chap-devops open dhis2`. It worked when the login `admin` / `district` works. This
is DHIS2 2.42 with the Laos demo data; for another version, see option 9.

More: [A DHIS2 on its own](./use-cases/dhis2-alone.md).

### 9. A particular DHIS2 version

Needs 8 GB of memory for Docker. Each version starts with an empty DHIS2
(only 2.42 has demo data, see option 8). `chap-devops init` says so in a
`warning: chap-devops knows no DHIS2 demo dump` line: that is expected. Run one of
these, not all three:
2.43,

```sh
chap-devops init dhis --only dhis2 --dhis2-tag 2.43
```

2.41,

```sh
chap-devops init dhis --only dhis2 --dhis2-tag 2.41
```

or the next DHIS2, not released yet:

```sh
chap-devops init dhis --only dhis2 --dhis2-image dhis2/core-dev --dhis2-tag master
```

Then:

```sh
cd dhis
chap-devops up
chap-devops status
```

Run `chap-devops status` every minute until the `dhis2` line says `up`, then
`chap-devops open dhis2`. It worked when the login `admin` / `district` works. To
have Chap with it, use option 1 with the same `--dhis2-tag` (and
`--dhis2-image`) added to its `chap-devops init` line; without demo data there is
nothing to forecast until they import some.

More: [A DHIS2 on its own](./use-cases/dhis2-alone.md#picking-the-version).

### 10. Climate data and DHIS2, no Chap

Needs 8 GB of memory for Docker.

```sh
chap-devops init lab --only ocs,s3,dhis2
cd lab
chap-devops up
chap-devops status
```

Run `chap-devops status` every minute until every line says `up`, then
`chap-devops open ocs` and `chap-devops open dhis2`. It worked when both open, and the
DHIS2 login `admin` / `district` works.

More: [Combinations](./use-cases/combinations.md#climate-data-and-a-dhis2-no-chap).

### 11. A new model of my own

For someone who wants to write a new model. Ask which language they use: Python
or R. `chap-devops models new` makes the project. Docker builds it, so Python, uv
and R are not necessary on their machine.

```sh
chap-devops models new my-model
```

For R, add `--template shell-r`. For a model in the MLproject format, use
`--template mlproject-py` or `--template mlproject-r`.

It worked when the first line says `created my-model in my-model`. Then build
the image in the new folder:

```sh
cd my-model
git init && git add . && git commit -m "feat: first version"
docker build --platform linux/amd64 --build-arg GIT_REVISION=$(git rev-parse HEAD) -t my-model:dev .
```

It worked when the build ends with no error. The first build pulls the base
image, which takes some minutes. Then run it with chap-core:

```sh
cd ..
chap-devops init mychap --models none
cd mychap
chap-devops models add my-model:dev
chap-devops up --wait
chap-devops models test my_model --backtest
```

It worked when the last line says `1 of 1 model passes`. The model is in
`main.py` (the default type) or in the `scripts/` folder. The `README.md` of
the project lists the files.

After a change, raise `version` in `main.py` (or in `MLproject`) first:
chap-core stores each version only once. Then commit, build the image again
with the same `docker build` line, and run `chap-devops restart` and the
`chap-devops models test` line in the `mychap` folder. If the test says
`is stored from revision`, the version was not raised.

More: [A new model project](./models.md#a-new-model-project).

### 12. My own model

The model has to be a chapkit model whose GitHub repository publishes a public
image on ghcr.io (most do, through their publish workflow). If `chap-devops models
add` says the repository `publishes no public image`, use option 14 instead:
build the image from their checkout and add that.

To try it, with no folder:

```sh
chap-devops run https://github.com/THEIR-ORG/THEIR-MODEL
```

It worked when the last lines say `running ... on http://localhost:...`. To keep
it in a folder of its own:

```sh
chap-devops init mymodel --only none
cd mymodel
chap-devops models add https://github.com/THEIR-ORG/THEIR-MODEL
chap-devops up
chap-devops status
```

`chap-devops models add` asks `Add it? [y/N]` for a repository that the
marketplace does not list, because nobody reviewed it. The person answers `y`.
If you run the command yourself, ask the person first, then add `--yes`.

It worked when `chap-devops status` shows the model as `up`, on the port in its
REACH column. A repository the marketplace already lists is enabled as that
marketplace model, with its reviewed version; the line `... is the marketplace
model ...; enabling that` says so.

More: [Models outside the marketplace](./models.md#models-outside-the-marketplace).

### 13. My own model, while I work on it

For someone developing a chapkit model in its folder. chap-devops runs chap-core;
they run the model from the folder.

```sh
chap-devops init mychap --models none
cd mychap
chap-devops up
```

Then, in the model's folder (keep the single quotes):

```sh
export SERVICEKIT_ORCHESTRATOR_URL='http://localhost:8700/v2/services/$register'
export SERVICEKIT_HOST=host.docker.internal
export SERVICEKIT_PORT=8001
uv run uvicorn main:app --host 0.0.0.0 --port 8001
```

`main:app` is right for a model with `main.py` at the top of its folder, which
is how chapkit models are laid out. If `main.py` is inside a package folder
instead, use `PACKAGE.main:app`.

It worked when `chap-devops status`, run in the `mychap` folder, lists the model with
the state `unmanaged` and a LAST PING of a few seconds (unmanaged because chap-devops
did not start it, which is expected).

More: [Your model from its checkout, with Chap](./use-cases/model-on-host.md).

### 14. A model image I built

In the model's folder, which must be a git checkout:

```sh
docker build --platform linux/amd64 --build-arg GIT_REVISION=$(git rev-parse HEAD) -t my-model:dev .
```

`GIT_REVISION` gives the image the commit it was built from. chap-core needs
it to store the model template. Without it, `chap-devops models configs sync` stops
with `answered HTTP 409 Conflict` and `is stored from revision None` or
`its source reports no git revision`. Then:

```sh
chap-devops init mychap --models none
cd mychap
chap-devops models add my-model:dev
chap-devops up
chap-devops status
```

It worked when `chap-devops status` lists the model as `registered`. Usually it does
not the first time:

1. Right after `chap-devops up`, the model is `running, not registered`. Wait a
   minute and run `chap-devops status` again.
2. The model registers under the `id` in its `MLServiceInfo` (in `main.py`),
   not as `my-model`. Then `chap-devops status` shows that id as `unmanaged`, next
   to `my-model` as `running, not registered`.
3. The line under the table gives the two commands that fix it:
   `chap-devops models remove my_model`, then `chap-devops models add my-model:dev
   --service-id` with the id from the `unmanaged` row. Run them, then run
   `chap-devops up` again.
4. When the model is `registered, not configured`, run
   `chap-devops models configs sync`.

If `chap-devops status` or `chap-devops models configs sync` says that the model reports no
git revision, build the image again with the `GIT_REVISION` line above. Then
run `chap-devops restart` and `chap-devops models configs sync`.

More: [A model image you built yourself](./use-cases/local-model-image.md).

### 15. My own chap-core, with the models

For someone developing chap-core itself. They run chap-core from its folder,
listening on `0.0.0.0:8000`; chap-devops runs the models. Have them start their
chap-core first: `chap-devops init` then sees whether it runs in Docker and
configures the models to match.

```sh
chap-devops init models --chap-core-url http://localhost:8000 --models chapkit_simple_multistep_model
cd models
chap-devops up
chap-devops status
```

It worked when `chap-devops status` shows chap-core `up` and the model `registered`.
Right after `chap-devops up`, the model is `running, not registered`; run
`chap-devops status` again after some seconds. If the model is
`registered, not configured`, run `chap-devops models configs sync`.

If chap-core was started after `chap-devops up`, the model stays
`running, not registered`: it tries to register for about ten seconds only.
Two minutes after `chap-devops up`, the line under the table gives
`chap-devops restart --all chapkit-simple-multistep-model`. Run it.

`registered, unreachable` means chap-core cannot call the model back, usually
because chap-core was not running during `chap-devops init`. If their chap-core
runs in a container, the line under the table gives the two commands that fix
it: `chap-devops components enable chap-core` with
`--models-host host.docker.internal`, then `chap-devops up`.

More: [chap-core from its checkout, with the models](./use-cases/chap-core-on-host.md).

### 16. chap-core built from my checkout

For someone changing chap-core who wants to run their version with models.
Replace the path with where they cloned chap-core:

```sh
chap-devops init mychap --source ~/dev/chap-core --models chapkit_simple_multistep_model
cd mychap
chap-devops up
chap-devops status
```

The first `chap-devops up` builds chap-core and takes several minutes (longer on an
Apple Silicon Mac). It worked when `chap-devops status` shows chap-core `up` and the
models registered. After changing the code, `chap-devops up` again rebuilds it.

More: [chap-core built from its checkout](./use-cases/chap-core-from-checkout.md).

### 17. My own DHIS2, with Chap

For someone who already runs DHIS2 on this machine (for DHIS2 or app work).
Ask whether their DHIS2 runs in Docker. If it does not:

```sh
chap-devops init mychap --models chapkit_simple_multistep_model
cd mychap
chap-devops up
chap-devops dhis2 use http://localhost:8080 --chap-url http://localhost:8700
chap-devops dhis2 connect
```

If it runs in Docker, it reaches Chap through `host.docker.internal`:

```sh
chap-devops init mychap --models chapkit_simple_multistep_model
cd mychap
chap-devops up
chap-devops dhis2 use http://localhost:8080 --chap-url http://host.docker.internal:8700
chap-devops dhis2 connect
```

It worked when `chap-devops dhis2 connect` finishes without `error:`. If it says it
has no credentials, put `DHIS2_ADMIN_USERNAME` and `DHIS2_ADMIN_PASSWORD` (or
`DHIS2_API_TOKEN`) in the `.env` file and run it again. On a DHIS2 chap-devops did not start it sets only
the route: if its `skipped:` lines say an app is missing, `chap-devops dhis2 apps`
installs it, and `chap-devops dhis2 analytics` generates the tables the Modeling App
reads.

On DHIS2 2.42 and later it can then say `DHIS2 refused the route`: their
DHIS2 only proxies to the addresses its own `dhis.conf` allows. Tell them to
add the `--chap-url` address, with no path (for example
`route.remote_servers_allowed = https://*,http://localhost:8700`), to their
DHIS2's `dhis.conf`, restart their DHIS2, and run `chap-devops dhis2 connect` again.
A value with a path, such as `http://localhost:8700/**`, stops DHIS2 from
starting at all.

More: [A DHIS2 you run yourself, with Chap from chap-devops](./use-cases/dhis2-dev-with-chap.md).

### 18. A DHIS2, with my own chap-core

For someone developing chap-core who wants DHIS2 and the Modeling App in front
of it. Their chap-core must listen on `0.0.0.0:8000`. Needs 8 GB of
memory for Docker.

```sh
chap-devops init lab --chap-core-url http://localhost:8000 --with dhis2
cd lab
chap-devops up
chap-devops status
```

Then, once `chap-devops status` shows chap-core `up` (`chap-devops dhis2 connect` waits
for DHIS2 by itself):

```sh
chap-devops dhis2 connect
chap-devops open dhis2
```

It worked when `chap-devops dhis2 connect` finishes without `error:` and the login
`admin` / `district` works.

More: [A DHIS2 from chap-devops, with a chap-core elsewhere](./use-cases/dhis2-with-chap-core-elsewhere.md).

### 19. Two of these at the same time

Every option uses the same ports, so a second one needs its own. Create the
second one with these added to its `chap-devops init` line, one flag per piece it
has (`--ocs-port` only with OCS, `--dhis2-port` only with DHIS2; a flag for a
missing piece is an error):

| Piece | Flag |
| --- | --- |
| chap-core | `--api-port 8701` |
| models | `--port-base 5101` |
| OCS | `--ocs-port 8791` |
| DHIS2 | `--dhis2-port 8781` |

For example, a second option 2: `chap-devops init second --models chapkit_simple_multistep_model --api-port
8701 --port-base 5101`.

For a third, use 8702, 8792, 8782 and 5201, and so on. It worked when both
folders' `chap-devops status` say `up`. Running them one at a time needs none of
this; see the note below.

More: [Several deployments on one machine](./use-cases/several-deployments.md).

### 20. Evaluate a model on my own data

They have a dataset as a CSV, with a GeoJSON of the same name beside it. Do
not install Python or uv for them. In the directory with the data:

```sh
chap-devops chap eval --model-name https://github.com/dhis2-chap/minimalist_example_r \
  --dataset-csv data.csv --output-file eval.nc
chap-devops chap plot-backtest eval.nc --output-file eval.html
```

Use their model's GitHub URL in `--model-name`, or a marketplace id such as
`auto_arima_chapkit`: chap-devops starts that model by itself, and they need no
`chap-devops up`. Never give a `localhost` URL. The first run pulls
an image of about 12 GB, so tell them it takes some minutes. If chap-devops stops with
`runs in docker`, run the same command with `chap-devops chap --docker` and tell
them that the flag gives the container control of docker. If the directory of
an output file is not there, chap-devops creates it.

It worked when the last lines say `chap finished; it wrote eval.nc` and
`chap finished; it wrote eval.html`. They open `eval.html` in a browser.

More: [Evaluating a model on your own data](./use-cases/evaluate-with-chap-cli.md),
which goes from installing chap-devops to a comparison of two models, and
[The chap CLI](./chap-cli.md).

### 21. Make my MLproject model a chapkit service

They have a model directory with an `MLproject` file. Do not install Python or
uv for them. Make sure that the directory is in git, or that they have a copy:
the migration moves files to `_old/`. In the directory above the model:

```sh
chap-devops chapkit mlproject migrate my-model --yes
```

It worked when the last lines say `chapkit finished; it wrote ...` and
``wrote my-model/uv.lock with `uv lock` in the same image``. If the second line
is a warning that chap-devops could not write `uv.lock`, the build fails; show them
the warning.

If chapkit stops with `No Python (.py) or R (.r/.R) scripts`, run the command
again with `--base-image chapkit-py` for Python. For R, use `chapkit-r`,
`chapkit-r-tidyverse` or `chapkit-r-inla`.

Then build the image and test it:

```sh
cd my-model
docker build --platform linux/amd64 --build-arg GIT_REVISION=$(git rev-parse HEAD) -t my-model:dev .
chap-devops run my-model:dev
chap-devops -C ~/.local/share/chap-devops/run/default models test my_model
```

The build needs a git commit for `GIT_REVISION`. It worked when the last line
says `1 of 1 model passes`.

More: [The chapkit CLI](./chapkit-cli.md).

### 22. Serve my MLproject model, with no build

They have a model directory with an `MLproject` file. Do not install Python,
R or uv for them. In the directory above the model:

```sh
chap-devops chapkit mlproject run my-model
```

It worked when the line says `the MLproject service answers at
http://localhost:PORT`. The service runs in the foreground. Leave it running,
and use a second terminal for the next steps. Ctrl-C stops it.

If the train job stops with `No module named`, the image does not have a
package that the model needs. Run `chap-devops chapkit --image`, and show them the
list. Then run the command again with `--image NAME`.

To evaluate it, give the same port at `host.docker.internal`:

```sh
chap-devops chap eval --model-name http://host.docker.internal:PORT \
  --dataset-csv laos_subset.csv --output-file my-model.nc
```

It worked when the line says `chap finished; it wrote my-model.nc`. For the
data, see option 20.

To test it through chap-core, commit the model directory first. Then start
it from a deployment directory, such as `mychap` from option 2, with
`chap-devops chapkit mlproject run ../my-model`. In a second terminal, in that
deployment directory:

```sh
chap-devops models configs sync my-model
chap-devops models test my-model --backtest
```

Use the `name` in `MLproject` as the id. It worked when the last line says
`1 of 1 model passes`.

More: [An MLproject model, with no image to build](./use-cases/mlproject-model.md).

## What to try next without DHIS2

Option 2 has no user interface, but the same evaluation the Modeling App runs
works from the terminal. In the `mychap` folder:

```sh
chap-devops models test --all --backtest
chap-devops jobs
```

The first line creates the configured models that chap-core needs, then makes
every enabled model train, forecast and run an evaluation through chap-core on
sample data the model generates itself, and prints the scores. Under each
row, the line `configured model: NAME (id N)` names the configured model that
the backtest used. For Simple Multistep
it takes under a minute. If every model was skipped, it says `warning: no model
was tested` and exits non-zero; the line under each row says why. `chap-devops jobs` lists what
chap-core did. To see it in the Modeling App instead, add DHIS2 to the same
folder (8 GB of memory for Docker):

```sh
chap-devops components enable dhis2
chap-devops up
```

then continue as in option 1 from `chap-devops dhis2 connect`. See
[Growing a deployment](./use-cases/growing.md).

## Getting climate data

For options 3, 6 and 10, once OCS is `up`. OCS covers Laos until the person
changes the extent (the `--ocs-name`, `--ocs-country` and `--ocs-bbox` flags of
`chap-devops init`). Six years of monthly rainfall, public, no account needed; it
takes a few minutes and the command waits:

```sh
curl -X POST http://localhost:8790/ingestions -H 'Content-Type: application/json' -d '{"dataset_id": "chirps3_precipitation_monthly", "start": "2019-01", "end": "2024-12"}'
```

It worked when the answer contains `"status":"completed"`.
`curl http://localhost:8790/data-sources` lists every other dataset;
temperature (`era5land_temperature_monthly`) needs a Copernicus account, whose
key goes in `.env`. To get the values out per province as the CSV Chap reads,
or as a NetCDF file, follow
[Getting it out](./use-cases/ocs-alone.md#getting-it-out) step by step.

## Every option: stop, start, remove

```sh
chap-devops down                    # stop everything; the data is kept
chap-devops up                      # start it again
chap-devops down --volumes --yes    # stop and delete all the data
```

After `chap-devops down --volumes --yes` the folder can be deleted.

All the options use the same ports, so only one of them runs at a time
(option 19 is the exception). While another deployment is up, `chap-devops init`
warns that a port `is already in use on this machine`. You can ignore that
warning if you stop the other one before `chap-devops up`.

When `chap-devops up` says a port is used by another deployment and asks whether to
stop it, answer `y`. The other one keeps its data, and `chap-devops up` in its
folder starts it again. `chap-devops up --replace` is the same `y` given in advance.
Without a terminal, `chap-devops up` does not ask. It stops with
`error: 1 host port this deployment needs is already in use; nothing was started`
and names `chap-devops up --replace`; ask the person before you run it.

While the other one is up, `chap-devops status` in this folder says this
deployment's chap-core is not running and names the one that is.
