# DHIS2

The `dhis2` component runs a DHIS2 instance and a PostgreSQL of its own beside
Chap, for a deployment that wants one.

```sh
chap-devops init mychap --with dhis2
chap-devops components enable dhis2      # or afterwards
chap-devops dhis2 connect                # once DHIS2 answers, so the Modeling App can
                                   # reach Chap
```

## What this component is for

A demo or a development DHIS2: something to log into while trying Chap out,
developing against it, or showing it. It is not a way to run DHIS2 in
production, and that is upstream's own judgement of the images rather than a
limitation of `chap-devops` - `docker/DOCKERHUB.md` in `dhis2/dhis2-core` says *"We
cannot recommend the images for use in production"*. A production DHIS2 is
deployed by other means, and a Chap deployment can be pointed at one without
this component: see [Connecting the Modeling App to Chap](#connecting-the-modeling-app-to-chap)
for what the connection is made of, all of which is requests to DHIS2 that you
can make against an instance `chap-devops` did not deploy.

Chap is often deployed **with** DHIS2 and not always. A deployment that already
has a DHIS2, or does not want one, leaves this component off and loses nothing:
chap-core, the models and OCS do not depend on it, and there is no `depends_on`
between DHIS2 and anything else here. This component is one way to get a DHIS2,
for the deployments that want one alongside.

## Four services and three volumes

`chap-devops sync` renders `compose.dhis2.yml`:

| Service | What it is |
| --- | --- |
| `dhis2` | DHIS2 itself, from `dhis2/core`, published on 8780 by default. Multi-arch since 2.39.1-rc, so nothing pins a platform for it. |
| `dhis2-db` | Its PostgreSQL, from `postgis/postgis:16-3.5`. The image is amd64-only, so the service pins `linux/amd64`. `expose`d on 5432 and never published. |
| `dhis2-dump` | A one-shot that prepares the [seed dump](#the-seed) and exits. Not rendered at all for a deployment that starts empty. |
| `dhis2-prep` | A one-shot that runs on every start, after the database is healthy and before DHIS2 boots. It uses the `dhis2-db` image, so it also pins `linux/amd64`. |

| Volume | What it holds |
| --- | --- |
| `dhis2_home` | `/opt/dhis2`: the installed apps and the file store, about 429 MB on a real instance. Without it both go with every recreate. |
| `dhis2_db` | The database's data directory. |
| `dhis2_dump` | The prepared dump - a **download cache**, not data. The one-shot fetches the dump again when the volume is empty. |

`dhis2-prep` does two things nothing else does, neither of them visible from
outside:

- `ANALYZE`. A freshly restored dump carries no planner statistics, and the
  analytics aggregations then get terrible query plans - measured here, a first
  analytics run of about 20 minutes instead of about 20 seconds. Autovacuum gets
  there eventually, but not before that first run.
- Reset any job a hard stop left flagged `RUNNING`. DHIS2 will not start a new
  analytics run while one is `RUNNING`, so a single stale row blocks analytics
  for good.

It runs under `ON_ERROR_STOP=0`, so a database DHIS2 has not migrated yet - one
with no `jobconfiguration` table - is a no-op rather than a failure.

## Its own database, and why it is not chap-core's

Three reasons, and the first is enough on its own.

1. **DHIS2 needs PostGIS.** Its own startup check throws without the `postgis`,
   `pg_trgm` and `btree_gin` extensions, so the image is a PostGIS one rather
   than plain PostgreSQL: `postgis/postgis:16-3.5`, from the PostGIS project.
2. **chap-core deploys `postgres:17`**, which has none of the three.
3. **A component could not reach it anyway.** Upstream's `compose.yml` puts the
   database on a second network, off the compose default one, which is exactly
   what keeps a model service away from it. A component's services join the
   default network like a model's do.

The two databases are therefore separate containers with separate volumes and
separate passwords, and `DHIS2_DB_PASSWORD` in `.env` has nothing to do with the
Chap one.

The health check on `dhis2-db` probes over TCP (`pg_isready -h 127.0.0.1`) on
purpose: while the entrypoint runs its init scripts the server listens on the
unix socket alone, so a socket probe would report a half-restored database as
ready and DHIS2 would start against half a database.

## `dhis2/dhis.conf`

DHIS2 loads this file at startup and **will not start without it**. There is no
environment-only mode: a missing file is a startup failure, not a fallback to
defaults. So `chap-devops` scaffolds it when the component is first enabled, mounts it
read-only at `/opt/dhis2/dhis.conf`, and never rewrites it - it is yours from
that moment on, exactly like
[`ocs/climate-service.yaml`](./components.md#ocsclimate-serviceyaml). `chap-devops
sync` re-creates it only if it has gone missing, which is worth doing precisely
because nothing starts without it.

What DHIS2 does with `${...}` in that file is substitute the environment
variable of that name, so the values live in the compose file and the file
itself carries placeholders. The `dhis2` service passes five:

| Variable | Default in the compose file |
| --- | --- |
| `DHIS2_DB_HOST` | `dhis2-db` |
| `DHIS2_DB_NAME` | `dhis` |
| `DHIS2_DB_USER` | `dhis` |
| `DHIS2_DB_PASSWORD` | `dhis`, overridden by the generated value in `.env` |
| `DHIS2_ENCRYPTION_PASSWORD` | empty, overridden by the generated value in `.env` |

The last is passed even when nothing sets it, so the placeholder in the file
becomes the empty string rather than standing there as a literal `${...}`.

### Applying an edit

```sh
chap-devops restart dhis2
```

DHIS2 reads this file once, at startup, and `chap-devops` mounts it as a **bind
mount**, which compose does not compare: `docker compose up -d` alone finds
nothing to do after an edit. `chap-devops restart` checks the file itself - when it
was written after the `dhis2` container was created, that container is
recreated, and the line says so:

```text
recreating dhis2 to apply its edited config file
```

If the file's modification time does not show the edit (a copy that kept an
older one), `chap-devops restart --all dhis2` recreates the container regardless.

DHIS2 migrates before it serves a request again, so it is not back the moment
compose returns; the next `chap-devops dhis2` command waits for it and says what it
found. This is the same trap, for the same reason, as
[`ocs/climate-service.yaml`](./components.md#read-only-instances).

### `encryption.password`

**24 characters or more.** DHIS2 encrypts stored credentials with it, and it
judges the length itself:

- shorter than 24, and it reports `ENCRYPTION_PASSWORD_TOO_SHORT` and stops;
- empty, and it reports `MISSING_ENCRYPTION_PASSWORD` and encrypts with a
  password compiled into DHIS2's own source - which every other DHIS2 in the
  world also has.

`chap-devops` generates 32 hex characters into `.env` as `DHIS2_ENCRYPTION_PASSWORD`,
comfortably past the floor. Like the database password it is written **once** and
never rewritten: the volume was created with it, and changing it later leaves the
credentials already in the database undecryptable.

### `route.remote_servers_allowed`

This is the one value `chap-devops` fills in rather than leaving as a placeholder:

```ini
route.remote_servers_allowed = http://*,https://*
```

DHIS2 version 42 and later default it to `https://*` and then **refuse** an
`http://` target. The target the Modeling App needs is usually exactly that: the
app does not reach chap-core directly, it reaches it through a DHIS2 Route, and
chap-core is `http://chap:8000` inside the deployment,
`http://host.docker.internal:8000` when it runs on this machine
(`--chap-core-url`), and plain http on most lab servers. So the default value
would refuse the one route this component exists to make possible.

The scaffold allows every http and https target, so any chap-core works without
touching the file. A DHIS2 chap-devops deploys is a development or evaluation
instance; DHIS2 logs a warning about the wildcard on every start, and on a DHIS2
that is not a test instance the line belongs narrowed to the origins in use. One
origin per entry, comma separated, and none of them may carry a path - DHIS2
throws on startup if one does.

### Behind a TLS-terminating proxy

Two lines are in the file, commented:

```ini
# server.https = on
# server.base.url = https://dhis2.example.org
```

The first marks the session cookie `Secure`, the second is the origin DHIS2
builds absolute links from. Uncomment **both** behind a proxy that terminates
TLS, and leave both commented while DHIS2 is reached over plain http - a `Secure`
cookie on an http origin makes the login loop.

Pair that with `--dhis2-port none` (or `chap-devops components enable dhis2 --port
none`), so the proxy is the only way in. `chap-devops` records no second copy of
`server.base.url`: that file is yours, so `chap-devops status` and `chap-devops components
list` say `internal` for such an instance rather than naming a proxy.

### Restoring an older dump

Two more commented lines cover the case where the seed dump comes from an older
DHIS2 than the image runs, which can stop on Flyway's migration order:

```ini
# flyway.repair_before_migration = on
# flyway.migrate_out_of_order = on
```

## The seed

`seed:` in `.chap-devops/components.yaml` decides what the database is restored from
the first time it is created. `--dhis2-seed` takes the same four answers at
`init`:

| Value | What happens |
| --- | --- |
| `default` | The demo dump published for the pinned minor line, if `chap-devops` knows of one. |
| `none` | Nothing. The database starts empty and DHIS2 migrates a new schema into it. |
| a URL | That dump, downloaded by the one-shot. An `http://` or `https://` prefix is the whole of the rule. |
| a path | That file, bind-mounted into the one-shot and copied rather than downloaded. |

```sh
chap-devops init mychap --with dhis2                              # the version-matched demo
chap-devops init mychap --with dhis2 --dhis2-seed none            # empty
chap-devops init mychap --with dhis2 --dhis2-seed dumps/laos.sql.gz
chap-devops init mychap --with dhis2 --dhis2-seed https://example.org/mine.sql.gz
```

After `init`, `chap-devops components enable dhis2 --seed SPEC` changes it. SPEC
takes the same values as `--dhis2-seed`. The seed applies only when `dhis2_db`
is created.

A path is relative to the deployment directory, or absolute. It becomes a bind
mount, and compose refuses to start a service whose bind source does not exist,
so `chap-devops sync` warns when the file is not there, before `chap-devops up` fails. A
relative path keeps the deployment directory self-contained: copy the dump into
it.

`chap-devops init` reads the version of the dump only from the deployment
directory, because that is the file the restore uses. If `init` makes the
directory, the file is not there yet, so `init` does not read its version and
warns:

```text
laos.sql.gz is not in /srv/laos, so chap-devops did not read its DHIS2 version, and DHIS2 runs 2.42
...
warning: the dhis2 seed names laos.sql.gz, which is not in /srv/laos; copy the dump there before `chap-devops up`, or run `chap-devops components enable dhis2 --seed none`
```

To let `init` read the version, make the directory and copy the dump into it
first:

```sh
mkdir laos
cp laos.sql.gz laos/
chap-devops init laos --only dhis2 --dhis2-seed laos.sql.gz
cd laos && chap-devops up
```

A URL seed under `--offline` is refused outright: the two ask for opposite
things.

### It applies once, to a database being created

The restore is the stock PostgreSQL entrypoint's own. `dhis2-dump` writes the
prepared dump into `dhis2_dump`, and `dhis2-db` mounts that volume at
`/docker-entrypoint-initdb.d/`, which the entrypoint reads **only** while it
initialises a data directory it has just created. So:

- the first `chap-devops up` restores the dump;
- every later `chap-devops up` does not, whatever `seed:` says;
- changing the seed on a deployment that has already started means removing
  `dhis2_db` first - `chap-devops components disable dhis2 --purge` is the honest way
  to ask for that, and it takes all three volumes.

Docker copies the image's own PostGIS init script into that volume as the
container is created, so the extensions are still set up, ahead of the dump.

### A restore that stops

If the restore stops with an error, the postgres entrypoint stops too. Docker
then starts `dhis2-db` again, and postgres sees a data directory, so it skips
the init. Without a guard, the result is an empty or incomplete database that
answers like a good one.

So the restore leaves a mark as its last init script, `zzz-chap-devops-seeded.sql`:
the comment `chap-devops: seed restored` on the database. On a seeded deployment, the
health check of `dhis2-db` asks for that mark. A restore that stopped has no
mark, so `dhis2-db` stays unhealthy, DHIS2 does not start, and `chap-devops up`
prints the lines of the log that say why. Here the dump had a statement that
PostgreSQL could not run:

```text
dependency failed to start: container mychap-1ab2c3-dhis2-db-1 is unhealthy

why dhis2-db is unhealthy:
  2026-10-08 04:44:18.888 UTC [71] ERROR:  function this_is_not_a_function() does not exist at character 8
  ERROR:  function this_is_not_a_function() does not exist
  the database has no mark `chap-devops: seed restored`, so its seed restore did not finish
  the restore of the DHIS2 seed stopped before the end, so `dhis2_db` is incomplete; fix the cause above, then run `chap-devops components disable dhis2 --purge`, `chap-devops components enable dhis2` and `chap-devops up`
error: docker compose exited with status 1
```

**This takes about 15 minutes.** The health check of `dhis2-db` has a start
period of 600 seconds and then 30 checks 10 seconds apart, and `chap-devops up`
prints nothing new until compose gives up. While it waits, `chap-devops status`
says `dhis2 starting`, because `dhis2-db` runs and DHIS2 waits for it. To see the error sooner, run `chap-devops logs dhis2-db`
in a second shell. The error is there seconds after the restore stops.

`chap-devops backup restore` sets the mark again after it loads the `pg_dump` of the
database, so a database that it puts back is healthy too. See
[Backing it up](#backing-it-up).

### The version of the dump

DHIS2 runs a database of its own minor version or older, and chap-devops supports
DHIS2 2.41 and newer, because the Modeling App needs it. The version of a dump
is in its Flyway table: each migration that DHIS2 ran is a row of
`flyway_schema_history`, and the newest one says the version. `2.42.54` is
migration 54 of DHIS2 2.42. The last number is not the patch release.

- For a dump on disk, `chap-devops init` reads the version. It looks for the path
  only in the deployment directory. If the dump is not there, `init` says
  `<path> is not in <dir>, so chap-devops did not read its DHIS2 version, and DHIS2
  runs <tag>`. A dump of 3.9 GB takes about 20 seconds, because the table
  comes after `datavalue`.
  `chap-devops components enable dhis2 --seed PATH` does not read it.
  - Without `--dhis2-tag`, DHIS2 runs the version of the dump.
  - A `--dhis2-tag` older than the dump is refused.
  - A dump older than 2.41 is refused.
- A `--dhis2-tag` or `--tag` older than 2.41 is refused, also without a dump:
  `` --tag 2.40 is older than DHIS2 2.41, the oldest version chap-devops supports;
  use `--tag 2.41` or newer ``.
- For a URL, the dump step reads the version after the download, before the
  rewrite, and stops with the same messages. `chap-devops up` then prints the error
  line of the dump step under `why dhis2-dump failed:`, with the command that
  fixes it.

```text
$ chap-devops init laos --only dhis2 --dhis2-seed laos.sql.gz
reading the DHIS2 version of laos.sql.gz from its Flyway table; a large dump takes about a minute
laos.sql.gz is DHIS2 2.42 (Flyway migration 2.42.54), so DHIS2 runs 2.42
```

A URL dump that is newer than the tag stops in the dump step, and `chap-devops up`
says why:

```text
service "dhis2-dump" didn't complete successfully: exit 1

why dhis2-dump failed:
  error: DHIS2 2.41 is older than the dump (DHIS2 2.42), and DHIS2 does not run a newer database; run chap-devops components enable dhis2 --tag 2.42, then chap-devops up
  the dump is DHIS2 2.42, and DHIS2 does not run a newer database; run `chap-devops components enable dhis2 --tag 2.42`, then `chap-devops up`
error: docker compose exited with status 1
```

`chap-devops logs dhis2-dump` shows the whole log of the step. To set the tag, run
`chap-devops components enable dhis2 --tag 2.42`, then `chap-devops up`.

### Values that analytics cannot read

A real DHIS2 database has data values that look like numbers but do not fit in
the number type of analytics (`double precision`, at most about 1.8 × 10^308).
An example is a value of 2,081 digits. DHIS2 accepted it when someone typed it
in or imported it. Analytics casts every value of a numeric data element, so
one such value stops every analytics run with `out of range for type double
precision`.

So every restore of a seed runs `zz-chap-devops-clean.sql` after the dump. The
script does two things:

1. It lists each such value in the log of `dhis2-db`: the data element, the
   period, the organisation unit, the first digits and the length.
2. It empties the value (`value = NULL`). It does this for deleted values
   too, because the outlier query of analytics reads them as well.

```text
$ chap-devops logs dhis2-db | grep -A4 "cannot read as a number"
chap-devops: data values that analytics cannot read as a number, emptied:
 dataelement |    name     |   period   |   orgunit   | value_starts | length
-------------+-------------+------------+-------------+--------------+--------
 BW7mJYKnkks | SBCC - VF1  | 2023-05-01 | Kh35DvuCtnl | 999999999999 |   2081
```

This changes the data of the restored copy, not the dump. Only values that
analytics cannot read change. Correct the values at their source too.

### One password for every user

A dump of a real DHIS2 has the passwords of its real users. To log in as any
of them, give every user one password at the restore:

```sh
chap-devops init laos --only dhis2 --dhis2-tag 2.42 --dhis2-seed laos.sql.gz --dhis2-seed-password
cp laos.sql.gz laos/
```

Without a value, the password is `district`. The option writes
`seed_password:` in `.chap-devops/components.yaml`:

```yaml
dhis2:
  seed: laos.sql.gz
  seed_password: district
```

The restore then does four things to every row of `userinfo`:

- It sets the password. PostgreSQL makes the bcrypt hash with `pgcrypto`, one
  hash for all the users. A hash for each user takes minutes on a large dump.
- It turns on the account (`disabled = false`).
- It turns off two-factor login (`twofactortype`, `secret`). A password alone
  does not log in a user who has it.
- It removes the expiry date of the account (`accountexpiry`).

The last two run only when the column is in the dump, because the columns
change between DHIS2 versions. Use this for a test or a development copy of a
database, not for a server that real users log in to.

The `dhis2-dump` one-shot writes the SQL to `zz-chap-devops-passwords.sql` in the
`dhis2_dump` volume. The postgres entrypoint runs the files of that volume in
name order, so the file runs after `dump.sh`, which restores the dump. When the dump did not have
the `pgcrypto` extension, the file removes it again.

The rules of the seed apply:

- It applies once, when `dhis2_db` is created. To apply it to a database that
  exists, run `chap-devops components disable dhis2 --purge` first.
- It needs a seed. `--dhis2-seed none` has no users, so the two options
  together are refused.
- To change or remove it later, edit `seed_password:` and run `chap-devops sync`.
  `DHIS2_SEED_PASSWORD` in `.env` also sets it.

The password is plain text in `.chap-devops/components.yaml` and in
`compose.dhis2.yml`. Use it for a known password such as `district`, not for a
secret. `chap-devops dhis2` uses it for every user, so `chap-devops dhis2 analytics --user
em` logs in as `em` with no other setting. A password in `.env` or in the
environment still comes first.

### The default dump comes from a table, not from the tag

The published filename does not follow from the version:

| Minor line | Dump |
| --- | --- |
| `2.42` | `https://databases.dhis2.org/climate/laos/2.42/laos.sql.gz` |

The 2.41 line used to have a dump as well; it is no longer published, so a 2.41
deployment starts empty unless `seed:` names one. DHIS2's Sierra Leone demo is
published for 2.41, 2.42 and 2.43
(`https://databases.dhis2.org/sierra-leone/<version>/dhis2-db-sierra-leone.sql.gz`),
without the climate data the Modeling App uses.

Published paths have differed in patch version and basename from one line to
the next, so `chap-devops` keeps a table keyed by the minor line (`2.42`, `2.42.1` and `2.42.1.1`
are all `2.42`) rather than building a URL from the tag, because a constructed
one would download a 404 on the first start and leave an empty DHIS2 with no
explanation.

A minor line the table does not list starts empty and says so, on every sync:

```text
warning: chap-devops knows no DHIS2 demo dump for 2.41, so a new `dhis2_db` starts empty; name one with `chap-devops components enable dhis2 --seed URL` (a URL or a path)
```

### What the one-shot does to the dump

The one-shot `dhis2-dump` downloads or copies the dump and keeps it as
`seed.dump.gz` in the `dhis2_dump` volume. The download goes to `raw.part`
first, and one `mv` makes it the seed, so a download that stopped is never
kept. The name does not end in `.sql.gz`, so the PostgreSQL entrypoint does not
load it by itself.

The restore is one pass. The one-shot writes `dump.sh`, and the entrypoint runs
it in name order, before the `zz-` scripts:

```text
gunzip -c seed.dump.gz | sed -E -f chap-devops-rewrite.sed | psql -v ON_ERROR_STOP=1
```

The dump is read once, with the `gzip` and `sed` of the PostgreSQL image. On a
dump of 3.9 GB, the old way took 24 to 40 minutes: three reads with the busybox
tools of the one-shot, then a fourth read for the restore. The entrypoint runs
`dump.sh` with `pipefail`, so a truncated dump stops the restore, and the
[mark](#a-restore-that-stops) reports it.

`chap-devops-rewrite.sed` changes the dump so that it loads into a new database:

- **It retrofits `--if-exists`.** Published dumps are `pg_dump --clean`
  *without* `--if-exists`, so they open on `DROP` and `ALTER` statements that
  assume the schema is already there, and the restore runs under
  `ON_ERROR_STOP=1`, where the first missing object stops it. The rewrite is
  idempotent and anchored to the start of each line, so `COPY` data rows stay
  as they are. `DROP EXTENSION` and `DROP SCHEMA` go entirely: the PostGIS
  image owns those objects.
- **It removes what this PostgreSQL does not know.** A dump of `pg_dump` 17.6
  or 16.10 and later opens with `\restrict` and closes with `\unrestrict`, and
  a dump of PostgreSQL 17 sets `transaction_timeout`.
- **It removes the owners and the grants**, as `pg_dump --no-owner
  --no-privileges` does. They name the roles of the source server, which do not
  exist here, so every object belongs to the database user.

An operator's own dump goes through the same steps: the mounted file is copied
to the place where a download goes, and everything after that is the same.

## The first start

**Minutes, not seconds.** DHIS2 migrates its whole schema before it serves a
request, and a seeded deployment restores the dump before that. Every other
service in a Chap deployment answers in seconds, so an operator who does not know
this reads the first `chap-devops status` as a broken deployment. With `-v`, both
`chap-devops init --with dhis2` and `chap-devops components enable dhis2` say so, and

```sh
chap-devops logs dhis2
```

is where the migration shows.

The honest budget, measured:

| Shape | Roughly |
| --- | --- |
| Native arm64, empty database | 40 seconds |
| Under emulation (an amd64 image on arm64, or the other way round) | 8 to 15 minutes |
| Seeded | the above, plus the restore: about 10 seconds for the climate demo (7 MB), much longer for a national dump |

The health check allows for that: `/api/ping` - the one route that answers
unauthenticated - with a `start_period` of 240 s and 60 retries, and 600 s on
`dhis2-db`, because on a seeded deployment the database's first start *is* the
restore.

## Memory

DHIS2 wants roughly **4 to 5 GB** for the analytics populate phase. Below that
the JVM is `SIGKILL`ed part-way through the populate, and the failure looks like
anything except memory: a container that exits with no Java exception in the log,
an analytics run that never finishes, a DHIS2 that was healthy a minute ago and
is now gone. On Docker Desktop the limit to raise is the VM's, not the container's.

The heap is passed as `JAVA_TOOL_OPTIONS`, not `JAVA_OPTS`: it is the JVM's own
variable and works for both image variants, while the distroless one ignores
`JAVA_OPTS` and then sizes the heap at about a quarter of the machine's memory.
The default is in the compose file and commented into `.env`:

```ini
# DHIS2_JAVA_TOOL_OPTIONS=-Xms2g -Xmx4g -XX:+UseG1GC
```

Uncomment and edit that line to change it; the variable replaces the options
rather than adding to them, so write the whole set.

## Changing the DHIS2 version

DHIS2 migrates its schema **forward only**, and that makes the image tag the one
setting here worth being careful with:

- a **newer** image migrates the database on the way up, irreversibly;
- an **older** image against a migrated database gives an instance that answers
  its own health check while every API request 404s. Up, and wrong - the worst
  shape a deployment can be in.

So: run `chap-devops backup create` first. Then move the tag deliberately, either in
`.chap-devops/components.yaml` followed by `chap-devops sync`:

```yaml
dhis2:
  enabled: true
  port: 8780
  image_tag: '2.41'
  image: dhis2/core
  seed: default
```

at creation with `chap-devops init --with dhis2 --dhis2-tag 2.41`, later with
`chap-devops components enable dhis2 --tag 2.41` (and `--image` for another
repository), or in the browser:
`chap-devops ui`, `Tab` to the components page, `v` on the `dhis2`
row. It offers the minor lines chap-devops has a demo database for (and the version in
force, if that is another), says the forward-only rule as soon as you pick one
that differs, and warns again on save when `dhis2_db` is already there. Or with
an active `DHIS2_IMAGE_TAG` line in `.env`, which Compose reads last and
which therefore wins without a sync. The rendered service is
`dhis2/core:${DHIS2_IMAGE_TAG:-2.42}`, so the recorded tag is only the default.

An unreleased DHIS2 comes from another image repository: DHIS2 publishes its
development builds as `dhis2/core-dev`, with the next version under `master`.
`chap-devops init --with dhis2 --dhis2-image dhis2/core-dev --dhis2-tag master` runs
it, and `image:` in the `dhis2` block of `.chap-devops/components.yaml` is the setting
afterwards. There is no demo database for a development build, so it starts
empty.

The tag is two parts rather than a full version on purpose, so a patch release
arrives with a `docker pull` rather than an edit.

`chap-devops` warns when a run moves the tag while `dhis2_db` is already there. It
compares the tag in the **rendered** `compose.dhis2.yml` - what is deployed -
against the one being asked for, not two records against each other:

```text
warning: the DHIS2 image moves from 2.42 to 2.41 and `dhis2_db` is already there: DHIS2 migrates a schema forward only, so run `chap-devops backup create` first - an older image on a migrated database answers healthy while every API request 404s
```

A move to a newer version gets its own warning, because the migration cannot
be undone:

```text
warning: the DHIS2 image moves from 2.41 to 2.42 and `dhis2_db` is already there: the next `chap-devops up` migrates the database to 2.42, and DHIS2 migrates a schema forward only, so run `chap-devops backup create` first - the migration cannot be undone
```

Moving the seed does not need this care and moving the tag does. So the browser
edits the tag only through the `v` prompt, which states the forward-only rule.
It does not edit the seed: its `i` overlay sends you to `chap-devops components
enable dhis2 --seed SPEC`.

## Connecting the Modeling App to Chap

A DHIS2 and a Chap started side by side cannot talk, and the reason is worth
understanding before the commands make sense: **chap-core never calls DHIS2 and
DHIS2 never calls chap-core.** The only thing that talks to both is the Modeling
App running in a browser, and it reaches chap-core through **DHIS2's Route API** -
a reverse proxy configured as a `Route` row inside DHIS2, with `code: "chap"`,
that DHIS2 then serves under `/api/routes/chap/run/`.

Three things follow from that, and they are the three the commands do:

| Without it | What the app does |
| --- | --- |
| the `chap` route | redirects to `/get-started`; it cannot see Chap at all |
| the `analytics_*` tables | has no data to send, and nothing on screen says why |
| the apps themselves | there is no Chap user interface in DHIS2 |

```sh
chap-devops dhis2 show        # what this instance has, changing nothing
chap-devops dhis2 connect     # the route, the apps, then analytics
```

| Command | What it does |
| --- | --- |
| `chap-devops dhis2 show` | Asks and reports: where the `chap` route points and whether chap-core answers through it, what DHIS2's analytics timestamp is worth, which of the two apps are installed, and each piece that is missing. A route that is right in every field but that nothing answers through counts as missing. Writes nothing. |
| `chap-devops dhis2 route` | Creates the `chap` route, or **repoints** one that is there, then proxies a request through it to prove the whole path. |
| `chap-devops dhis2 analytics` | Generates the analytics tables and waits for them; `--no-wait` starts the run and leaves it going. |
| `chap-devops dhis2 apps` | Installs the Modeling App and the Climate App from the App Hub, at the newest version this DHIS2 can run. |
| `chap-devops dhis2 connect` | All three: the route, then the apps, then analytics. It also creates the configured models in chap-core, which the Modeling App lists. |
| `chap-devops dhis2 use` | Points all of the above at a DHIS2 that runs elsewhere. See [A DHIS2 that runs elsewhere](#a-dhis2-that-runs-elsewhere). |

Every one of them is idempotent and meant to be re-run: a second `chap-devops dhis2
connect` repoints nothing, reinstalls nothing and says so. It runs analytics
again, which takes about 20 seconds on the climate demo. A step that found
nothing to do still reports it.

### `chap-devops up` does none of this, on purpose

`chap-devops up` is a thin wrapper around `docker compose up`, and three things make
this the wrong work to hang off it:

- it needs **DHIS2 credentials**, which are not chap-devops's to invent;
- it reaches the **network** - the App Hub, twice - and `chap-devops up` never does;
- DHIS2's API is **not ready when `up` returns**. Tomcat serves pages while every
  `/api/*` request 404s, which is the shape `chap-devops status`'s `dhis2` row already
  knows about, and analytics then takes tens of seconds on demo data and much
  longer on real data. An `up` that waited for that would be an `up` that takes
  an hour.

So `chap-devops components enable dhis2` prints the line that names the command
instead, and `chap-devops init --with dhis2` prints it with `-v`:

```text
the Modeling App reaches chap-core through a DHIS2 route, and this deployment has none yet; once DHIS2 answers, `chap-devops dhis2 connect` adds it, generates analytics and installs the apps
```

### `up` and `status` keep asking until a connect is recorded

That note is printed at the one moment DHIS2 does not exist yet. What follows it
is `chap-devops up`, then several minutes of restoring a dump and migrating a schema,
and by the time anything can be connected the note is far up the scrollback. A
deployment can therefore sit for good with every row `up`, `chap-devops doctor`
reporting no problem, and the Modeling App unable to reach Chap at all.

So the two commands that report on a running deployment say it again, and go on
saying it until a connect has been recorded. `chap-devops up` closes with it, under
the line it always ends on:

```text
already running: chap, chapkit-simple-multistep-model, dhis2, dhis2-db, postgres, redis, worker
chap-devops has not connected this DHIS2 to Chap; run `chap-devops dhis2 connect` once DHIS2 answers
```

and `chap-devops status` puts it under its verdict, with the model hints:

```text
chap-core   up   http://localhost:8700   2.4.0   auth: off
dhis2       up   http://localhost:8780

MODEL                           STATE                       REACH          LAST PING
chapkit-simple-multistep-model  registered, not configured  via chap-core  10s ago

1 model registered
chapkit-simple-multistep-model: chap-core has no configured model for it, so nothing can run it; run `chap-devops models configs sync`
chap-devops has not connected this DHIS2 to Chap; run `chap-devops dhis2 connect`
```

Both lines come off `.chap-devops/components.yaml` and nothing else. Neither asks
DHIS2 anything, which is why each says what **chap-devops** has recorded rather than
what DHIS2 is - and naming the command is safe either way, because every `chap-devops
dhis2` verb is idempotent and a second run changes nothing.

`chap-devops status` says it only while the `dhis2` row reads `up`, since a DHIS2 that
is not answering cannot be connected to anything and the advice could not be
taken. `chap-devops up` has asked nothing at all and DHIS2 is minutes from its first
request, so it says *once DHIS2 answers*. Neither says it on a deployment
[without chap-core](./components.md), where `chap-devops dhis2 connect` refuses: the
route would point at a service that is not there. There, `chap-devops dhis2 show`
says `target  none (this deployment has no chap-core)` and names `chap-devops dhis2
apps` and `chap-devops dhis2 analytics` in place of `connect`.

### `connected_at`, and what it is not

The line stops once `chap-devops dhis2 connect` has got through, and that is recorded
in the `dhis2` block of `.chap-devops/components.yaml`:

```yaml
dhis2:
  enabled: true
  port: 8780
  image_tag: '2.42'
  image: dhis2/core
  seed: default
  connected_at: 2026-09-27T12:09:53Z
```

**It is a note that the command ran, and it is not evidence.** Nothing decides
anything by it except whether those two lines are printed. `chap-devops doctor`
also shows the time, as ``last `chap-devops dhis2 connect`: TIME``. The route can be
deleted, repointed at another server or disabled in DHIS2's own Route
administration a minute later, and this timestamp will not move, because nothing
reads DHIS2 to check it.

**[`chap-devops dhis2 show`](#connecting-the-modeling-app-to-chap) is the command that
asks DHIS2.** It reads the route out of the instance, proxies a request through
it to prove chap-core answers, lists which of the two apps are installed and
says what the analytics timestamp is worth - and it writes nothing, so it is
safe to run on anything. If the question is *can the Modeling App reach Chap*,
that is the command; `connected_at` cannot answer it and does not try.

`connect` says as much where it writes the record:

```text
hint: recorded the connect in `.chap-devops/components.yaml`, so `chap-devops up` and `chap-devops status` stop asking for it
hint: the record says that this ran, not that the route is still right; `chap-devops dhis2 show` asks DHIS2
```

#### What sets it

A run of `chap-devops dhis2 connect` that ended with a **verified route and both apps
in place**. On an external DHIS2, where `connect` sets only the route, a
verified route is enough: the apps are the step of its admin, and `chap-devops dhis2
show` names them. Two of the three steps, and deliberately so: the hint exists because
a DHIS2 beside a Chap cannot be used at all without them - the app redirects to
`/get-started` with no route, and there is no Chap user interface in DHIS2 with
no apps. Analytics is not part of it. The Modeling App reaches chap-core and
works without the `analytics_*` tables; empty tables are a deployment with no
data rather than one that cannot talk to itself, `chap-devops dhis2 analytics
--no-wait` is a supported shape in which no run has finished, and `chap-devops dhis2
show` reports the analytics question separately and with the evidence it is
worth.

The route is judged by the **proxied request**, not by the row: `connect` asks
chap-core for its health through `/api/routes/chap/run/`, the way the app does.

When chap-core has an API token (see [Authentication](./auth.md)), the route
carries it: `connect` writes it into the route's `auth` as an `Authorization:
Bearer` header, which DHIS2 adds to every request it proxies and never lists
back. `/health` needs no token, so with one on the check also asks for
`/v2/services`, which does; a route that reaches `/health` and gets a 401 past
it is rewritten, and one DHIS2 lists with no header auth at all is rewritten
before anything is asked. The Modeling App itself never needs the token.

The single-step verbs record nothing. `chap-devops dhis2 route` alone leaves a
deployment with a route and no apps, which is still one nobody can use Chap
from.

`chap-devops dhis2 connect --offline` is the third answer, and it is neither of the
other two. It skips the app install and says so, so the run has looked at
nothing that could tell it whether the apps are in DHIS2 - it **judges nothing,
and leaves the record exactly as it found it**, in either direction. A
deployment nothing has connected still records no connect. A deployment that
was connected keeps the timestamp it has: the apps are still installed, this
run simply did not ask, and a flag that turned off an unrelated step is not
evidence that anything broke. Clearing on it would make `chap-devops up` say chap-devops
has not connected a DHIS2 that chap-devops connected.

#### What clears it

| When | What happens |
| --- | --- |
| `chap-devops components disable dhis2`, with or without `--purge` | Forgotten with the component. With `-v`, the disable says so. The record is about a DHIS2 instance this deployment no longer has. |
| `chap-devops down --volumes`, when `dhis2_db` was actually removed | Forgotten with the database, and the line says why. |
| A `chap-devops dhis2 connect` that found something wrong | Cleared. With `-v`, the report says ``cleared the earlier `chap-devops dhis2 connect` ``. A route nothing answered through clears it. An app that is missing or failed clears it only for the `dhis2` component, not for an external DHIS2. The hint comes back, which is the answer that errs the safe way. |
| A `chap-devops dhis2 connect` that could not look | Nothing. `--offline` skips the apps, so the run has nothing to say about them and the timestamp stays where it was. |

The `chap-devops down --volumes` row is the one worth understanding. `dhis2_db` going
means the next `chap-devops up` restores [the seed dump](#the-seed) into a database
being created **and that dump ships a `chap` route of its own, pointed at an
external server**. A record that survived would suppress the one line asking the
operator to repoint it, on a deployment whose Modeling App is quietly talking to
somebody else's Chap. Disabling the component is the same hazard by another
route, which is why it clears the record whether or not `--purge` was given: a
`dhis2_db` kept on disk may be re-created from the dump later anyway.

```text
hint: the record of `chap-devops dhis2 connect` is forgotten with the component; a DHIS2 enabled here again is asked to connect afresh
```

#### What does not clear it

Nothing that happens inside DHIS2. Deleting the route, repointing it, disabling
it, dropping the `F_CHAP_MODELING_APP` authority, uninstalling either app, or
restoring a different database into the same volume by hand all leave the
timestamp exactly where it is, and the two hint lines stay quiet. So does a
restored [backup](./backup.md), which brings `.chap-devops/` and the volumes back
together and is therefore consistent, but says nothing about what has happened
to that DHIS2 since.

None of that is a gap to be closed by reading more state: a record can only ever
say what happened at one moment, and the live answer needs a request. `chap-devops
dhis2 show` is that request.

### The credentials, and where they come from

`chap-devops` holds no DHIS2 credentials, and every request above needs one: a
username and password (HTTP Basic), or a DHIS2 **personal access token**. The
rule is that **a password belongs to the user it was set with**, so the places
chap-devops reads are pairs, and the first that has a value wins:

| Order | Source | Notes |
| --- | --- | --- |
| 1 | `DHIS2_API_TOKEN` in `.env` | A personal access token. Preferred to the password beside it: it can be scoped and revoked. |
| 2 | `DHIS2_ADMIN_PASSWORD` in `.env`, for `DHIS2_ADMIN_USERNAME` | The deployment's own answer, and where its other secrets already live. The user is `admin` when the file names none. |
| 3 | `CHAP_DEVOPS_DHIS2_TOKEN` in the environment | A token kept off disk: `export` it for one shell. |
| 4 | `CHAP_DEVOPS_DHIS2_PASSWORD` in the environment, for `CHAP_DEVOPS_DHIS2_USERNAME` | A password kept off disk. Without `CHAP_DEVOPS_DHIS2_USERNAME` it is the password of the user `.env` names. |
| 5 | `seed_password:` in `.chap-devops/components.yaml` | For any user, on a DHIS2 chap-devops restored from a seed with that option. The restore gave every user that password. |
| 6 | `admin` / `district` | The DHIS2 default, and only on a DHIS2 chap-devops deployed. |

The default is not a guess. A seeded demo dump ships that user, and a
Flyway-bootstrapped empty database gets it from `DefaultAdminUserPopulator`, which
has both words compiled in. It is never sent to an
[external DHIS2](#a-dhis2-that-runs-elsewhere): chap-devops did not create that
instance, so `district` is nobody's password there, and trying it would only be a
failed login in its audit log. An external DHIS2 with none of the four set is an
error before any request is made. So is a `.env` that names a user and sets no
password for them: the default belongs to `admin`, not to whoever the file names.

`.env` carries the variable names as commented placeholders. On a DHIS2 chap-devops
deployed they hold exactly the values already used, so uncommenting one changes
nothing until it is edited:

```ini
# DHIS2 login `chap-devops dhis2` uses. Only chap-devops reads these - no container is given
# them - and commented out means the DHIS2 default below. A personal access token,
# when set, is used instead of the username and password.
# DHIS2_API_TOKEN=
# DHIS2_ADMIN_USERNAME=admin
# DHIS2_ADMIN_PASSWORD=district
```

For an external DHIS2 the password line is empty, since there is no default to
write down. `chap-devops sync` adds the section when a DHIS2 is first used.
Only `chap-devops` reads these variables. None of them reaches a container, so none
appears in a generated compose file.

**`--user NAME`** asks for one user by name for one run. That always means a
password, even when a token is on offer, and only a password that belongs to
`NAME` is sent:

- `DHIS2_ADMIN_PASSWORD`, when `.env` names `NAME`;
- `CHAP_DEVOPS_DHIS2_PASSWORD`, unless `CHAP_DEVOPS_DHIS2_USERNAME` gives it to someone else;
- `seed_password:` in `.chap-devops/components.yaml`, for any user, on a DHIS2 chap-devops
  restored from a seed with that option;
- the default, for `admin` on a DHIS2 chap-devops deployed.

Anything else is refused before a request is made, rather than sending one user's
password in another's name. So a one-off run as someone else is:

```sh
CHAP_DEVOPS_DHIS2_PASSWORD=theirs chap-devops dhis2 show --user alice
```

There is deliberately **no `--password` or `--token` flag**, because a secret on
a command line ends up in the shell history and in `ps`. Nothing ever prints the
secret. A report names the user and says what the credential is and where it was
found (``password from `.env` ``, `API token from CHAP_DEVOPS_DHIS2_TOKEN`, `the DHIS2
default password`). For a token, the user is whoever DHIS2 says owns it
(`GET /api/me`). `-vv` traces the header as `Authorization: Basic <admin and its
password>` or `Authorization: ApiToken <the token>`.

A token is created in DHIS2 under **Profile > Personal access tokens**. Leave its
allowed HTTP methods unrestricted, or allow at least `GET`, `POST` and `PUT`:
`route` writes with `POST` and `PUT`, and `analytics` and `apps` start work with
`POST`. The token's user needs the same authority a password's would, and a
superuser has it.

A credential DHIS2 does not accept gets an error that says so, rather than
"request failed":

```text
error: DHIS2 at http://localhost:8780 did not accept the password for `admin` (the DHIS2 default password); set `DHIS2_ADMIN_USERNAME` and `DHIS2_ADMIN_PASSWORD` in `.env`, or export `CHAP_DEVOPS_DHIS2_PASSWORD`
error: DHIS2 at https://dhis2.example.org did not accept the API token (API token from `.env`): it is expired, revoked, or not allowed from this address; set a current one as `DHIS2_API_TOKEN` in `.env`, or export `CHAP_DEVOPS_DHIS2_TOKEN`
```

### A DHIS2 that runs elsewhere

Most real deployments put Chap beside a DHIS2 that already runs on a server of
its own. `chap-devops dhis2 use` records one, and from then on every `chap-devops dhis2`
verb talks to it instead of the `dhis2` component:

```sh
chap-devops dhis2 use https://dhis2.example.org --chap-url https://chap.example.org
```

It takes two URLs because the two directions are different:

| URL | Whose view | What it is for |
| --- | --- | --- |
| the DHIS2 URL | this machine's | every request chap-devops makes: the origin, plus the context path of a DHIS2 served under one (`https://example.org/dhis`) |
| `--chap-url` | DHIS2's | where the `chap` route points. chap-core as that DHIS2 server reaches it, including any `CHAP_ROOT_PATH` |

The route is the only reason for `--chap-url`, and chap-devops cannot work it out. The
compose alias `http://chap:8000` resolves only inside this deployment, and
`localhost` on the DHIS2 server is that server. So the route becomes
`<chap-url>/**` (`https://chap.example.org/**`), and a `--chap-url` that names
`localhost` or `127.x` is recorded with a note that it will not work. When the
DHIS2 URL is on this machine too, the note says that a DHIS2 in Docker reaches
chap-core at `http://host.docker.internal:PORT` instead. A
DHIS2 URL that is plain `http://` on another machine - not this one, not a
private address - is recorded with a note too: every request carries the
password or the token, and over `http://` they cross the network unencrypted. A
DHIS2 that redirects - `http://` to `https://` behind a proxy, say - is
followed, and its credentials go along only on the same host, never to
another one.

The record lives in `.chap-devops/components.yaml`, next to the components:

```yaml
dhis2-external:
  url: https://dhis2.example.org
  chap_url: https://chap.example.org
  connected_at: null
```

`use` records the URLs and then asks the DHIS2 two things: whether `/api/ping`
answers, and whether it accepts the credential `chap-devops dhis2` would send. It
reports both, and names the next step: the credential to set, or `chap-devops dhis2
connect`. A URL that did not answer is still recorded, since the DHIS2 may just
be down for maintenance. The report says so.

```text
dhis2      https://dhis2.example.org (answers /api/ping)
chap-url   https://chap.example.org
route      https://chap.example.org/**
login      API token from `.env` (accepted)
connected  never recorded
recorded the external DHIS2 at https://dhis2.example.org in `.chap-devops/components.yaml`
run `chap-devops dhis2 connect` to point its route at this Chap
```

| Form | What it does |
| --- | --- |
| `chap-devops dhis2 use URL --chap-url URL` | Record one, or replace the one recorded. |
| `chap-devops dhis2 use URL` / `chap-devops dhis2 use --chap-url URL` | Move one of the two URLs of the one already recorded. With `--chap-url` alone, the headline says `` changed the chap-url of the external DHIS2 at URL to CHAP_URL in `.chap-devops/components.yaml` ``. |
| `chap-devops dhis2 use` | Say which DHIS2 `chap-devops dhis2` talks to, and ask it. Writes nothing. |
| `chap-devops dhis2 use --clear` | Forget it; `chap-devops dhis2` talks to the `dhis2` component again. |

What changes against an external DHIS2:

- **`connect` sets the route and nothing else.** Installing apps and generating
  analytics tables change a server chap-devops does not run, and an analytics run on a
  national instance takes hours of its CPU. `connect` says which of the two
  apps are missing and ends with a `skipped:` line for each step it left; its
  admin runs `chap-devops dhis2 apps` and `chap-devops dhis2 analytics` when they agree.
  `connected_at` is recorded once the route is proved. If an app is missing,
  `connect` then ends with `` the Modeling App can reach Chap after the admin of
  this DHIS2 installs the Modeling App and the Climate App; `chap-devops dhis2 show`
  says when they are there ``. `chap-devops dhis2 show` ends with `` run `chap-devops dhis2
  apps` for the apps, then `chap-devops dhis2 analytics` for the analytics tables;
  the apps and analytics change this DHIS2, so ask its admin first ``.
- **Credentials have no default.** See
  [the credentials](#the-credentials-and-where-they-come-from). A token is the
  natural choice for a server you do not own the admin password of.
- **Nothing asks docker.** There is no container. The wait for `/api/ping` is
  the whole check, and a DHIS2 that does not answer points you back at the
  recorded URL rather than at `chap-devops logs dhis2`.
- **The analytics timestamp is trusted.** No seed dump of chap-devops's was restored
  into it, so `lastAnalyticsTableSuccess` is that instance's own.
- **The allowlist is that server's.** A refused route names
  `route.remote_servers_allowed` in the `dhis.conf` on the DHIS2 server, which is
  its operator's file, not one in this directory.
- **`chap-devops open dhis2` opens it**, and `chap-devops up` names `chap-devops dhis2 connect`
  until a connect is recorded against it, just as for the component.
  `connected_at` is its own record. Moving either URL forgets it, because a
  connect was a fact about one DHIS2 and one route target.

It is one DHIS2 or the other, never both. `use` refuses while the `dhis2`
component is on, and `chap-devops components enable dhis2` refuses while an external
DHIS2 is recorded. Each names the command that clears the way.

### The route is repointed, not created

**A seeded instance already has a `chap` route, and it points at a stranger's
Chap.** The climate demo dumps ship one - right code, right authority, not
disabled - aimed at an external server, so it looks fully configured. A
"create if absent" implementation would skip it and the deployment would quietly
send its data somewhere else.

`chap-devops dhis2 route` therefore compares and rewrites. It leaves the route alone
only when all three are already true, and names whichever is not:

- the URL is this deployment's chap-core;
- it is not disabled;
- it carries the `F_CHAP_MODELING_APP` authority.

```text
$ chap-devops -v dhis2 route
DHIS2 2.42.6 at http://localhost:8780, as `admin`
hint: the credential is the DHIS2 default password
repointed the `chap` route at http://chap:8000/**; chap-core answered through it
hint: the route was rewritten because it pointed at http://158.39.75.126/stable/**
hint: chap-core said: healthy
hint: run `chap-devops dhis2 apps` next, or `chap-devops dhis2 show` to see what is still missing
```

The target is `http://chap:8000/**` - the compose service alias, never
`localhost`, because the request is made by DHIS2 from inside the deployment, and
`localhost` there is DHIS2's own container. The `/**` suffix is what makes DHIS2
proxy the paths *under* the route; without it exactly one path is proxied and
everything the app asks for after `/health` answers 404. A deployment that sets
`CHAP_ROOT_PATH` gets it in the middle (`http://chap:8000/master/**`), because
that prefix moves every chap-core route with it.

Then the route is **proved rather than assumed**: `GET
/api/routes/chap/run/health` goes through DHIS2 to chap-core and back, which is
the same address the Modeling App uses. A row in DHIS2's database says nothing
about whether DHIS2 can resolve the hostname, is allowed to reach it, or gets an
answer. If nothing answers, the route is still correct and the command still
succeeds - a chap-core that is down is `chap-devops up`'s problem - so the line is a
warning and `--json` carries `"verified": false`:

```text
warning: the `chap` route is in place but nothing answered through it: HTTP 502 Bad Gateway; run `chap-devops status` to see whether chap-core is up
```

The `chap-devops status` clause is there only when chap-core does not answer on this
machine either. When chap-core answers, the line names the more probable
cause: the allowlist of DHIS2, or a chap-url that DHIS2 cannot reach. `chap-devops
dhis2 show` adds the same clause after `` missing: nothing answered through the
`chap` route: ... ``. See [The route is there but nothing answers through
it](./troubleshooting.md#the-route-is-there-but-nothing-answers-through-it).

`route.remote_servers_allowed` in
[`dhis2/dhis.conf`](#routeremote_servers_allowed) already permits every http and
https target, so the allowlist is not normally in the way. On an instance whose file was narrowed
or replaced it is, and DHIS2's refusal is turned into the line that says so:

```text
error: DHIS2 refused the route: version 42 and later only allow the origins `route.remote_servers_allowed` lists, and http://chap:8000 has to be one of them; check that line in `dhis2/dhis.conf` and run `chap-devops restart dhis2`, which recreates it to read the file again
```

DHIS2's own answer to that write is `409 Conflict` with the message `Route URL
is not permitted`, which names neither the setting nor the file - and its
`errorCode`, `E1004`, is DHIS2's general-purpose conflict code rather than this
one's, so the message above is built from the message alone. If
`chap-devops restart dhis2` does not recreate the container, use
`chap-devops restart --all dhis2`; see [Applying an edit](#applying-an-edit).

### Analytics, and the parameter that populates nothing

The Modeling and Climate apps read their figures out of DHIS2's `analytics_*`
tables, which exist only once analytics has been generated.

```sh
chap-devops dhis2 analytics
```

is `POST /api/resourceTables/analytics?skipTrackedEntities=true`, followed by
polling `GET /api/system/tasks/ANALYTICS_TABLE/{id}` until DHIS2 says the run
completed.

**No `lastYears`, ever.** It reads as an obvious optimisation and it is a trap:
measured on the climate demo, `&lastYears=8` produced **zero rows in every
analytics table** while reporting success in 16.9 seconds, where the same run
without it wrote 146,129 rows into `analytics_2024` in 15.7. A silent success
that populates nothing leaves the app with no data and nothing explaining why, so
there is no flag for it here. An instance so large that the years have to be
limited is a `curl` against DHIS2 rather than a footgun in this command.

DHIS2 runs **one** analytics job at a time, and a second `POST` queues behind the
first with an empty notifier - so a wait on it would report nothing for as long
as the first one takes. `chap-devops dhis2 analytics` therefore adopts a run that is
already going instead of asking for another:

```text
the analytics run that was already going finished in 4 minutes (job jFxL1tE0pAy)
```

DHIS2 lists a run in its notifier about five seconds after it accepts it. A
second `chap-devops dhis2 analytics` in those seconds also reads
`/api/jobConfigurations`. There it finds the analytics job that DHIS2 accepted
and has not started yet, and it adopts that job. It does not start another.

A job left `RUNNING` by a hard stop blocks every future run for good;
[`dhis2-prep`](#four-services-and-three-volumes) resets those on every start,
which is why that is not a state you have to get out of by hand.

The run is not silent: a step is printed to stderr every half minute, and `-vv`
prints every one DHIS2 announces. `--timeout SECONDS` is how long to wait, an
hour by default, and running out of it is not a cancellation - DHIS2 carries on,
and the same command watches the same job again.

### What `show` can say about analytics, and what it cannot

DHIS2 reports a `lastAnalyticsTableSuccess` in `/api/system/info`, and on a
[seeded](#the-seed) deployment **that timestamp is not about this deployment.**
It is a settings row, so it is restored with everything else: measured on a
2.42.6 with the Laos climate demo, a freshly seeded instance reported a last
success of `2026-06-16T07:51:00.093` while `analytics_2024` did not exist at all -
the table was absent, not empty. It is a fact about the database the dump was
taken from.

`chap-devops` reaches DHIS2 over HTTP and cannot look at the tables, so `show` reports
what it checked and labels the timestamp with what it is worth:

| Row | What chap-devops checked |
| --- | --- |
| `analytics  never run` | DHIS2 records no successful run at all. DHIS2 2.42 with an empty database reports `1970-01-01T00:00:00.000` instead, and chap-devops reads that time as no run too. |
| `analytics  2026-09-27 12:10 (a run finished on this deployment)` | An analytics run has finished on this DHIS2 since it started, which `GET /api/system/tasks/ANALYTICS_TABLE` says. That notifier lives in the running process, so nothing a dump carries can put an entry in it. |
| `analytics  2026-06-16 09:51 (unconfirmed on a seeded database)` | DHIS2 records a success, this deployment was seeded, and no run has finished here since DHIS2 started. The timestamp proves nothing either way. |
| `analytics  2026-09-27 12:10` | DHIS2 records a success and this deployment has no seed, so its database was migrated from empty and the record was made against it. |

DHIS2 records the time with no zone, on the clock of the server, and a
DHIS2 container runs in UTC. chap-devops reads the offset of the server from
`serverDate` in `GET /api/system/info`, and shows the time on the clock of
this machine. `--json` gives the same time in RFC 3339, with the offset of
this machine. If DHIS2 sends no `serverDate`, the time stays as DHIS2 wrote it,
with `(DHIS2 server time)` after it.

The third of those is a question and not a verdict, and it is put as one:

```text
missing: analytics may never have run on this deployment: a seeded database can carry the dump's timestamp, and no run has finished since DHIS2 started; run `chap-devops dhis2 analytics` to settle it
```

Restarting DHIS2 empties the notifier, so a seeded deployment whose run was made
before the last restart is reported `unconfirmed` again. That is the cautious
answer rather than a wrong one: running `chap-devops dhis2 analytics` a second time is
idempotent and cheap, and reading `show` as "analytics is done" when the tables
are absent is the failure this whole command exists to catch.

**A run that finished is not the same as tables with rows in them.** Nothing
here can see a row count, so nothing here claims one - which is the same
distinction the `lastYears` trap above turns on.

### The apps come from the App Hub, server-side

```sh
chap-devops dhis2 apps
```

resolves each app's newest version that this DHIS2 can run from
`https://apps.dhis2.org/api/v1/apps/{id}`, then `POST /api/appHub/{versionId}` -
and **DHIS2 downloads the app itself.** Nothing is fetched here and no multipart
upload is built.

| App | What it is for |
| --- | --- |
| Modeling App | the Chap user interface inside DHIS2 |
| DHIS2 Climate App | imports climate data into DHIS2 through Google Earth Engine, which is where the Modeling App's covariates come from - chap-core does not fetch them |

A version whose `minDhisVersion` is newer than the instance is skipped: DHIS2
installs it happily and the app then fails in the browser. An app already
installed at that version is left alone, and one at another version is moved to
it.

**A blank bound is no bound.** The App Hub sends a bound it has not set as an
empty string rather than as an absent field - every one of the Modeling App's
published versions carries `"maxDhisVersion": ""` - and chap-devops treats missing,
empty, whitespace and unreadable the same on both sides. A bound it cannot
compare against is one it cannot honour, and refusing on it would refuse
everything.

Three names for the same app, and they all differ: the App Hub publishes the
Modeling App as `Modeling`, an instance lists it under `Modeling` with the key
`dhis2-chapmodeling-app`, and `chap-devops` calls it the **Modeling App** everywhere it
prints. Matching takes the punctuation and the case out and accepts one name
containing the other, so all three spellings find each other.

`chap-devops dhis2 show` reports those two apps and no others. A 2.42.6 ships 29
bundled apps of its own, identical on every DHIS2, and they are not what this
command answers for:

```text
apps       Modeling App 7.3.0, DHIS2 Climate App not installed
missing: the Climate App is not installed
```

Installed apps live in `/opt/dhis2`, which is the `dhis2_home` volume, so they
survive a recreate and [a backup](#backing-it-up) carries them.

This is the one step that cannot work `--offline`, and it needs the network twice:
here for the version id, and from DHIS2 for the app. Under `--offline` it is
refused with what it would have needed, and `chap-devops dhis2 connect --offline` does
the other two steps and reports the skip rather than failing:

```text
skipped: installing an app needs the DHIS2 App Hub, twice: chap-devops resolves the version there and DHIS2 downloads the app itself; run the command without `--offline`, or install both apps from DHIS2's own App Management page
```

### The waits are long, and they are refusals first

Before a single request, three things are refused with what is true instead: a
deployment without the component, one whose DHIS2 publishes no host port and so
cannot be reached from this machine at all, and one whose `dhis2` container docker
says is not running. A docker that cannot be asked is not a refusal - that would
refuse on every machine docker is absent from - so the container check degrades
to a trace line and DHIS2 is asked directly.

Then `/api/ping` is polled until it answers, for twenty minutes by default
(`--wait SECONDS`), because that is what a first start can take. An external
DHIS2 recorded with `chap-devops dhis2 use` is not migrating behind this
deployment's back, so it gets one minute by default: a mistyped URL or one
behind single sign-on fails in a minute rather than in twenty. A DHIS2 that is
already answering says nothing at all; one that is not says so once:

```text
DHIS2 at http://localhost:8780 is not answering /api/ping yet; waiting up to 20 minutes, and `chap-devops logs dhis2` is where the migration shows
```

Right after `chap-devops restart dhis2`, DHIS2 can hold the `/api/ping` request
until it is ready, which took about 15 seconds on the climate demo. The command
then waits without this line.

`/api/ping` is the only route DHIS2 answers without credentials, so it is the
honest probe for "the API layer is up"; the authenticated `/api/system/info` right
after it is what settles whether the password is right, and its answer is the
version every report opens with.

## The files

```text
mychap/
  .chap-devops/
    components.yaml      intent: the dhis2 block - enabled, port, image_tag, seed
  compose.dhis2.yml      artifact: the four services and the three volumes
  dhis2/
    dhis.conf            the DHIS2 instance configuration - yours, written once
  .env                   the two generated secrets, three commented pins and the login
```

The `.env` block is appended once, when the component is first enabled, and never
rewritten:

```ini
# DHIS2 (component). Generated once; the database volume was created with them.
DHIS2_DB_PASSWORD=d82bc75bc49b4c2670235dd1e0ff1fb8
DHIS2_ENCRYPTION_PASSWORD=217ab7126f92244466434ebc5a4c7c16
# DHIS2_IMAGE_TAG=2.42
# DHIS2_DB_DUMP_URL=https://databases.dhis2.org/climate/laos/2.42/laos.sql.gz
# DHIS2_JAVA_TOOL_OPTIONS=-Xms2g -Xmx4g -XX:+UseG1GC

# DHIS2 login `chap-devops dhis2` uses. Only chap-devops reads these - no container is given
# them - and commented out means the DHIS2 default below. A personal access token,
# when set, is used instead of the username and password.
# DHIS2_API_TOKEN=
# DHIS2_ADMIN_USERNAME=admin
# DHIS2_ADMIN_PASSWORD=district
```

The commented lines each carry the value the command that reads them already
falls back to, so uncommenting one changes nothing until it is edited. The login
block is [a section of its own](#the-credentials-and-where-they-come-from) so that
`chap-devops sync` appends it to a `.env` that does not have it yet; the two variables are the whole answer to where the
credentials come from, and a name nobody can find is a name nobody sets. For a seed
that is a file rather than a URL, `DHIS2_DB_DUMP_URL` carries the path *inside*
the container (`/opt/seed.sql.gz`), because the host path would be one nothing in
there can read.

`chap-devops components disable dhis2` keeps all three volumes and names each one, and
leaves `dhis2/` alone:

```text
$ chap-devops -v components disable dhis2
disabled dhis2
stopped and removed 4 containers (dhis2, dhis2-db, dhis2-dump, dhis2-prep); host port 8780 is free again
kept volume mychap-1ab2c3_dhis2_home; remove it with `chap-devops components disable dhis2 --purge` or `docker volume rm mychap-1ab2c3_dhis2_home`
kept volume mychap-1ab2c3_dhis2_db; remove it with `chap-devops components disable dhis2 --purge` or `docker volume rm mychap-1ab2c3_dhis2_db`
kept volume mychap-1ab2c3_dhis2_dump; remove it with `chap-devops components disable dhis2 --purge` or `docker volume rm mychap-1ab2c3_dhis2_dump`
hint: the dhis2/ directory is left alone; it is yours
hint: the record of `chap-devops dhis2 connect` is forgotten with the component; a DHIS2 enabled here again is asked to connect afresh
hint: removed compose.dhis2.yml
hint: `chap-devops status` shows what runs now
```

## Backing it up

`chap-devops backup create` archives `dhis2_home` and the DHIS2 database, one member
each, and `chap-devops backup restore` puts both back.

The database is a `pg_dump`, not a copy of the `dhis2_db` volume, with the
exclusions of [dhis2-server-tools](https://github.com/dhis2/dhis2-server-tools):

- It leaves out the tables `analytics_*`, `aggregated_*`, `completeness_*` and
  `_*`. DHIS2 makes them again with the next `chap-devops dhis2 analytics`.
- It leaves out the rows of `audit`, and keeps the table.
- It does not pause the database: `pg_dump` reads one consistent snapshot while
  DHIS2 runs.

The restore makes the database new, loads the dump with `pg_restore -j 4`, and
sets the mark of a complete restore. Measured on a national dump of 22 GB, with
a volume of 62 GB after analytics:

| | Time | Size |
| --- | --- | --- |
| `chap-devops backup create` | 4 min 32 s | 4.2 GB |
| `chap-devops backup restore` | 7 min 36 s | 19 GB database, no analytics tables |

Run `chap-devops dhis2 analytics` after a restore: until then, DHIS2 has no
analytics tables, so its dashboards and the Modeling App have no data.

`dhis2_dump` is deliberately left out: it
is a download cache the one-shot refills on its own, so archiving it would add
the whole dump to every backup of the deployment for nothing. See
[Backup and restore](./backup.md).

`dhis2/` is in the archive's `files/`, on the same terms
`ocs/climate-service.yaml` is: it is scaffolded once and is yours from then on, so an
edit made to `dhis.conf` exists nowhere else, and `chap-devops sync` can only write a
fresh one. The whole directory goes in rather than the one file, because whatever
you keep beside it is yours on the same grounds, and anything nested under
`dhis2/` is archived with it.

The directory is collected whether or not `dhis2` is enabled when the backup is
taken. `chap-devops components disable dhis2` leaves `dhis2/` alone and says it is
yours, so an archive written while the component is off still has your
`dhis.conf` in it, and a restore puts it back where it was.
