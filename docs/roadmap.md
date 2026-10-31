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

### A confirmation for an image outside the marketplace

`chap-admin install --image` needs `--accept-risk` each time, because nobody
reviewed the image. `chap-devops models add` asks for no confirmation. Whether it
should, and how (a flag, or a question at a terminal), is still open.

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
