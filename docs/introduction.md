# Introduction

[Chap](https://chap.dhis2.org) forecasts
cases of climate-sensitive diseases such as dengue and malaria. It learns from
past case counts together with climate data and population, scores how well
each forecasting model would have done on the past, and predicts the coming
months per district or province. It is often used together with DHIS2,
through the Modeling App. [Your first forecast in the Modeling App](./modeling-app.md)
shows it end to end on demo data.

`chap-devops` deploys and manages [Chap](https://chap.dhis2.org) and the services
around it, as Docker Compose deployments:
[chap-core](https://github.com/dhis2-chap/chap-core), the forecasting model
services published in the
[Chap Marketplace](https://github.com/dhis2-chap/chap-marketplace),
[Open Climate Service](https://github.com/dhis2/open-climate-service) (OCS) with
an S3-compatible object store, and [DHIS2](https://dhis2.org).

They deploy together or any of them on its own. A deployment is made of
**components** (chap-core, OCS, the object store, DHIS2) plus the model
services you enable. chap-core is on unless you leave it out, so the default is
a Chap deployment. `chap-devops init --only ocs,s3` is an OCS server,
`chap-devops init --only dhis2` a DHIS2, and
`chap-devops init --only none --models ID` one model service answering on its own
port. [Use cases](./use-cases.md) walks through each shape, and
[Components](./components.md) is the reference.

## The name

`chap-devops` is the development and operations tool of Chap. The name says
the two halves of the work it does:

- **Operations:** it deploys Chap and the services around it, keeps them up to
  date, backs them up, and says what is wrong when something stops (`init`,
  `up`, `update`, `backup`, `status`, `doctor`).
- **Development:** it makes and tests model projects, and runs the chap and
  chapkit CLIs with no Python or R on the machine (`models new`,
  `models test`, `chap`, `chapkit`).

The command has a short form: `cdx` is the same command as `chap-devops`. The install
script puts it next to `chap-devops`.

## What it does, in one screen

One command writes a self-contained deployment directory: the base services
(chap-core, its worker, Valkey and PostgreSQL), one Compose overlay per enabled
model, an `.env` file, and a `.chap-devops/` directory that records exactly what the
deployment is meant to be.

```sh
chap-devops init mychap --models chapkit_simple_multistep_model   # writes the deployment directory
cd mychap
chap-devops up                             # sync the compose files, docker compose up -d
chap-devops status                         # chap-core health and registered models
chap-devops ui                             # browse the marketplace, toggle models
chap-devops up                             # apply what the browser changed
chap-devops update                         # move the pins to what upstream publishes now
chap-devops restart                        # apply them to the services that are running
```

Other shapes:

```sh
chap-devops init mychap --with ocs         # Chap with Open Climate Service beside it
chap-devops init climate --only ocs,s3     # OCS alone, with no Chap
```

A new model:

```sh
chap-devops models new my-model            # a model project, from a template
```

After `init`, `chap-devops` is two things at once: a thin wrapper around
`docker compose` that always passes the explicit `-f` list, and a model manager
that can add or remove models without you hand-editing YAML.

Every command takes `--json` for machine-readable output, and every command
ends with a line saying what it did or found. Empty output is a bug.

## The three words to remember

- **`chap-devops up`** starts what is on disk. It renders the compose files from
  `.chap-devops/`, checks the host ports, and runs `docker compose up -d`. It never
  moves a pin. Only `chap-devops up --pull` downloads a newer build, and only for an
  image on a moving tag such as the `main` tag of OCS.
- **`chap-devops update`** fetches newer versions. It asks the marketplace and the
  chap-core release feed what they publish today, moves the pins that follow a
  channel or a release, and pulls the images. It never touches a container: it
  ends by naming the services that are now running something out of date.
- **`chap-devops restart`** applies them to the services that are running. It
  recreates the containers that no longer match the files and leaves the rest
  alone, and it changes no file and no pin.

Everything else follows from that split. `chap-devops up`, `chap-devops restart`,
`chap-devops models expose` and `chap-devops models unexpose` are safe on a deployment
running a build you do not want moved. `chap-devops update`, `chap-devops models enable`,
`chap-devops models add` and `chap-devops components enable --tag` are the commands that
move a version; see [What moves a pin](./updating.md#what-moves-a-pin).

## The pure server promise

A machine that runs Chap with `chap-devops` needs Docker and one binary. Nothing
else: no Python, no `uv`, no checkout of chap-core, no build step on the
server. The Linux releases are static musl builds, so the same file runs on any
distribution.

The compose files `chap-devops` writes are plain Compose files with nothing
`chap-devops`-specific in them, so

```sh
docker compose -f compose.yml -f compose.chap-devops.yml -f compose.marketplace.yml up -d
```

works on a machine that has never had `chap-devops` installed. That is the point of
generating them rather than keeping the deployment inside the tool.

That is the list for chap-core and models. A component adds its own file
before `compose.marketplace.yml`, for example `-f compose.ocs.yml`. A
deployment without chap-core has no `compose.yml` or `compose.chap-devops.yml`.

## Where to go next

- [Install](./install.md) puts the binary on the machine.
- [Quickstart](./quickstart.md) walks a first deployment end to end.
- [Your first forecast in the Modeling App](./modeling-app.md) uses one.
- [Concepts](./concepts.md) explains the deployment directory, intent versus
  artifacts, and the pins.
- [Commands](./commands.md) is the tour; [Command reference](./reference.md) is
  the generated, complete list.

`chap-devops` is licensed under the AGPL-3.0, like chap-core. The full text is in
`LICENSE` at the repository root.
