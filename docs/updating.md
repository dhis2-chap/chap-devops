# Updating

This chapter is about updating a deployment: the model versions and the
chap-core release it runs. Updating the `chap-devops` binary itself is
`chap-devops self update`, which is in [Install](./install.md) and has nothing to do
with anything here.

## Three commands, one job each

| Command | What it does |
| --- | --- |
| `chap-devops up` | Starts what is on disk. Never changes a version. |
| `chap-devops update` | Fetches newer versions: moves the pins, pulls the images. Never touches a container. `--chap-tag` moves chap-core somewhere else entirely. |
| `chap-devops restart` | Applies what was fetched to the services that are running. |

**Without a deployment**, `chap-devops update` has no pin to move. It refreshes the
marketplace registry of this machine instead, as `chap-devops registry update` does.
`chap-devops self update` updates chap-devops itself, and
`chap-devops -C ~/.local/share/chap-devops/run/<group> update` updates a `chap-devops run` group. `--dry-run` reports
the registry that chap-devops has now, and fetches nothing, so it also works with
`--offline`. `--chap-tag`, `--pin-chap-core` and `--list-tags` need a
deployment, so they are refused there.

The first line says what changed against the copy that chap-devops had
before: the new and removed models, and the models whose entry changed, with
the stable version before and after. `-v` adds the age of that copy.

```text
$ chap-devops update
updated the marketplace registry: 7 models; changed: chapkit_simple_multistep_model 0.1.3 to 0.1.4
$ chap-devops update
the marketplace registry did not change: 7 models
$ chap-devops update --dry-run
the marketplace registry has 7 models (cache (0 seconds old)); --dry-run fetched nothing
```

An update that restarted things by itself would decide for you when your
deployment goes down, and an update that started a stopped deployment would be
a deployment nobody asked for. So `update` ends by telling you which of the
other two to run, and you run it when it suits you:

```sh
chap-devops update     # ... restart needed: chapkit-simple-multistep-model
chap-devops restart    # recreated chapkit-simple-multistep-model
```

## What moves a pin

Model pins move only with `chap-devops models enable ID` (with `--channel` or
`--version`), `chap-devops models add SOURCE` and `chap-devops update`. Nothing else, `up`,
`restart`, `expose` and `unexpose` included, changes the version a model runs.

| Command | Moves a pin? |
| --- | --- |
| `chap-devops up`, `chap-devops up --pull` | No. Starts what is on disk. |
| `chap-devops restart` | No. Recreates containers from the files as they are. |
| `chap-devops sync` | No. Renders the compose files from `.chap-devops/`. |
| `chap-devops docker pull` | No. Downloads the pinned images. |
| `chap-devops models expose`, `chap-devops models unexpose` | No. Only the `ports:` of one overlay. |
| `chap-devops models enable ID --channel C` | Yes, for that one model. |
| `chap-devops models enable ID --version X` | Yes, to exactly `X`. |
| `chap-devops models add SOURCE` | Yes, for the model it adds: to the newest published build of the branch, or to the tag or digest it was given. |
| `chap-devops update` | Yes: every channel-following model, every model added from a repository URL, and chap-core. |

## `chap-devops update`

```sh
chap-devops update [--dry-run] [--pin-chap-core] [--chap-tag TAG] [--list-tags] [--yes]
```

`chap-devops update` fetches the registry from the network: no cache, no fallback. It
fails under `--offline`, `--dry-run` included, because a plan made from a stale
catalogue is not a plan. It exits with status 2:

```text
error: `chap-devops update` refreshes the marketplace registry, which needs the network; drop --offline, or run `chap-devops update --list-tags` to see the chap-core tags offline
```

A run is four steps, printed in that order:

