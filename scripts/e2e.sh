#!/usr/bin/env bash
# End-to-end run against real images: OCS, chap-core and DHIS2 in one
# deployment, one marketplace model beside them, and every check chap-devops has.
#
#   scripts/e2e.sh                       # chap-core latest and master, DHIS2 2.41 2.42 2.43 dev
#   scripts/e2e.sh --tags "v2.3.1"       # one tag
#   scripts/e2e.sh --keep                # leave the deployments running
#   scripts/e2e.sh --no-dhis2            # OCS and chap-core only
#   scripts/e2e.sh --models all          # every marketplace model, not only the one
#   scripts/e2e.sh --dhis2 2.42          # one DHIS2 version (dev: dhis2/core-dev:master)
#   scripts/e2e.sh --chap-admin          # the chap-admin commands, and a takeover, per tag
#
# For each chap-core tag and DHIS2 version it creates a deployment under the
# work directory,
# starts it, waits until every service answers, and then runs, in order:
# doctor, an OCS check, an OCS ingestion of a few days of CHIRPS3 (public, no
# credentials), dhis2 connect, models test (every model trains and predicts),
# models test --backtest (a dataset, a backtest and its scores through
# chap-core) and the chap CLI through `chap-devops chap` (an evaluation and its
# plot). A failed step collects the logs and the rest of that tag
# carries on where it still can. A report is written next to the deployments.
#
# Needs Docker with room for DHIS2 (about 8 GB), and the network. Takes a
# while: image pulls, DHIS2's first start and the backtests are minutes each.

set -uo pipefail

CHAP_DEVOPS="${CHAP_DEVOPS:-}"
TAGS="latest master"
MODELS="chapkit_simple_multistep_model"
DHIS2_VERSIONS="2.41 2.42 2.43 dev"
KEEP=0
DHIS2=1
ADMIN=0
WORK=""
WAIT_SECONDS="${WAIT_SECONDS:-1800}"

