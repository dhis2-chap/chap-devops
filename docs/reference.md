# Command reference

Generated from the `--help` texts by `make docs-reference`; edit
`src/cli.rs` and run that target rather than editing this file.

Every command accepts the options listed under [chap-devops](#chap-devops), `--json`
included. The registry options (`--registry-url`, `--offline`, `--cache-dir`)
show only in the help of the commands that use them, and each such command
names them below. Commands that need a deployment directory are hidden from
`chap-devops --help` outside one, but they are all listed here.

## chap-devops

deploy Chap (climate-informed disease forecasting) and its services.

```text
Usage: chap-devops [OPTIONS] <COMMAND>
```

| Global option | Description |
| --- | --- |
| `--json` | Print machine-readable JSON instead of human output. |
| `--no-color` | Never colour the output (NO_COLOR does the same). |
| `-v, --verbose` | Show the hints; -vv also shows the commands and requests that run. |
| `-d, --debug` | -vv plus the raw responses. |
| `-C, --project-dir <DIR>` | Deployment directory, or any directory inside one. Default: `.`. |
| `--registry-url <URL>` | URL of the marketplace registry index. Default: `https://raw.githubusercontent.com/dhis2-chap/chap-marketplace/main/registry.yaml`. |
| `--offline` | Never touch the network; use what is cached or on this machine. |
| `--cache-dir <DIR>` | Cache directory: the registry snapshot and the self-update downloads. |

Subcommands: [`chap-devops init`](#chap-devops-init), [`chap-devops components`](#chap-devops-components), [`chap-devops models`](#chap-devops-models), [`chap-devops registry`](#chap-devops-registry), [`chap-devops sync`](#chap-devops-sync), [`chap-devops update`](#chap-devops-update), [`chap-devops auth`](#chap-devops-auth), [`chap-devops dhis2`](#chap-devops-dhis2), [`chap-devops up`](#chap-devops-up), [`chap-devops down`](#chap-devops-down), [`chap-devops restart`](#chap-devops-restart), [`chap-devops status`](#chap-devops-status), [`chap-devops logs`](#chap-devops-logs), [`chap-devops open`](#chap-devops-open), [`chap-devops ui`](#chap-devops-ui), [`chap-devops run`](#chap-devops-run), [`chap-devops ps`](#chap-devops-ps), [`chap-devops stop`](#chap-devops-stop), [`chap-devops jobs`](#chap-devops-jobs), [`chap-devops api`](#chap-devops-api), [`chap-devops bench`](#chap-devops-bench), [`chap-devops chap`](#chap-devops-chap), [`chap-devops chapkit`](#chap-devops-chapkit), [`chap-devops backup`](#chap-devops-backup), [`chap-devops doctor`](#chap-devops-doctor), [`chap-devops docker`](#chap-devops-docker), [`chap-devops top`](#chap-devops-top), [`chap-devops cleanup`](#chap-devops-cleanup), [`chap-devops self`](#chap-devops-self), [`chap-devops completions`](#chap-devops-completions)

## chap-devops init

Create a deployment directory: compose files, .env and .chap-devops/.

```text
Usage: chap-devops init [OPTIONS] [DIR]
```

| Argument | Description |
| --- | --- |
| `<DIR>` | Directory to create. Default: `.`. |
| `--models <SPEC>` | Models to enable: none, all, or a list of ids. Default: `none`. |
| `--interactive` | Select the models in the browser instead of with --models. |
| `--chap-tag <TAG>` | chap-core image tag: latest, master, dev or vX.Y.Z. Default: `latest`. |
| `--force` | Overwrite an existing deployment in the target directory. |
| `--api-port <PORT>` | Host port to publish chap-core's API on. Default: `8700`. |
| `--port-base <PORT>` | Lowest host port a model may be published on. Default: `5001`. |
| `--api-token <TOKEN>` | Protect the API with a token (generated when no value is given). |
| `--no-env` | Do not write a .env file. |
| `--fresh-env` | Regenerate .env with a new database password. |
| `--source <PATH>` | Build chap-core from the chap-core checkout at PATH. |
| `--with <LIST>` | Components to add: ocs, s3, dhis2. |
| `--without <LIST>` | Components to omit: chap-core, ocs, s3, dhis2. |
| `--only <LIST>` | Exactly these components and nothing else; `none` for models alone. |
| `--adopt <DIR>` | Take over the chap-admin deployment in DIR, with its database. |
| `--chap-core-url <URL>` | Register the models with the chap-core at URL instead of running one. |
| `--ocs-base-url <URL>` | Public URL OCS is reached at, for a proxied instance. |
| `--ocs-port <PORT\|none>` | Host port to publish OCS on, or none to keep it internal. |
| `--s3-port <PORT\|none>` | Host port to publish the object store on, or none. |
| `--dhis2-port <PORT\|none>` | Host port to publish DHIS2 on, or none to keep it internal. |
| `--dhis2-seed <SPEC>` | Dump to seed the DHIS2 database from: default, none, URL or path. |
| `--dhis2-seed-password <PW>` | Give every DHIS2 user this password (default: district). |
| `--dhis2-tag <TAG>` | DHIS2 version to run: an image tag such as 2.41, 2.42 or 2.43.1. |
| `--dhis2-image <REPO>` | DHIS2 image repository, e.g. dhis2/core-dev for unreleased versions. |
| `--ocs-read-only` | Refuse ingestion over HTTP on the new OCS instance. |
| `--ocs-name <NAME>` | Country or region the OCS instance covers, e.g. Malawi. |
| `--ocs-country <CODE>` | ISO 3166-1 alpha-3 country code for the OCS extent. |
| `--ocs-bbox <BBOX>` | OCS extent as xmin,ymin,xmax,ymax in degrees. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops components

Show and change what this deployment is made of.

```text
Usage: chap-devops components [OPTIONS] <COMMAND>
```

Subcommands: [`chap-devops components list`](#chap-devops-components-list), [`chap-devops components enable`](#chap-devops-components-enable), [`chap-devops components disable`](#chap-devops-components-disable)

## chap-devops components list

List every component and whether this deployment has it.

```text
Usage: chap-devops components list [OPTIONS]
```

## chap-devops components enable

Enable a component, or change the settings of an enabled one.

```text
Usage: chap-devops components enable [OPTIONS] <NAME>
```

| Argument | Description |
| --- | --- |
| `<NAME>` | Component name: ocs, s3, dhis2 or chap-core. |
| `--port <PORT\|none>` | Host port to publish on, or none to keep it internal. |
| `--base-url <URL>` | Public URL OCS is reached at, for a proxied instance. |
| `--read-only` | Refuse ingestion over HTTP on this OCS instance. |
| `--read-write` | Allow ingestion over HTTP again. |
| `--url <URL>` | chap-core only: use the chap-core at URL instead of running one. |
| `--tag <TAG>` | dhis2 only: the DHIS2 version to run, an image tag such as 2.43. |
| `--image <REPO>` | dhis2 only: the image repository, e.g. dhis2/core-dev. |
| `--seed <SPEC>` | dhis2 only: dump for a new database: default, none, URL or path. |
| `--models-host <HOST>` | With --url: the host that chap-core calls the models back at. |
| `--ocs-name <NAME>` | Country or region the OCS instance covers, e.g. Malawi. |
| `--ocs-country <CODE>` | ISO 3166-1 alpha-3 country code for the OCS extent. |
| `--ocs-bbox <BBOX>` | OCS extent as xmin,ymin,xmax,ymax in degrees. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops components disable

Disable a component and remove its compose file.

```text
Usage: chap-devops components disable [OPTIONS] <NAME>
```

| Argument | Description |
| --- | --- |
| `<NAME>` | Component name: ocs, s3, dhis2 or chap-core. |
| `--purge` | Delete the component's data volume as well. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models

Browse and manage marketplace models.

```text
Usage: chap-devops models [OPTIONS] <COMMAND>
```

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

Subcommands: [`chap-devops models list`](#chap-devops-models-list), [`chap-devops models search`](#chap-devops-models-search), [`chap-devops models info`](#chap-devops-models-info), [`chap-devops models test`](#chap-devops-models-test), [`chap-devops models configs`](#chap-devops-models-configs), [`chap-devops models new`](#chap-devops-models-new), [`chap-devops models add`](#chap-devops-models-add), [`chap-devops models remove`](#chap-devops-models-remove), [`chap-devops models enable`](#chap-devops-models-enable), [`chap-devops models disable`](#chap-devops-models-disable), [`chap-devops models expose`](#chap-devops-models-expose), [`chap-devops models unexpose`](#chap-devops-models-unexpose)

## chap-devops models list

List marketplace models.

```text
Usage: chap-devops models list [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--all` | Include every entry, starters as well as models. |
| `--starters` | List only starters. |
| `--enabled` | List only the models enabled in this deployment. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models search

Search the marketplace by id, name or summary.

```text
Usage: chap-devops models search [OPTIONS] <QUERY>
```

| Argument | Description |
| --- | --- |
| `<QUERY>` | Text to look for; matching is case-insensitive. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models info

Show everything known about one model.

```text
Usage: chap-devops models info [OPTIONS] <ID>
```

| Argument | Description |
| --- | --- |
| `<ID>` | Marketplace id or service id. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models test

Make a model train and predict, and say whether it could.

```text
Usage: chap-devops models test [OPTIONS] [ID]...
```

| Argument | Description |
| --- | --- |
| `<ID>...` | Marketplace ids or service ids of the models to test. |
| `--all` | Test every model this deployment has enabled. |
| `--backtest` | Run a backtest through chap-core instead of the model's own test. |
| `--config <NAME>` | Variant name of the configured model to backtest, as in `models configs`. |
| `--seed <N>` | Seed for the generated data, so a run can be repeated. |
| `--timeout <SECONDS>` | Time limit for the test of one model, in seconds. |
| `--configs <N>` | Configs that `chapkit test` creates for each model. |
| `--trainings <N>` | Trainings for each config. |
| `--predictions <N>` | Predictions for each trained model. |
| `--rows <N>` | Rows of training data (locations times periods). |
| `--predict-rows <N>` | Rows of the history and of the future data for each prediction. |
| `--keep` | Keep what the test created instead of deleting it. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models configs

List, add, archive and sync the configured models of each model.

```text
Usage: chap-devops models configs [OPTIONS] [ID]
       chap-devops models configs <COMMAND>
```

| Argument | Description |
| --- | --- |
| `<ID>` | Marketplace id or service id; all enabled models when none is given. |
| `--all` | Show the archived configured models too. |

Subcommands: [`chap-devops models configs list`](#chap-devops-models-configs-list), [`chap-devops models configs add`](#chap-devops-models-configs-add), [`chap-devops models configs update`](#chap-devops-models-configs-update), [`chap-devops models configs archive`](#chap-devops-models-configs-archive), [`chap-devops models configs export`](#chap-devops-models-configs-export), [`chap-devops models configs sync`](#chap-devops-models-configs-sync)

## chap-devops models configs list

List the configured models chap-core has of each enabled model.

```text
Usage: chap-devops models configs list [OPTIONS] [ID]
```

| Argument | Description |
| --- | --- |
| `<ID>` | Marketplace id or service id; all enabled models when none is given. |
| `--all` | Show the archived configured models too. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models configs add

Add a configured model of a model to chap-core.

```text
Usage: chap-devops models configs add [OPTIONS] <ID>
```

| Argument | Description |
| --- | --- |
| `<ID>` | Marketplace id or service id. |
| `--name <NAME>` | Variant name, which the Modeling App shows in brackets. |
| `--set <KEY=VALUE>` | Value of one user option; repeat for more than one. |
| `--covariates <A,B>` | Additional covariates, separated by commas. |
| `--from <FILE>` | Add every configuration in a file of the marketplace format. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models configs update

Change the values of a configured model of a model.

```text
Usage: chap-devops models configs update [OPTIONS] <ID> <NAME>
```

| Argument | Description |
| --- | --- |
| `<ID>` | Marketplace id or service id. |
| `<NAME>` | Variant name of the configured model. |
| `--set <KEY=VALUE>` | New value of one user option; repeat for more than one. |
| `--unset <KEY>` | Remove the value of one option, so the model default applies. |
| `--covariates <A,B>` | Additional covariates, separated by commas, in place of the old ones. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models configs archive

Archive a configured model of a model.

```text
Usage: chap-devops models configs archive [OPTIONS] <ID> <NAME>
```

| Argument | Description |
| --- | --- |
| `<ID>` | Marketplace id or service id. |
| `<NAME>` | Variant name of the configured model. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models configs export

Write the configured models of a model in the marketplace format.

```text
Usage: chap-devops models configs export [OPTIONS] [ID]
```

| Argument | Description |
| --- | --- |
| `<ID>` | Marketplace id or service id; may be left out with one enabled model. |
| `--out <FILE>` | File to write instead of standard output. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models configs sync

Create the configured models that chap-core needs to run each model.

```text
Usage: chap-devops models configs sync [OPTIONS] [ID]...
```

| Argument | Description |
| --- | --- |
| `<ID>...` | Marketplace ids or service ids; all enabled models when none is given. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models new

Create a model project from a template.

```text
Usage: chap-devops models new [OPTIONS] <DIR>
```

| Argument | Description |
| --- | --- |
| `<DIR>` | Directory to create; its name becomes the model name. |
| `--template <TYPE>` | The kind of project and its base image. Values: `fn-py`, `shell-py`, `shell-r`, `shell-r-tidyverse`, `shell-r-inla`, `mlproject-py`, `mlproject-r`, `mlproject-r-tidyverse`, `mlproject-r-inla`. Default: `fn-py`. |
| `--with-validation` | Add $validate hooks to main.py (chapkit service types only). |

## chap-devops models add

Add a model the marketplace does not list.

```text
Usage: chap-devops models add [OPTIONS] <SOURCE>
```

| Argument | Description |
| --- | --- |
| `<SOURCE>` | GitHub repository URL, image reference, or local image name:tag. |
| `--id <ID\|auto>` | Identifier to record the model under, or auto for a free one. |
| `--service-id <ID>` | Compose service name, which must match the service's own id. |
| `--name <NAME>` | Name to show in the listings. |
| `--port <PORT\|auto>` | Host port to publish the model on, or auto for a free one. |
| `--bind <ADDR>` | Host address to publish the port on, e.g. 127.0.0.1 for this machine. |
| `--data-dir <PATH>` | Data directory inside the container. |
| `--user <USER>` | User the container runs as, as user:group. |
| `--runtime-amd64` | Record the image as published for amd64 only. |
| `--allow-starter` | Enable a starter even though it is not a model. |
| `-y, --yes` | Add an image that nobody reviewed without asking first. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models remove

Remove a model that was added with models add.

```text
Usage: chap-devops models remove [OPTIONS] <ID>
```

| Argument | Description |
| --- | --- |
| `<ID>` | Id of a model added with models add. |
| `--purge` | Delete the model's data volume as well. |
| `--keep-in-chap` | Leave the model and its configured models live in chap-core. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models enable

Enable a model and write its compose overlay.

```text
Usage: chap-devops models enable [OPTIONS] <ID>...
```

| Argument | Description |
| --- | --- |
| `<ID>...` | Marketplace ids or service ids, separated by spaces or commas. |
| `--channel <CHANNEL>` | Follow a release channel instead of pinning a version. Values: `stable`, `latest`. |
| `--version <VERSION>` | Pin an exact version from the model's versions list. |
| `--port <PORT\|auto>` | Host port to publish the model on, or auto for a free one. |
| `--bind <ADDR>` | Host address to publish the port on, e.g. 127.0.0.1 for this machine. |
| `--data-dir <PATH>` | Data directory inside the container. |
| `--user <USER>` | User the container runs as, as user:group. |
| `--allow-starter` | Enable a starter even though it is not a model. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models disable

Disable a model and remove its compose overlay.

```text
Usage: chap-devops models disable [OPTIONS] <ID>
```

| Argument | Description |
| --- | --- |
| `<ID>` | Marketplace id or service id. |
| `--purge` | Delete the model's data volume as well. |
| `--retire` | Retire the model in chap-core too, with its configured models. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models expose

Publish a host port for an enabled model.

```text
Usage: chap-devops models expose [OPTIONS] <ID>
```

| Argument | Description |
| --- | --- |
| `<ID>` | Marketplace id or service id. |
| `--port <PORT\|auto>` | Host port to publish on, or auto for the lowest free one. |
| `--bind <ADDR>` | Host address to publish the port on, e.g. 127.0.0.1 for this machine. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops models unexpose

Remove the host port of an enabled model.

```text
Usage: chap-devops models unexpose [OPTIONS] <ID>
```

| Argument | Description |
| --- | --- |
| `<ID>` | Marketplace id or service id. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops registry

Inspect and refresh the marketplace registry.

```text
Usage: chap-devops registry [OPTIONS] <COMMAND>
```

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

Subcommands: [`chap-devops registry update`](#chap-devops-registry-update), [`chap-devops registry show`](#chap-devops-registry-show)

## chap-devops registry update

Fetch the registry from the network and refresh the cache.

```text
Usage: chap-devops registry update [OPTIONS]
```

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops registry show

Show where the registry was loaded from and what it holds.

```text
Usage: chap-devops registry show [OPTIONS]
```

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops sync

Render the compose files from .chap-devops/.

```text
Usage: chap-devops sync [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--check` | Write nothing; exit non-zero if anything would change. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops update

Move the pins to what upstream publishes now, and pull.

```text
Usage: chap-devops update [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--dry-run` | Show what would change: no pull, and nothing written. |
| `--pin-chap-core` | Pin a moving chap-core tag to the newest release. |
| `--chap-tag <TAG>` | Move chap-core to this tag: vX.Y.Z, latest, master or dev. |
| `--list-tags` | List the chap-core tags you can move to, newest first. |
| `--yes` | Answer yes to the confirmation a backwards move asks for. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops auth

Enable or disable API authentication, and show the token.

```text
Usage: chap-devops auth [OPTIONS] <COMMAND>
```

Subcommands: [`chap-devops auth show`](#chap-devops-auth-show), [`chap-devops auth token`](#chap-devops-auth-token), [`chap-devops auth enable`](#chap-devops-auth-enable), [`chap-devops auth disable`](#chap-devops-auth-disable), [`chap-devops auth rotate`](#chap-devops-auth-rotate)

## chap-devops auth show

Say whether the API is protected, and by which token.

```text
Usage: chap-devops auth show [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--reveal` | Print the API token. |

## chap-devops auth token

Print the API token alone, for a script to capture.

```text
Usage: chap-devops auth token [OPTIONS]
```

## chap-devops auth enable

Enable authentication: write both secrets and sync.

```text
Usage: chap-devops auth enable [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--token <TOKEN>` | Use this API token instead of generating one. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops auth disable

Disable authentication; keep both values as comments.

```text
Usage: chap-devops auth disable [OPTIONS]
```

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops auth rotate

Replace both secrets with new generated ones.

```text
Usage: chap-devops auth rotate [OPTIONS]
```

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops dhis2

Let the DHIS2 Modeling App reach this deployment's Chap.

```text
Usage: chap-devops dhis2 [OPTIONS] <COMMAND>
```

Subcommands: [`chap-devops dhis2 show`](#chap-devops-dhis2-show), [`chap-devops dhis2 route`](#chap-devops-dhis2-route), [`chap-devops dhis2 analytics`](#chap-devops-dhis2-analytics), [`chap-devops dhis2 apps`](#chap-devops-dhis2-apps), [`chap-devops dhis2 connect`](#chap-devops-dhis2-connect), [`chap-devops dhis2 use`](#chap-devops-dhis2-use)

## chap-devops dhis2 show

Say what DHIS2 has: the chap route, analytics and the apps.

```text
Usage: chap-devops dhis2 show [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--user <NAME>` | DHIS2 user to authenticate as, with a password rather than a token. |
| `--wait <SECONDS>` | Seconds to wait for DHIS2's API; 1200 local, 60 external by default. |

## chap-devops dhis2 route

Point DHIS2's chap route at this deployment's chap-core.

```text
Usage: chap-devops dhis2 route [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--user <NAME>` | DHIS2 user to authenticate as, with a password rather than a token. |
| `--wait <SECONDS>` | Seconds to wait for DHIS2's API; 1200 local, 60 external by default. |

## chap-devops dhis2 analytics

Generate DHIS2's analytics tables and wait for them.

```text
Usage: chap-devops dhis2 analytics [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--user <NAME>` | DHIS2 user to authenticate as, with a password rather than a token. |
| `--wait <SECONDS>` | Seconds to wait for DHIS2's API; 1200 local, 60 external by default. |
| `--timeout <SECONDS>` | Seconds to wait for the analytics run to finish. Default: `3600`. |
| `--no-wait` | Start the run and return without a wait for it to finish. |

## chap-devops dhis2 apps

Install the Modeling and Climate apps from the App Hub.

```text
Usage: chap-devops dhis2 apps [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--user <NAME>` | DHIS2 user to authenticate as, with a password rather than a token. |
| `--wait <SECONDS>` | Seconds to wait for DHIS2's API; 1200 local, 60 external by default. |

Registry options: `--offline` (see [chap-devops](#chap-devops)).

## chap-devops dhis2 connect

The route, the apps, then analytics; the route only on an external DHIS2.

```text
Usage: chap-devops dhis2 connect [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--user <NAME>` | DHIS2 user to authenticate as, with a password rather than a token. |
| `--wait <SECONDS>` | Seconds to wait for DHIS2's API; 1200 local, 60 external by default. |
| `--timeout <SECONDS>` | Seconds to wait for the analytics run to finish. Default: `3600`. |
| `--no-wait` | Start the run and return without a wait for it to finish. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops dhis2 use

Use a DHIS2 that runs elsewhere, or show which DHIS2 is used.

```text
Usage: chap-devops dhis2 use [OPTIONS] [URL]
```

| Argument | Description |
| --- | --- |
| `<URL>` | DHIS2's URL, as this machine reaches it. |
| `--chap-url <URL>` | chap-core's URL, as that DHIS2 reaches it; the route points here. |
| `--clear` | Forget the external DHIS2 and use the dhis2 component again. |

## chap-devops up

Sync, then start Chap (docker compose up).

```text
Usage: chap-devops up [OPTIONS] [EXTRA]...
```

| Argument | Description |
| --- | --- |
| `-a, --attach, --foreground` | Run in the foreground and stream all logs (Ctrl-C stops Chap). |
| `--pull` | Pull every image first (docker compose up --pull always). |
| `--no-preflight` | Do not check the host ports first. |
| `--replace` | First stop the other chap-devops deployments that hold these ports. |
| `--wait` | Return only once chap-core, the models, OCS and DHIS2 answer. |
| `--timeout <SECONDS>` | How long --wait waits before it fails, in seconds. Default: `300`. |
| `<EXTRA>...` | Extra arguments passed through to docker compose up. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops down

Stop Chap (docker compose down).

```text
Usage: chap-devops down [OPTIONS] [EXTRA]...
```

| Argument | Description |
| --- | --- |
| `--volumes` | Also remove this deployment's volumes, destroying its data. |
| `-y, --yes` | Skip the confirmation. |
| `<EXTRA>...` | Extra arguments passed through to docker compose down. |

## chap-devops restart

Recreate the services whose image or configuration changed.

```text
Usage: chap-devops restart [OPTIONS] [SERVICE]...
```

| Argument | Description |
| --- | --- |
| `--all` | Recreate the named services even when nothing changed. |
| `<SERVICE>...` | Services to restart; the whole deployment when omitted. |

## chap-devops status

Show chap-core health and which models registered.

```text
Usage: chap-devops status [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--url <URL>` | Base URL of the chap-core API. |
| `--timeout <SECONDS>` | Request timeout in seconds. Default: `5`. |

## chap-devops logs

Show container logs (docker compose logs).

```text
Usage: chap-devops logs [OPTIONS] [SERVICE]...
```

| Argument | Description |
| --- | --- |
| `-f, --follow` | Show new output when it arrives. |
| `--tail <N>` | Show only the last N lines of each service's log. |
| `<SERVICE>...` | Services to show logs for; all of them when omitted. |

## chap-devops open

Open a component's web interface in a browser.

```text
Usage: chap-devops open [OPTIONS] [NAME]
```

| Argument | Description |
| --- | --- |
| `<NAME>` | Component to open (chap-core, ocs, dhis2), or an enabled model's id. |
| `--no-browser` | Print the address instead of opening a browser. |

## chap-devops ui

Open the browser: marketplace models and components.

```text
Usage: chap-devops ui [OPTIONS]
```

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops run

Start one model and print where it answers.

```text
Usage: chap-devops run [OPTIONS] <MODEL>
```

| Argument | Description |
| --- | --- |
| `<MODEL>` | Marketplace id, GitHub repository URL, image reference, or local image:tag. |
| `--port <PORT\|auto>` | Host port to publish the model on, or auto for a free one. Default: `auto`. |
| `--bind <ADDR>` | Host address to publish the port on, e.g. 0.0.0.0 for the network. |
| `--id <ID\|auto>` | Identifier to record an added model under, or auto for a free one. |
| `--group <NAME>` | Group to start it in, outside a deployment; ps and stop take it too. |
| `--allow-starter` | Start a marketplace starter, which is not a deployable model. |
| `-y, --yes` | Run an image that nobody reviewed without asking first. |
| `--no-wait` | Return once the container started, without waiting for it to answer. |
| `-a, --attach, --foreground` | Run in the foreground and stream its log (Ctrl-C stops the model). |
| `--rm` | With --attach, Ctrl-C also removes the model's data volume. |
| `--chap-core <URL>` | A chap-core elsewhere for the group's models to register with. |
| `--models-host <HOST>` | Host that chap-core calls the models back at; detected if omitted. |
| `--timeout <SECONDS>` | How long to wait for the model to answer, in seconds. Default: `300`. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops ps

List the models that run, and where each one answers.

```text
Usage: chap-devops ps [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--group <NAME>` | List this group only; every group when omitted. |

## chap-devops stop

Stop a model and remove its compose overlay; its data stays.

```text
Usage: chap-devops stop [OPTIONS] [ID]
```

| Argument | Description |
| --- | --- |
| `<ID>` | Id or service id of the model. |
| `--group <NAME>` | Group to stop in; every model in it when no id is given. |
| `--all` | Stop every model in the group, or in every group. |
| `--purge` | Delete the data volumes too, and a group left empty with them. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops jobs

List the backtests and predictions chap-core has run.

```text
Usage: chap-devops jobs [OPTIONS]
       chap-devops jobs <COMMAND>
```

| Argument | Description |
| --- | --- |
| `--status <STATUS>` | Only jobs in this status; repeat for more than one. |
| `--type <TYPE>` | Only jobs of this type, such as create_backtest. |
| `--limit <N>` | Show at most this many jobs. |

Subcommands: [`chap-devops jobs list`](#chap-devops-jobs-list), [`chap-devops jobs show`](#chap-devops-jobs-show), [`chap-devops jobs logs`](#chap-devops-jobs-logs), [`chap-devops jobs cancel`](#chap-devops-jobs-cancel), [`chap-devops jobs delete`](#chap-devops-jobs-delete)

## chap-devops jobs list

List the jobs chap-core knows about, newest first.

```text
Usage: chap-devops jobs list [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--status <STATUS>` | Only jobs in this status; repeat for more than one. |
| `--type <TYPE>` | Only jobs of this type, such as create_backtest. |
| `--limit <N>` | Show at most this many jobs. |

## chap-devops jobs show

Show everything chap-core records about one job.

```text
Usage: chap-devops jobs show [OPTIONS] <ID>
```

| Argument | Description |
| --- | --- |
| `<ID>` | Job id, or enough of its start to name one job. |

## chap-devops jobs logs

Print one job's log, which is where a failure says why.

```text
Usage: chap-devops jobs logs [OPTIONS] <ID>
```

| Argument | Description |
| --- | --- |
| `<ID>` | Job id, or enough of its start to name one job. |
| `--tail <N>` | Print only the last N lines of the log. |

## chap-devops jobs cancel

Ask chap-core to stop a job that is still running.

```text
Usage: chap-devops jobs cancel [OPTIONS] <ID>
```

| Argument | Description |
| --- | --- |
| `<ID>` | Job id, or enough of its start to name one job. |

## chap-devops jobs delete

Remove a finished job from chap-core's list.

```text
Usage: chap-devops jobs delete [OPTIONS] <ID>
```

| Argument | Description |
| --- | --- |
| `<ID>` | Job id, or enough of its start to name one job. |

## chap-devops api

Send one authenticated request to chap-core's API.

```text
Usage: chap-devops api [OPTIONS] <METHOD> <PATH>
```

| Argument | Description |
| --- | --- |
| `<METHOD>` | GET, POST, PUT, PATCH or DELETE; case does not matter. |
| `<PATH>` | Request path starting with /, query string and all. |
| `--data <JSON\|@FILE\|->` | JSON body: inline, @file, or - to read stdin. |
| `--url <URL>` | Base URL of the chap-core API. |
| `--raw` | Print the body exactly as it arrived. |
| `--timeout <SECONDS>` | Request timeout in seconds. Default: `30`. |

## chap-devops bench

Time the Modeling App's pages, or run backtests under load.

```text
Usage: chap-devops bench [OPTIONS] [BACKTEST]
       chap-devops bench <COMMAND>
```

| Argument | Description |
| --- | --- |
| `<BACKTEST>` | Backtest id to time; the newest backtest when not given. |
| `--page <PAGE>` | Pages to time, comma-separated; all pages when not given. Values: `evaluation`, `compare`, `plots`, `benchmark`, `dataset`, `prediction`. |
| `--prediction <ID>` | Prediction id for the prediction page; the newest when not given. |
| `--repeat <N>` | How many times to send each request. Default: `3`. |
| `--parallel <N>` | How many requests to have open at the same time. Default: `1`. |
| `--url <URL>` | Base URL of the chap-core API. |
| `--timeout <SECONDS>` | Request timeout in seconds. Default: `120`. |
| `--write` | Import a dataset and run backtests on it, instead of timing reads. |
| `--model <MODEL>` | Configured models to backtest, by name or id, comma-separated. |
| `--backtests <N>` | How many runs of the models to start at the same time. Default: `4`. |
| `--dataset <ID>` | Backtest this dataset id instead of a new synthetic one. |
| `--wait <SECONDS>` | Seconds to wait for the jobs to finish. Default: `3600`. |
| `--predict` | Also make a prediction from the first backtest that passed. |
| `--keep` | Keep the dataset and the backtests that the run made. |
| `--locations <N>` | Number of locations (org units). Default: `18`. |
| `--periods <N>` | Number of periods for each location. Default: `104`. |
| `--period-type <TYPE>` | Length of one period. Values: `week`, `month`. Default: `week`. |
| `--cases <N>` | Typical number of cases in one location and period. Default: `100`. |
| `--covariates <N>` | Number of covariates: rainfall, mean_temperature, population, then more. Default: `3`. |
| `--format <FORMAT>` | How to send the observations: rows as the Modeling App, or columns. Values: `rows`, `columns`. Default: `rows`. |
| `--seed <N>` | Seed for the random values; the same seed gives the same dataset. Default: `1`. |

Subcommands: [`chap-devops bench dataset`](#chap-devops-bench-dataset)

## chap-devops bench dataset

Write a synthetic dataset as CSV and GeoJSON, or import it into chap-core.

```text
Usage: chap-devops bench dataset [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--out <DIR>` | Directory for the CSV and GeoJSON files; `.` when there is no --import. |
| `--import` | Import the dataset into chap-core and wait until it is stored. |
| `--name <NAME>` | Name of the dataset and of its files; made from the options if not given. |
| `--url <URL>` | Base URL of the chap-core API to import into. |
| `--timeout <SECONDS>` | Seconds to wait for chap-core to store the dataset. Default: `600`. |
| `--locations <N>` | Number of locations (org units). Default: `18`. |
| `--periods <N>` | Number of periods for each location. Default: `104`. |
| `--period-type <TYPE>` | Length of one period. Values: `week`, `month`. Default: `week`. |
| `--cases <N>` | Typical number of cases in one location and period. Default: `100`. |
| `--covariates <N>` | Number of covariates: rainfall, mean_temperature, population, then more. Default: `3`. |
| `--format <FORMAT>` | How to send the observations: rows as the Modeling App, or columns. Values: `rows`, `columns`. Default: `rows`. |
| `--seed <N>` | Seed for the random values; the same seed gives the same dataset. Default: `1`. |

## chap-devops chap

Run the chap CLI in a container, with no Python or uv installed.

```text
Usage: chap-devops chap [OPTIONS] [CHAP_ARGS]...
```

| Argument | Description |
| --- | --- |
| `--tag <TAG>` | chap-core image tag; the deployment's tag or the newest release if omitted. |
| `--image <IMAGE>` | Image to run in; chosen from --model-name when omitted. Values: `core`, `worker`. |
| `--group <NAME>` | `chap-devops run` group whose network the container joins. |
| `--docker` | Give the container the docker socket, for docker_env models. |
| `--stop` | Stop the model this run started, once chap is done. |
| `--timeout <SECONDS>` | How long a model this run starts may take to answer, in seconds. Default: `300`. |
| `<CHAP_ARGS>...` | Arguments for chap, such as `eval --model-name URL ...`. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops chapkit

Run the chapkit CLI in the chap-core image, with no Python installed.

```text
Usage: chap-devops chapkit [OPTIONS] [CHAPKIT_ARGS]...
```

| Argument | Description |
| --- | --- |
| `--tag <TAG>` | chap-core image tag; the deployment's tag or the newest release if omitted. |
| `--image <IMAGE>` | Image for `mlproject run`, a short name or a name; alone, lists them. |
| `<CHAPKIT_ARGS>...` | Arguments for chapkit, such as `mlproject migrate DIR`. |

## chap-devops backup

Create or restore a backup of this deployment.

```text
Usage: chap-devops backup [OPTIONS] <COMMAND>
```

Subcommands: [`chap-devops backup create`](#chap-devops-backup-create), [`chap-devops backup restore`](#chap-devops-backup-restore)

## chap-devops backup create

Write a tar.gz of the database, the data and the files.

```text
Usage: chap-devops backup create [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--out <PATH>` | Where to write the archive: a file, or a directory. |
| `--no-db` | Do not put the chap-core database in the archive. |
| `--no-models` | Do not put the model data volumes in the archive. |
| `--no-components` | Do not put the component data volumes in the archive. |

## chap-devops backup restore

Put a deployment back from a backup archive.

```text
Usage: chap-devops backup restore [OPTIONS] <ARCHIVE>
```

| Argument | Description |
| --- | --- |
| `<ARCHIVE>` | The tar.gz written by chap-devops backup create. |
| `--yes` | Skip the confirmation. |
| `--files-only` | Restore only the deployment files; no Docker needed. |
| `--db-only` | Restore only the chap-core database. |
| `--no-models` | Leave the model data volumes as they are. |
| `--no-components` | Leave the component data volumes as they are. |
| `--adopt-identity` | Use the compose project name from the archive. |
| `--no-start` | Do not start the deployment at the end. |

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops doctor

Run a checklist over this machine and this deployment.

```text
Usage: chap-devops doctor [OPTIONS]
```

Registry options: `--registry-url <URL>`, `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops docker

Use Docker directly: containers, images, compose.

```text
Usage: chap-devops docker [OPTIONS] <COMMAND>
```

Subcommands: [`chap-devops docker ps`](#chap-devops-docker-ps), [`chap-devops docker pull`](#chap-devops-docker-pull), [`chap-devops docker exec`](#chap-devops-docker-exec), [`chap-devops docker run`](#chap-devops-docker-run), [`chap-devops docker config`](#chap-devops-docker-config)

## chap-devops docker ps

List the containers this deployment is running.

```text
Usage: chap-devops docker ps [OPTIONS] [EXTRA]...
```

| Argument | Description |
| --- | --- |
| `<EXTRA>...` | Extra arguments passed through to docker compose ps. |

## chap-devops docker pull

Download the pinned images into the local Docker daemon.

```text
Usage: chap-devops docker pull [OPTIONS]
```

## chap-devops docker exec

Run a command inside one of the running containers.

```text
Usage: chap-devops docker exec [OPTIONS] <SERVICE> [CMD]...
```

| Argument | Description |
| --- | --- |
| `<SERVICE>` | Service to run the command in. |
| `<CMD>...` | Command to run; a shell (sh) when omitted. |

## chap-devops docker run

Run any docker compose command against this deployment.

```text
Usage: chap-devops docker run [OPTIONS] [ARGS]...
```

| Argument | Description |
| --- | --- |
| `<ARGS>...` | Arguments passed to docker compose after the -f list. |

## chap-devops docker config

Print every compose file merged into one document.

```text
Usage: chap-devops docker config [OPTIONS] [EXTRA]...
```

| Argument | Description |
| --- | --- |
| `<EXTRA>...` | Extra arguments passed through to docker compose config. |

## chap-devops top

Watch every chap-devops deployment on this machine as a live tree.

```text
Usage: chap-devops top [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--interval <SECONDS>` | Seconds between two looks at docker. Default: `2`. |
| `--record <FILE>` | Append CPU and memory samples to FILE as JSON lines, with no screen. |
| `--duration <SECONDS>` | Stop the recording after this many seconds; Ctrl-C stops it too. |

## chap-devops cleanup

Delete what deployments whose directory is gone left in docker.

```text
Usage: chap-devops cleanup [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `-n, --dry-run` | List what would be deleted and change nothing. |
| `--yes` | Delete without asking first. |

## chap-devops self

Update chap-devops itself, and report what this build is.

```text
Usage: chap-devops self [OPTIONS] <COMMAND>
```

Subcommands: [`chap-devops self update`](#chap-devops-self-update), [`chap-devops self version`](#chap-devops-self-version)

## chap-devops self update

Replace this binary with the newest release.

```text
Usage: chap-devops self update [OPTIONS]
```

| Argument | Description |
| --- | --- |
| `--check` | Report what an update would do and change nothing. |
| `--version <TAG>` | Install this release tag instead of the newest one. |
| `-y, --yes` | Do not ask before replacing the binary. |

Registry options: `--offline`, `--cache-dir <DIR>` (see [chap-devops](#chap-devops)).

## chap-devops self version

Show what this build is: version, revision, target and path.

```text
Usage: chap-devops self version [OPTIONS]
```

## chap-devops completions

Print a shell completion script for chap-devops.

```text
Usage: chap-devops completions [OPTIONS] <SHELL>
```

| Argument | Description |
| --- | --- |
| `<SHELL>` | Shell to generate for. Values: `bash`, `elvish`, `fish`, `powershell`, `zsh`. |