1. **The plan.** One row per enabled model, one for chap-core when the
   deployment has it, and one per optional component, saying what each is
   pinned to and what it would move to. The names are padded to one width, so that the versions line up. For
   every model that follows a channel the channel is re-resolved; a
   pin that moved is recorded in `.chap-devops/models.yaml` and its
   `# <ID>_IMAGE_TAG=` comment in `.env` is updated. Models enabled with
   `--version` are listed as pinned and skipped:

   ```text
     auto_arima_chapkit              v1.0.2 (sha-e9047d4)  unchanged
     chapkit_simple_multistep_model  v0.1.1 (sha-6c1d9ee) -> v0.1.2 (sha-10b2bc7)
     chap-core                       v2.4.0  unchanged, the newest release
   ```

   A model enabled with `--version 0.1.1` reads
   `v0.1.1 (sha-6c1d9ee)  pinned, skipped` instead.

   The marketplace can list a new image digest for the same version and tag.
   That is a move too, and the row shows the first 12 hex characters of each
   digest: `v1.0.4 (sha-49ee939@no digest) -> v1.0.4 (sha-49ee939@49f2b0ecce17)`.

   A model added with `chap-devops models add` is resolved against its own
   repository rather than the catalogue, so its row reads in image tags:

   ```text
     chapkit_dengue_model  sha-1eb8cf1 -> sha-b1d6c31
   ```

   One added from a repository URL follows that repository's default branch:
   the row moves when a newer commit has a published `sha-` build, and the new
   pin is written to `.chap-devops/models-manual.yaml` as well, so a later
   `models enable` brings back what is running now. One added from an image
   reference is `pinned, skipped`. A repository or registry that will not
   answer is a warning and a row reading `unchanged (could not check)`: the
   pin stays where it is, and the rest of the update goes ahead.
2. **The sync**, which renders the compose files from `.chap-devops/`.
3. **The pull**: `docker compose pull`. In a terminal, docker shows its own
   progress. Without a terminal (a pipe, a log file, CI), chap-devops tells docker
   to print no progress, unless you give `-vv`, so the run is silent until
   the pull ends. A pull of new chap-core images can take several minutes.
4. **The closing lines**, below.

## The closing lines

They answer the only question a finished update leaves: is there anything to
do now? There are five answers, and every run ends on exactly one of them.
With `-v`, the run also says which registry it used, as a hint. The warnings
of the run, such as a lookup that did not answer, come after these lines on
stderr.

Nothing moved, no new image arrived, and nothing running is out of date:

```text
already up to date
```

Running services are behind what the files now say, which is the case the whole
command is shaped around:

```text
updated 1 model pin; restart needed: chapkit-simple-multistep-model
run `chap-devops restart` to apply
```

Something moved and there is nothing running for it to be ahead of. With
`-v`, a hint says that `chap-devops up` starts the new versions:

```text
updated chap-core v2.4.0 -> latest
hint: Chap is not running; `chap-devops up` starts the new versions
```

Something moved, Chap is up, and none of it was affected. With `-v`, a hint
says `nothing running needs a restart`:

```text
updated 1 model pin
```

Something moved, and docker could not say what is running:

```text
updated 1 model pin
run `chap-devops restart` to apply it to what is running
```

The second answer can also start with another clause. Nothing was pinned to
move, but a moving tag brought a newer image - `ocs:main` after upstream has
published, say - and the running container is still on the old one:

```text
pulled a new image for ocs; restart needed: ocs
run `chap-devops restart` to apply
```

`chap-devops restart` then recreates that service alone, and a second `chap-devops update`
says `already up to date`.

If you run `chap-devops update` again before `chap-devops restart`, nothing moves, but the
services are still behind. The line then starts with `already up to date`:

```text
already up to date; restart needed: chap, worker
run `chap-devops restart` to apply
```

The first clause names what actually moved: the model pins, chap-core's tag,
and the services the pull brought a genuinely different image for. A moving
tag that pulled the same image this machine already had is not a change and is
not named, which is why a second `chap-devops update` on a `latest` deployment says
`already up to date` rather than claiming to have done something. A switch to
a tag with other images names both clauses:

```text
updated chap-core latest -> master and pulled new images for chap, worker; restart needed: chap, worker
run `chap-devops restart` to apply
```

If this machine already has the images at the new tag, the pull brings
nothing new, and the line has no `pulled` clause:

```text
updated chap-core latest -> master; restart needed: chap, worker
run `chap-devops restart` to apply
```

## What "needs a restart" means

For every running container, `update` asks docker two questions:

- Is the image it is running still the image its reference points at, now that
  the pull has finished? A moving tag such as OCS's `main` answers no as soon
  as upstream publishes.
- Is the configuration hash Compose stamped on it
  (`com.docker.compose.config-hash`) still the hash Compose computes for the
  files as they are now? A pin that moved, a port that changed, a secret that
  was rotated: all of them answer no.

Either answer being no means that container is running something this project
no longer describes, and it is named in the closing line. A question docker
would not answer is never counted as a difference, so an unreachable daemon
cannot turn into "restart everything".

## `chap-devops restart`

```sh
chap-devops restart [SERVICE..] [--all]
```

