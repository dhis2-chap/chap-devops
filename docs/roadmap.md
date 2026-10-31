# Roadmap

What is planned for chap-devops, and what is not. The order is not a promise.

## Planned

### Resource limits for each service

Today the compose files set no memory or CPU limits, except the heap of
DHIS2 (`DHIS2_JAVA_TOOL_OPTIONS` in `.env`). A large DHIS2 database, such as a
national dump, needs more memory for PostgreSQL and for DHIS2 than the demo
dumps do. The plan:

- Settings in `.chap-devops/components.yaml` and `.chap-devops/models.yaml` for the
  memory and the CPUs of each service, rendered as `mem_limit`, `cpus` and,
  for PostgreSQL, `shared_buffers` and `work_mem`.
- A preset for a large DHIS2 database.
- A `chap-devops doctor` check that compares the limits with the memory that docker
  can give.

### A longer wait for a large seed

The health check of `dhis2-db` waits at most about 15 minutes. A large
dump takes longer, so `chap-devops up` stops with "unhealthy" while the restore
continues. The wait must grow with the size of the dump.

### Levels for the output of every command

Most commands print their closing lines with a level (info, warning, hint),
as [Status and output](./status.md#levels) describes. `chap-devops api` and
`chap-devops chap` still print their closing lines on stderr with no level. They
change to the levels next.

### Templates for the compose files

The large compose files are templates in `src/compose/templates/`, but the
optional parts are built in Rust: the seed one-shot, the port lines, the
mounts. The plan is to use minijinja (Jinja2 for Rust) so that the conditions
and the loops are in the template files, and the Rust code only gives the
values. The golden files in `tests/fixtures/` make sure that the output does
not change.

### The checks of chap-admin

`chap-devops` installs, updates and removes models as `chap-admin` does, but
it checks less. The plan:

- Check that the `image_tag` of a version is `sha-<prefix>` of its
  `commit`.
- Check that the `id` of a marketplace entry matches its file name, and that
  its `service_id` is the id with `-` for `_`.
- After a model registers, compare its service id and its `git_revision`
  with the pin, and warn when they differ.
- Before `chap-devops models add` registers an image, refuse a template name that
  is already live in chap-core and that this deployment does not own.

### A confirmation for an image outside the marketplace

`chap-admin install --image` needs `--accept-risk` each time, because nobody
reviewed the image. `chap-devops models add` asks for no confirmation. Whether it
should, and how (a flag, or a question at a terminal), is still open.

### Undo of a failed update, for the container too

When a configure step fails, `chap-devops` retires the template version it
stored, so chap-core is as it was. `chap-admin` also stops the new container
and starts the previous image again. `chap-devops` leaves the new container
running.

### The development overlays in a takeover

`chap-devops init --adopt` reads `compose.marketplace.yml` only. A deployment that
also ran chap-core's `compose.ewars.yml` or `compose.chapkit.yml` loses those
services in the takeover. They are for development, but the takeover must
at least name them.

### chap-devops for chap-admin users

A page that maps each `chap-admin` command and flag to the `chap-devops`
command that does the same, and a scenario in `scripts/e2e.sh` that runs
each of them against real images: install, update, uninstall, a custom image
and a takeover.

### Chap Hub

The marketplace now calls itself Chap Hub in its `registry.yaml`. The help,
the messages and the docs change to that name when the Chap team confirms it.

## Not planned

### Native Windows

chap-devops runs on Windows in WSL 2, with the Linux binary. The native Windows
builds are published, but they are not supported, and there is no PowerShell
installer. These parts are ready:

- the cache and data directories under `%LOCALAPPDATA%\chap-devops`;
- `chap-devops open`, Ctrl-C and `chap-devops self update`.

Native support would also need a Windows job in CI, a test double for docker
that is not a shell script, a path map for the mounts of `chap-devops chap`, and a
signed binary.
