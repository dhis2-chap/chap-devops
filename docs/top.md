# Watching everything: chap-devops top

`chap-devops top` shows every chap-devops deployment on this machine as one tree that keeps
itself up to date: the deployments made with `chap-devops init` and the
[`chap-devops run` groups](./run.md) alike, each with its services under it -
chap-core and its worker and databases, DHIS2, OCS, the object store and the
models - and what each container is doing.

```text
 chap-devops top  3 deployments  8/12 running  at 13:17:51 (every 2s)
┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│DEPLOYMENT / SERVICE                                                  STATE                  CPU      MEMORY                 URL                              │
│▾ mychap  init                                                        6/9 running                                            /home/me/mychap                  │
│  ├─ chap                                                             running (healthy)      6.52%    442.9MiB / 31.29GiB    http://localhost:8700            │
│  ├─ postgres                                                         running (healthy)      0.02%    35.27MiB / 31.29GiB                                     │
│  ├─ redis                                                            running (healthy)      0.28%    12.56MiB / 31.29GiB                                     │
│  ├─ worker                                                           running (healthy)      0.79%    1.118GiB / 31.29GiB                                     │
│  ├─ dhis2                                                            running (healthy)      2.82%    1.982GiB / 31.29GiB    http://localhost:8780            │
│  ├─ dhis2-db                                                         running (healthy)      0.04%    71.71MiB / 31.29GiB                                     │
│  ├─ dhis2-dump                                                       exited                                                                                  │
│  ├─ dhis2-prep                                                       exited                                                                                  │
│  └─ chapkit-simple-multistep-model (chapkit_simple_multistep_model)  exited                                                 http://localhost:5001            │
│▸ run/default  run                                                    1/1 running                                            /home/me/.local/share/chap-devops/run/ │
│▾ run/trial  run                                                      1/2 running                                            /home/me/.local/share/chap-devops/run/ │
│  ├─ auto-arima-chapkit (auto_arima_chapkit)                          running (healthy)      0.43%    108.9MiB / 31.29GiB    http://localhost:5003            │
│  └─ chapkit-simple-multistep-model (chapkit_simple_multistep_model)  exited                                                 http://localhost:5001            │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
 j/k move  enter fold  l logs  s stop model  r refresh  q quit
```

| Key | What it does |
| --- | --- |
| `j`/`k`, the arrows | Move. |
| `enter`, space | Fold or unfold the deployment under the cursor. |
| `l` | Show the last 200 lines of the service's log in a pane underneath, colour codes removed; `l` or `esc` closes it. The log is asked for on a thread of its own, so the pane says `asking docker ...` until it arrives and a slow daemon never holds up a key. |
| `s` | Stop the model under the cursor, after a `y`: the same as `chap-devops stop`, which takes the model out of its deployment - its overlay goes and `chap-devops up` no longer starts it; its data stays. The prompt says so. `chap-devops run` or `chap-devops models enable` brings it back. Only a model stops here; a whole deployment stops with `chap-devops -C DIR down`, which the footer names. |
| `r` | Look again now rather than at the next tick. |
| `q`, `esc` | Quit. |

`--interval SECONDS` (2 by default, at most 3600) sets how often docker is asked. Docker is
asked from a thread of its own, so a slow daemon never holds up a key.

## How it finds them

Every container chap-devops starts carries labels that say what it is (see
[Container labels](./concepts.md#container-labels)), so one
`docker ps -a --filter label=io.github.dhis2-chap.chap-devops.role` finds them all, on any
deployment, wherever its directory is. Compose's own labels say which
deployment and which service each one is, and the directory compose ran in;
from that directory `chap-devops top` reads the deployment's state for the URLs. A
deployment whose directory is gone still shows, without them.

The deployment the command runs in, and every `chap-devops run` group with a model
enabled, show even with nothing running: their enabled models are listed as
`not running`. A group with no model is left out.

## In a script

Off a terminal - piped, or under `--json` - `chap-devops top` prints one snapshot of
the same tree and exits:

```sh
chap-devops top --json | jq '.deployments[] | {name, kind, services: [.services[] | {service, state, cpu}]}'
```

Each deployment has `project` (the compose project name), `name`, `kind`
(`init` or `run`), `group`, `dir` and `services`; each service has `service`,
`role`, `model`, `state`, `health`, `cpu`, `memory` and `url`. For the models
of one deployment with their registration state, [`chap-devops ps`](./run.md) and
`chap-devops status` say more.