`chap-devops restart` is `docker compose up -d`, which recreates only the containers
that no longer match the files and leaves the rest running. It changes no file,
no pin and nothing in `.chap-devops/`, and it never syncs: it applies what is already
on disk.

Compose cannot see an edit to a mounted config file such as `dhis2/dhis.conf`
or `ocs/climate-service.yaml`. If such a file changed after its container was
made, `chap-devops restart` first recreates that service and says
`recreating <service> to apply its edited config file`. `--all` recreates
everything anyway, so it skips this step.

```sh
chap-devops restart                       # the whole deployment; compose decides
chap-devops restart chap worker           # only these, without their dependencies
chap-devops restart --all                 # recreate everything, changed or not
chap-devops restart --all my-model        # recreate this one, changed or not
```

Docker's own lines (`Container ... Running`, `Recreated`, `Healthy`) come
first. The command ends with what it did:

```text
recreated chap, worker
```

or, when it turned out there was nothing to apply, `nothing needed a restart:
every container matches its files`. With `-v`, the hints are there too: the
services it did not recreate, and the command to run next:

```text
$ chap-devops -v restart
...
recreated chapkit-simple-multistep-model
hint: unchanged: auto-arima-chapkit, chap, postgres, redis, worker
hint: run `chap-devops status` to check that everything answers
```

When nothing needed a restart, the hint is
``hint: `chap-devops restart --all` recreates every one anyway``.

A service name the project does not have is refused with the list of the names
it does have. A deployment with nothing running at all is not this command's
job, and it says which command it is, then exits non-zero:

```text
Chap is not running; start it with `chap-devops up`
```

`--all` is for the case where nothing has changed and a restart is still what
you want. A model whose container is up but which never registered with
chap-core is the example, and `chap-devops status` prints it as a hint:

```sh
chap-devops restart --all chapkit-rwanda-malaria-bym-model
```

## `--dry-run`

`chap-devops update --dry-run` prints the plan and stops: nothing is pulled and
nothing is written. It still needs the network, and it says what it would do:

```text
would update 1 model pin
```

With `-v`, a hint says ``hint: `chap-devops update` does it``. When nothing would
move, the line is `already up to date`. A dry run fetches no
`compose.ghcr.yml` either, so a warning about a missing one comes only from
the real run.

It says nothing about restarting. The images were not pulled and the files were
not written, so there is nothing yet for a running container to be out of step
with.

## chap-core's own pin

chap-core's pin moves in the same run.

`chap-devops init` resolves the default `--chap-tag latest` to the release it points
at today (`v2.4.0`, say) and writes that into `.env` and `.chap-devops/project.yaml`,
so a deployment is reproducible rather than following a tag that changes under
it.

`chap-devops update` then looks up the newest release, and when there is a newer one
it downloads the `compose.ghcr.yml` that release publishes, moves
`chap_image_tag`, and rewrites the single active `CHAP_IMAGE_TAG=` line in
`.env`. Only that line, and only when it still says what the project recorded.
A value you pinned yourself, or the commented placeholder, is left alone with a
warning saying what Chap will actually run.

## Moving tags

`--chap-tag latest|master|dev` keeps a moving tag instead. There is nothing to
move, so `update` only re-pulls it. Compose never re-pulls a tag it already
has, so the image is refreshed by `chap-devops docker pull`, `chap-devops update` or
`chap-devops up --pull`. The plan row says so:

```text
  chap-core  latest  moving tag, re-pulled; pin it with `chap-devops update --pin-chap-core`
```

```sh
chap-devops update --pin-chap-core
```

