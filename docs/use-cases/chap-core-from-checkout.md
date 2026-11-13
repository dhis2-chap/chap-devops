# chap-core built from its checkout

For testing a chap-core change the way it will ship: chap-devops builds the chap-core
and worker images from your checkout and runs them with everything else it
normally runs (PostgreSQL, Valkey, the models, and OCS or DHIS2 if you add them
with `--with`).

Replace `~/dev/chap-core` with the path of your chap-core checkout:

```sh
chap-devops init mychap --source ~/dev/chap-core --models chapkit_simple_multistep_model
cd mychap
chap-devops up                     # builds both images from the checkout, then starts them
chap-devops status
```

The API image (`chap`) is built for the platform of this machine:
`linux/arm64` on an Apple Silicon Mac, `linux/amd64` on most servers. Its base
image has both, so the API runs without emulation, and its times are like
the times on a server. Use this when you time a change with
[`chap-devops bench`](../bench.md). The worker image stays `linux/amd64`,
because its base image (`chap-worker-with-inla`) has no other platform. On an
Apple Silicon Mac, the worker runs under emulation.

The build gives chap-core the commit of the checkout (`GIT_REVISION`), with
`-dirty` if a tracked file has changes. chap-core reports it as `revision`
on `/system/info`, and `chap-devops bench` shows it in its `measured on`
line.

The first `chap-devops up` pulls the base images and builds both images,
which took two to five minutes on an Apple Silicon Mac. It prints the full
docker build log, and ends with the services it started:

```text
started chap, chapkit-simple-multistep-model, postgres, redis, worker
```

```text
chap-core   up   http://localhost:8700   2.5.0.dev0   auth: off

MODEL                           STATE                       REACH          LAST PING
chapkit-simple-multistep-model  registered, not configured  via chap-core  1s ago

1 model registered
chapkit-simple-multistep-model: chap-core has no configured model for it, so nothing can run it; run `chap-devops models configs sync`
```

It worked when `chap-devops status` shows chap-core `up` and the model registered.
The version is the one that your checkout reports.

`registered, not configured` is the normal state after a plain `chap-devops up`.
chap-core makes no configured model when a model registers. A backtest and the
Modeling App need one. To make them, run `chap-devops models configs sync`. After that,
`chap-devops status` shows the model as `registered`. See
[Configured models](../models.md#configured-models).

- **`compose.yml` comes from the checkout's own `compose.ghcr.yml`**, re-read on
  every `chap-devops sync`, so the services match the code you are building.
- **Every `chap-devops up` builds the checkout as it is now.** `compose.chap-devops.yml`
  gives `chap` and `worker` a `build:` from the checkout and
  `pull_policy: build`. Docker's layer cache makes a build with nothing changed
  take some seconds, and `chap-devops up` then says `already running: ...`. After a
  change in the code, the build installs the dependencies again (about a minute
  on an Apple Silicon Mac), and `chap-devops up` recreates only what moved:
  `started chap, worker`.
- **The images are `<project>-chap:checkout` and `<project>-worker:checkout`**,
  named after the deployment's compose project (such as
  `mychap-77c0c2-chap:checkout`), so a build never overwrites a released
  `ghcr.io/dhis2-chap/...` tag on this machine, and two deployments built from
  two checkouts never overwrite each other's. The worker image is about 12 GB.
- **There is no pin.** `chap-devops update` still moves the models and components, and
  refuses `--chap-tag`: check out the version you want in the checkout instead.

The checkout has to have `Dockerfile`, `Dockerfile.worker` and
`compose.ghcr.yml`, which every chap-core clone does; anything else is refused
before a file is written (see
[`is not a chap-core checkout`](../troubleshooting.md#is-not-a-chap-core-checkout)).

## Go back to a released chap-core

`.env` and the data are kept. Do these steps in the deployment directory:

1. Stop the deployment:

   ```sh
   chap-devops down
   ```

2. Run `init` again without `--source`, with the same `--with` and `--models`
   values you used:

   ```sh
   chap-devops init . --force --models chapkit_simple_multistep_model
   ```

   `init` keeps `.env`, and moves its `CHAP_IMAGE_TAG` line from `checkout`
   to the released tag. It says so, and prints that tag:

   ```text
   created a deployment in /home/me/mychap
   moved CHAP_IMAGE_TAG in the kept .env from checkout to v2.4.0
   components: chap-core
   chap-core: v2.4.0, API on http://localhost:8700
   enabled chapkit_simple_multistep_model v0.1.2 at http://localhost:8700/v2/services/chapkit-simple-multistep-model/run/
   run `cd /home/me/mychap && chap-devops up` to start the deployment
   ```

3. Start the deployment:

   ```sh
   chap-devops up
   ```

It worked when `chap-devops status` shows the released version, such as `2.4.0`.
Some seconds after `chap-devops up`, `chap-devops status` can show the model as
`running, not registered` and exit with 1. If it does, run `chap-devops status` again
in a minute. If the checkout added a database migration, the released chap-core
may not start on that data.

The images built from the checkout stay on this machine. To get the disk space
back, remove them with `docker image rm <project>-chap:checkout
<project>-worker:checkout`. `<project>` is the `name:` line in
`compose.chap-devops.yml`, such as `mychap-77c0c2`.

More: [chap-core from its checkout, with the models](./chap-core-on-host.md)
runs chap-core as a process instead, without building images.

All shapes: [Use cases](../use-cases.md).