usage() {
  awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
  cat <<USAGE

options:
  --tags "T1 T2"   chap-core tags to run, in order (default: "latest master")
  --keep           leave each deployment running afterwards
  --dhis2 "V1 V2"  DHIS2 versions to run each tag with (default: "2.41 2.42 2.43 dev");
                   dev is the unreleased next one, dhis2/core-dev:master
  --no-dhis2       leave DHIS2 out, and dhis2 connect with it
  --chap-admin     run what chap-admin does, with chap-core only: install, update, a
                   custom image, uninstall, and a takeover of a chap-core compose deployment
  --models LIST    model ids, comma separated, \`none\` or \`all\` (default: chapkit_simple_multistep_model)
  --work DIR       where the deployments and the report go (default: a temp dir)

environment: CHAP_DEVOPS (the chap-devops binary), WAIT_SECONDS (default 1800)
USAGE
}

while [ $# -gt 0 ]; do
  case "$1" in
    --tags) TAGS="$2"; shift ;;
    --keep) KEEP=1 ;;
    --models) MODELS="$2"; shift ;;
    --dhis2) DHIS2_VERSIONS="$2"; shift ;;
    --no-dhis2) DHIS2=0 ;;
    --chap-admin) ADMIN=1 ;;
    --work) WORK="$2"; shift ;;
    -h | --help) usage; exit 0 ;;
    *) echo "error: unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [ -z "$CHAP_DEVOPS" ]; then
  if [ -x "$root/target/debug/chap-devops" ]; then
    CHAP_DEVOPS="$root/target/debug/chap-devops"
  else
    CHAP_DEVOPS="$(command -v chap-devops || true)"
  fi
fi
if [ -z "$CHAP_DEVOPS" ] || [ ! -x "$CHAP_DEVOPS" ]; then
  echo "error: no chap-devops binary; run \`cargo build\` or set CHAP_DEVOPS=/path/to/chap-devops" >&2
  exit 2
fi
if ! docker info >/dev/null 2>&1; then
  echo "error: docker is not answering; start Docker and run this again" >&2
  exit 2
fi
if [ -z "$WORK" ]; then
  WORK="$(mktemp -d "${TMPDIR:-/tmp}/chap-devops-e2e.XXXXXX")"
fi
mkdir -p "$WORK"
report="$WORK/report.md"
{
  echo "# chap-devops end-to-end run"
  echo
  echo "- chap-devops: $("$CHAP_DEVOPS" --version)"
  echo "- started: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "- tags: $TAGS"
  echo "- models: $MODELS"
  [ "$DHIS2" = 1 ] && echo "- DHIS2: $DHIS2_VERSIONS"
  echo
  echo "| run | step | result | seconds |"
  echo "| --- | --- | --- | --- |"
} >"$report"

failures=0

# run TAG STEP LOGFILE COMMAND...: time one step, record it, and keep its output.
run() {
  local tag="$1" step="$2" log="$3"
  shift 3
  local started=$SECONDS
  printf '  %-22s ' "$step"
  if "$@" >"$log" 2>&1; then
    local took=$((SECONDS - started))
    echo "pass (${took}s)"
    echo "| $tag | $step | pass | $took |" >>"$report"
    return 0
  fi
  local took=$((SECONDS - started))
  echo "FAIL (${took}s) - $log"
  echo "| $tag | $step | **fail** | $took |" >>"$report"
  failures=$((failures + 1))
  return 1
}

# wait_up DIR: until `chap-devops status` exits 0, which is every service up and
# every model registered, or WAIT_SECONDS runs out.
wait_up() {
  local dir="$1" deadline=$((SECONDS + WAIT_SECONDS))
  while [ $SECONDS -lt $deadline ]; do
    if "$CHAP_DEVOPS" -C "$dir" status; then
      return 0
    fi
    sleep 15
  done
  "$CHAP_DEVOPS" -C "$dir" status
}

# ocs_url DIR: OCS's base URL, as chap-devops status reports its health endpoint.
ocs_url() {
  local url
  url="$("$CHAP_DEVOPS" -C "$1" status --json | sed -n 's/.*"health_url": *"\([^"]*\)".*/\1/p' | head -1)"
  [ -n "$url" ] || return 1
  echo "${url%/health}"
}

# get URL: fetch one URL, fail on anything but a 2xx, and show the start of it.
get() {
  local body
  echo "GET $1"
  body="$(curl -fsS "$1")" || return 1
  printf '%s\n' "${body:0:1500}"
}

# ocs_check DIR: OCS answers its health endpoint and serves its STAC catalog.
ocs_check() {
  local base
  base="$(ocs_url "$1")" || { echo "no OCS health_url in chap-devops status --json"; return 1; }
  get "$base/health" && get "$base/stac/catalog.json"
}

# ocs_ingest DIR: three days of CHIRPS3 over the instance's extent, published,
# and the collection then listed by STAC.
ocs_ingest() {
  local base
  base="$(ocs_url "$1")" || { echo "no OCS health_url in chap-devops status --json"; return 1; }
  echo "POST $base/ingestions"
  curl -fsS --max-time 1800 -X POST "$base/ingestions" \
    -H 'Content-Type: application/json' \
    -d '{"dataset_id": "chirps3_precipitation_daily", "start": "2024-01-01", "end": "2024-01-03", "publish": true}' ||
    return 1
  echo
  get "$base/stac/collections/chirps3_precipitation_daily"
}

# chap_cli DIR LOGS: the chap CLI through `chap-devops chap`, in a directory of its
# own: an evaluation of the deployment's first model over its network, on
# chap-core's Laos example data, then a plot of it. Both files must be on the
# host afterwards.
chap_cli() {
  local dir="$1" work="$2/chap-cli" service data
  data="https://raw.githubusercontent.com/dhis2-chap/chap-core/master/example_data"
  service="$(sed -n 's/^ *service_id: *//p' "$dir/.chap-devops/models.yaml" | head -1)"
  [ -n "$service" ] || { echo "no enabled model in $dir/.chap-devops/models.yaml"; return 1; }
  mkdir -p "$work" || return 1
  (
    cd "$work" || exit 1
    curl -fsSO "$data/laos_subset.csv" && curl -fsSO "$data/laos_subset.geojson" || exit 1
    "$CHAP_DEVOPS" -C "$dir" chap eval --model-name "http://$service:8000" \
      --dataset-csv laos_subset.csv --output-file eval.nc \
      --backtest-params.n-splits 2 --backtest-params.n-periods 3 || exit 1
    "$CHAP_DEVOPS" -C "$dir" chap plot-backtest eval.nc --output-file eval.html || exit 1
    test -s eval.nc && test -s eval.html
  )
}

# dhis2_seed VERSION: chap-devops's own seed where it has one (the Laos climate demo,
# 2.42), and otherwise an empty database. DHIS2's Sierra Leone demo exists for
# other versions, but its admin is not a superuser and may not create the
# route `chap-devops dhis2 connect` writes; an empty DHIS2's admin is one.
dhis2_seed() {
  case "$1" in
    2.42 | 2.42.*) echo default ;;
    *) echo none ;;
  esac
}

# model_state DIR SERVICE STATE [VERSION]: the model row of `chap-devops status`
# has STATE, and chap-core has it registered at VERSION when one is given.
model_state() {
  "$CHAP_DEVOPS" -C "$1" --json status | python3 -I -c '
import json, sys
service, state, version = sys.argv[1], sys.argv[2], sys.argv[3]
report = json.load(sys.stdin)
row = [m for m in report["models"] if m["id"] == service]
if state == "absent":
    sys.exit(0 if not row else 1)
if not row or row[0]["state"] != state:
    sys.exit("state of %s: %s" % (service, row[0]["state"] if row else "no row"))
if version and not any(r["id"] == service and r["version"] == version for r in report["registered"]):
    sys.exit("%s is not registered at %s" % (service, version))
' "$2" "$3" "${4:-}"
}

# custom_model WORK: a model project from `chap-devops models new`, built as
# e2e-model:dev, as option 11 of docs/ai.md builds one.
custom_model() {
  local work="$1/e2e-model"
  rm -rf "$work"
  "$CHAP_DEVOPS" models new "$work" || return 1
  (
    cd "$work" || exit 1
    git init -q && git add . && git -c user.name=e2e -c user.email=e2e@localhost \
      -c commit.gpgsign=false commit -qm "feat: first version" || exit 1
    docker build --platform linux/amd64 --build-arg "GIT_REVISION=$(git rev-parse HEAD)" \
      -t e2e-model:dev .
  )
}

# old_deployment DIR TAG PORT: chap-core's own compose deployment, started and
# stopped again, as `chap-devops init --adopt` expects to find it.
old_deployment() {
  local dir="$1" tag="$2" port="$3" ref=master
  case "$tag" in v*) ref="$tag" ;; esac
  rm -rf "$dir" && mkdir -p "$dir" || return 1
  curl -fsSL -o "$dir/compose.ghcr.yml" \
    "https://raw.githubusercontent.com/dhis2-chap/chap-core/$ref/compose.ghcr.yml" || return 1
  printf 'CHAP_IMAGE_TAG=%s\n' "$tag" >"$dir/.env"
  printf 'services:\n  chap:\n    ports: !override\n      - "%s:8000"\n' "$port" \
    >"$dir/compose.port.yml"
  (
    cd "$dir" || exit 1
    docker compose -f compose.ghcr.yml -f compose.port.yml up -d --wait --wait-timeout 600 &&
      docker compose -f compose.ghcr.yml -f compose.port.yml down
  )
}

# admin_steps LABEL DIR LOGS: what chap-admin does, with chap-devops.
admin_steps() {
  local label="$1" dir="$2" logs="$3" model=chapkit_simple_multistep_model
  local service=chapkit-simple-multistep-model tag="${run_id%%/*}"
  run "$label" init "$logs/init.log" \
    "$CHAP_DEVOPS" init "$dir" --force --chap-tag "$tag" --models none --api-port "$api" \
    --port-base "$base" || return
  run "$label" install "$logs/install.log" sh -c \
    '"$1" -C "$2" models enable "$3" --version 0.1.3 && "$1" -C "$2" up --wait' \
    - "$CHAP_DEVOPS" "$dir" "$model"
  run "$label" "install check" "$logs/install-check.log" \
    model_state "$dir" "$service" registered 0.1.3
  run "$label" update "$logs/update.log" sh -c \
    '"$1" -C "$2" models enable "$3" --channel stable && "$1" -C "$2" up --wait' \
    - "$CHAP_DEVOPS" "$dir" "$model"
  run "$label" "update check" "$logs/update-check.log" \
    model_state "$dir" "$service" registered 0.1.4
  if run "$label" "custom build" "$logs/custom-build.log" custom_model "$logs"; then
    run "$label" "custom image" "$logs/custom.log" sh -c \
      '"$1" -C "$2" models add e2e-model:dev && "$1" -C "$2" up --wait' \
      - "$CHAP_DEVOPS" "$dir"
    run "$label" "custom test" "$logs/custom-test.log" \
      "$CHAP_DEVOPS" -C "$dir" models test e2e_model -v
  fi
  run "$label" uninstall "$logs/uninstall.log" sh -c \
    '"$1" -C "$2" models disable "$3" --retire --purge && "$1" -C "$2" up' \
    - "$CHAP_DEVOPS" "$dir" "$model"
  run "$label" "uninstall check" "$logs/uninstall-check.log" \
    model_state "$dir" "$service" absent
  "$CHAP_DEVOPS" -C "$dir" down --volumes --yes >"$logs/down.log" 2>&1

  local old="$WORK/e2eold${offset}" new="$WORK/e2enew${offset}"
  run "$label" "old deployment" "$logs/old.log" old_deployment "$old" "$tag" "$((api + 50))" ||
    return
  run "$label" takeover "$logs/takeover.log" \
    "$CHAP_DEVOPS" init "$new" --adopt "$old" --api-port "$((api + 50))" || return
  run "$label" "takeover up" "$logs/takeover-up.log" "$CHAP_DEVOPS" -C "$new" up --wait
  run "$label" "takeover check" "$logs/takeover-check.log" "$CHAP_DEVOPS" -C "$new" status
  if [ "$KEEP" = 0 ]; then
    "$CHAP_DEVOPS" -C "$new" down --volumes --yes >"$logs/takeover-down.log" 2>&1
  fi
}

# steps LABEL DIR LOGS: every step for one deployment, in order.
steps() {
  local label="$1" dir="$2" logs="$3"
  run "$label" init "$logs/init.log" \
    "$CHAP_DEVOPS" init "$dir" --force --chap-tag "${run_id%%/*}" --models "$MODELS" --with "$with" \
    --api-port "$api" --ocs-port "$ocs" --port-base "$base" "${extra[@]}" || return
  run "$label" up "$logs/up.log" "$CHAP_DEVOPS" -C "$dir" up || {
    "$CHAP_DEVOPS" -C "$dir" logs >"$logs/compose.log" 2>&1
    return
  }
  if ! run "$label" "wait until up" "$logs/status.log" wait_up "$dir"; then
    "$CHAP_DEVOPS" -C "$dir" logs >"$logs/compose.log" 2>&1
  fi
  run "$label" doctor "$logs/doctor.log" "$CHAP_DEVOPS" -C "$dir" doctor
  run "$label" ocs "$logs/ocs.log" ocs_check "$dir"
  run "$label" "ocs ingest" "$logs/ocs-ingest.log" ocs_ingest "$dir"
  if [ "$version" != - ]; then
    run "$label" "dhis2 connect" "$logs/dhis2-connect.log" "$CHAP_DEVOPS" -C "$dir" dhis2 connect
  fi
  run "$label" "models test" "$logs/models-test.log" \
    "$CHAP_DEVOPS" -C "$dir" models test --all -v
  run "$label" "models backtest" "$logs/models-backtest.log" \
    "$CHAP_DEVOPS" -C "$dir" models test --all --backtest -v
  run "$label" "chap cli" "$logs/chap-cli.log" chap_cli "$dir" "$logs"
  "$CHAP_DEVOPS" -C "$dir" status --json >"$logs/status.json" 2>&1
  if [ "$failures" -gt 0 ]; then
    "$CHAP_DEVOPS" -C "$dir" logs >"$logs/compose.log" 2>&1
  fi
}

runs=()
for tag in $TAGS; do
  if [ "$ADMIN" = 1 ]; then
    runs+=("$tag/admin")
  elif [ "$DHIS2" = 1 ]; then
    for version in $DHIS2_VERSIONS; do runs+=("$tag/$version"); done
  else
    runs+=("$tag/-")
  fi
done

offset=0
for run_id in "${runs[@]}"; do
  version="${run_id#*/}"
  name="chap-${run_id%%/*}"
  case "$version" in
    -) ;;
    admin) name="$name-admin" ;;
    *) name="$name-dhis2-$version" ;;
  esac
  dir="$WORK/$name"
  logs="$WORK/logs-$name"
  mkdir -p "$logs"
  api=$((18000 + offset))
  dhis=$((18080 + offset))
  base=$((18100 + offset))
  ocs=$((20000 + offset))
  offset=$((offset + 300))

  if [ "$version" = admin ]; then
    echo "== chap-core ${run_id%%/*}, the chap-admin commands  ($dir)"
    admin_steps "$run_id" "$dir" "$logs"
    if [ "$KEEP" = 0 ]; then
      "$CHAP_DEVOPS" -C "$dir" down --volumes --yes >>"$logs/down.log" 2>&1
    fi
    continue
  fi
  echo "== chap-core ${run_id%%/*}, DHIS2 ${version/-/off}  ($dir)"
  with="ocs,s3"
  extra=()
  if [ "$version" != - ]; then
    with="ocs,s3,dhis2"
    if [ "$version" = dev ]; then
      extra=(--dhis2-port "$dhis" --dhis2-image dhis2/core-dev --dhis2-tag master --dhis2-seed none)
    else
      extra=(--dhis2-port "$dhis" --dhis2-tag "$version" --dhis2-seed "$(dhis2_seed "$version")")
    fi
  fi
  steps "$run_id" "$dir" "$logs"
  if [ "$KEEP" = 0 ]; then
    "$CHAP_DEVOPS" -C "$dir" down --volumes --yes >"$logs/down.log" 2>&1
  fi
done

{
  echo
  echo "- finished: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "- failed steps: $failures"
} >>"$report"
echo
echo "report: $report"
if [ "$failures" -gt 0 ]; then
  echo "$failures step(s) failed; the log of each is named in the report's directory"
  exit 1
fi
echo "every step passed"