converts such a tag into the newest release pin, the way `init` does by
default. From `latest`, it pins the release that `latest` points at, so there
is no warning and no question. From `dev` or `master`, it is a move backwards
(see [Moving backwards](#moving-backwards)), so it shows the warning and asks.
In a script, give `--yes`: `chap-devops update --pin-chap-core --yes`.

If the newest release cannot be looked up, for example because the GitHub rate
limit is used up, the command pins nothing and fails:

```text
error: --pin-chap-core pinned nothing, because the newest chap-core release could not be looked up; try again later, or run `chap-devops update --chap-tag <TAG>` with a release from `chap-devops update --list-tags`
```

A tag that is neither a release nor a moving one, a `sha-` build for instance,
is never moved. When `--chap-tag` moves the pin to such a tag, a warning says
so:

```text
warning: sha-fa880a1 is neither a chap-core release nor a moving tag, so it is recorded as an exact pin and `chap-devops update` will never move it
```

A dry run says `so it would be recorded as an exact pin`. The warning comes
only when the pin moves. If ghcr refuses the tag, chap-devops prints only the ghcr
error.

## Switching chap-core's tag

`chap-devops update` moves a release pin forward and re-pulls a moving one. Going
somewhere else - onto `master`, back to a release, or straight to a release
that came out this morning - is `--chap-tag`:

```sh
chap-devops update --chap-tag master     # follow the master branch from now on
chap-devops update --chap-tag v2.4.0     # pin that release
chap-devops update --chap-tag latest     # follow whatever the newest release is
```

Before the pin moves, chap-devops asks ghcr whether it has both images, the
`chap-core` and the `chap-worker` image, at that tag. The dry run asks too. At
the time of writing, ghcr has a `chap-core:dev` image and no
`chap-worker:dev` image, so `--chap-tag dev` is refused:

```text
error: ghcr.io has no image chap-worker:dev, so the pull would fail; nothing was changed; pick another tag, for example a release from `chap-devops update --list-tags`
```

Use `chap-devops update --list-tags` to see the tags you can move to.

It is the same run as any other update: the tag is checked, the
`compose.ghcr.yml` that goes with it is fetched and cached under `.chap-devops/`,
`chap_image_tag` and `chap_compose_source` are recorded, the single active
`CHAP_IMAGE_TAG=` line in `.env` is rewritten, the compose files are
re-rendered, the images are pulled, and the closing lines say what moved and
what needs restarting:

```text
  auto_arima_chapkit              v1.0.2 (sha-e9047d4)  unchanged
  chapkit_simple_multistep_model  v0.1.2 (sha-10b2bc7)  unchanged
  chap-core                       latest -> master  (compose.ghcr.yml too)
updated chap-core latest -> master and pulled new images for chap, worker; restart needed: chap, worker
run `chap-devops restart` to apply
```

When the plan is printed, the new `compose.ghcr.yml` is not fetched yet. The
chap-core row ends in `(compose.ghcr.yml too)` when this run fetches
`compose.ghcr.yml` at a ref that the deployment does not have yet. A move from
`latest` to `master` shows it. A move from `v2.4.0` to `latest`, where `latest`
is `v2.4.0`, does not.

`--chap-tag` cannot be combined with `--pin-chap-core`: both decide where
chap-core's pin goes, and one command does not get to do it twice. clap
refuses the pair with
`error: the argument '--chap-tag <TAG>' cannot be used with '--pin-chap-core'`.

### The three moves

| Move | Command | What happens |
| --- | --- | --- |
| A release to the newest release | `chap-devops update` | The ordinary run. Nothing to name: `update` looks the newest release up itself. |
| A release to `master` and back | `chap-devops update --chap-tag master`, then `chap-devops update --chap-tag v2.4.0 --yes` | Onto `master` without a question; back to the release with the warning below, and an answer. |
| To a release the day it is out | `chap-devops update --chap-tag v2.4.1` | The same as waiting for `chap-devops update` to find it, except that it happens now. A release that does not exist is refused. |

A release that does not exist is refused before anything is written:

```text
error: chap-core has no release v9.9.9; run `chap-devops update --list-tags` to see the tags this deployment can move to
```

Asking for the tag that is already pinned changes nothing and says so:

```text
  ...
  chap-core                       master  already the pin, nothing to switch
already up to date
```

### Which compose file comes with which tag

`compose.ghcr.yml` is fetched at a ref of the repository, so a release tag and
a branch bring their own, and `latest` - which is an image tag rather than a
ref - brings the one the newest release publishes. A ref that has no
`compose.ghcr.yml` is a warning: the image pin moves and `compose.yml` keeps
the layout it has.

If the newest release cannot be looked up, `--chap-tag latest` still moves the
pin, because you named the tag. Only the compose file of the newest release is
missing:

```text
warning: could not resolve the newest chap-core release (...); the pin moves to `latest`, and compose.yml keeps the layout it has
```

A dry run says ``the pin would move to `latest` ``. Without `--chap-tag`, the
warning ends in ``the chap-core pin stays at `v2.4.0` `` and the pin does not
move.

### Moving backwards

Going back to an older chap-core means running an older schema against a
database a newer one has already migrated. `chap-devops` does not know chap-core's
migrations and will not pretend to, so it names the risk and asks:

```text
warning: moving chap-core from master to v2.4.0 can run an older schema against a database migrated by the newer one; run `chap-devops backup create` first

move chap-core from master to v2.4.0? [y/N]
```

An answer other than `y` stops the run with
`error: cancelled; nothing was fetched, written or pulled`.

Two moves count as backwards: a release to an older release, and a moving tag
to any release, since `dev` and `master` are ahead of every release and
`latest` is the newest of them. Going forward, and moving between moving tags,
needs no answer. `chap-devops update --pin-chap-core` from `latest` needs no answer
either: it pins the release that `latest` points at.

`--yes` is how a script says it meant it. A run with nothing to ask - no
terminal on stdin, or `--json` - is refused rather than assumed:

```text
error: moving chap-core backwards needs an answer and this is not a terminal; run `chap-devops update --chap-tag v2.3.1 --yes` to confirm it
```

With `--json`, the `error` field says `--json has nobody to ask`, and the
`hint` field holds the same `--yes` command.

The same refusal comes from `chap-devops update --pin-chap-core` from `dev` or
`master`; it ends with `` run `chap-devops update --pin-chap-core --yes` to confirm
it ``. A `--dry-run` prints the warning and stops there, because it writes
nothing to confirm.

### What a moving tag shows afterwards

`master` is the same name today and tomorrow, so the tag alone does not say
what is running. Both `chap-devops status` and `chap-devops doctor` say which build it is:

```text
chap-core   up   http://localhost:8700   2.5.0.dev0   master: running ad5aec7dc76f, revision a690e8804f25   auth: off
```

```text
ok    chap-core pin                                master (moving tag, running ad5aec7dc76f); `chap-devops update` re-pulls it, `chap-devops update --pin-chap-core` pins a release
```

The digest is the one the image was pulled at (`docker inspect`), shortened to
the twelve characters a person compares; the revision is what chap-core
reports for itself at `/system/info`, which a build without a `GIT_REVISION`
does not carry. Either is left off the line when it could not be had rather
than printed as a hole.

## `--list-tags`

```sh
chap-devops update --list-tags
```

lists where this deployment can move chap-core to, newest first: the three
moving tags, then the newest releases GitHub publishes.

That list, and the release lookup behind every `chap-devops update`, come from
GitHub's REST API, which allows 60 requests an hour to an address that sends
no credential; set `GITHUB_TOKEN` (or `GH_TOKEN`) and `chap-devops` sends it, for
5000 an hour instead. `chap-devops` never stores it and never prints it. See
[a used-up rate limit](./troubleshooting.md#githubs-rate-limit-is-used-up).

```text
TAG     KIND     PUBLISHED   NOTE
dev     moving   -           -
master  moving   2026-10-07  pinned (moving, running ad5aec7dc76f)
latest  moving   2026-10-05  -
v2.4.0  release  2026-10-05  newest
v2.3.1  release  2026-09-21  -
v2.3.0  release  2026-09-11  -
...

chap-core is pinned to master
```

With `-v`, a hint names `chap-devops update --chap-tag <TAG>`, which moves the pin.

`PUBLISHED` is the day the release was published, or the day the branch behind
a moving tag was last committed to; `latest` carries the date of the release it
publishes. A cell that could not be had cheaply is `-`. The `dev` row has `-`
because chap-core has no `dev` branch at the time of writing.

`NOTE` marks the tag this deployment records (`pinned`, with the running build
for a moving tag whose container is up) and the newest release (`newest`).

It writes nothing, pulls nothing, asks for no confirmation and ignores
`--dry-run`. Under `--offline` it prints the moving tags and this deployment's
own pin, with a warning that the releases could not be listed:

```text
warning: --offline: the chap-core releases were not listed, so this is the moving tags and this deployment's own pin
```

## Without the network

`chap-devops init --offline`, or an `init` whose fetch fails, keeps the tag exactly as
given and renders `compose.yml` from the copy of `compose.ghcr.yml` compiled
into the binary, warning on stderr both times. With `--chap-tag latest` that
means the deployment does follow the moving tag rather than a resolved release.

An already cached `.chap-devops/compose.chap-core.<tag>.yml` is reused rather than
re-downloaded, so a second `init --force` at the same tag works offline.

## Pulling without updating

```sh
chap-devops docker pull      # download the pinned images; no files change
chap-devops up --pull        # docker compose up --pull always
```

`chap-devops up --pull` is how a moving chap-core tag gets refreshed on the way up.
Neither of these asks the marketplace anything, so neither moves a pin.
