#!/usr/bin/env bash
# Create dhis2-chap/chap-devops on GitHub and put it in a publishable state.
#
#   scripts/bootstrap-repo.sh          # print what would run, change nothing
#   scripts/bootstrap-repo.sh --yes    # actually run it
#
# This is a one-time script. It creates the repository, pushes main, switches
# GitHub Pages to the Actions build source so .github/workflows/docs.yml can
# deploy the book, points the repository's About link at the published site,
# and finally hands over to scripts/github-secrets.sh for the Apple signing
# secrets.
#
# Every command is printed before it runs, so a dry run doubles as the
# checklist for doing it by hand.
#
# Prerequisites: `gh auth login` with rights to create repositories in the
# organisation, and a local `main` branch with at least one commit.

set -euo pipefail

ORG="${ORG:-dhis2-chap}"
REPO_NAME="${REPO_NAME:-chap-devops}"
REPO="${REPO:-${ORG}/${REPO_NAME}}"
BRANCH="${BRANCH:-main}"
PAGES_URL="${PAGES_URL:-https://${ORG}.github.io/${REPO_NAME}/}"
# Kept in step with the `description` in Cargo.toml.
DESCRIPTION="${DESCRIPTION:-Deploy and manage Chap (climate-informed disease forecasting) with Docker Compose}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

usage() {
  cat <<USAGE
usage: scripts/bootstrap-repo.sh [--yes] [--skip-secrets]

  --yes            run the commands; without it nothing is executed
  --skip-secrets   do not call scripts/github-secrets.sh at the end

environment: ORG, REPO_NAME, REPO, BRANCH, PAGES_URL, DESCRIPTION
USAGE
}

execute=0
secrets=1

while [ $# -gt 0 ]; do
  case "$1" in
    --yes | -y)
      execute=1
      ;;
    --dry-run)
      execute=0
      ;;
    --skip-secrets)
      secrets=0
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

# Print the command the way it would be typed, then run it if --yes was given.
run() {
  printf '\n$'
  for argument in "$@"; do
    case "$argument" in
      *[!A-Za-z0-9._/:=@-]*) printf " '%s'" "$argument" ;;
      *) printf ' %s' "$argument" ;;
    esac
  done
  printf '\n'
  if [ "$execute" -eq 1 ]; then
    "$@"
  fi
}

if [ "$execute" -eq 0 ]; then
  echo "dry run: the commands below are printed, not executed."
  echo "Re-run with --yes to execute them."
fi

echo
echo "repository: ${REPO}"
echo "branch:     ${BRANCH}"
echo "pages:      ${PAGES_URL}"

if ! command -v gh >/dev/null 2>&1; then
  echo "error: gh is not installed or not on PATH" >&2
  exit 1
fi

# 1. Create the repository from this checkout and wire up the origin remote.
run gh repo create "$REPO" \
  --public \
  --source . \
  --remote origin \
  --description "$DESCRIPTION" \
  --disable-wiki

# 2. Push main and set it as the upstream.
run git push -u origin "$BRANCH"

# 3. Build Pages from the Actions workflow rather than from a branch. Without
#    this, .github/workflows/docs.yml deploys into a Pages site that does not
#    exist yet and fails. It is a one-time switch.
run gh api -X POST "repos/${REPO}/pages" -f build_type=workflow

# 4. The About box on the repository page links to the book.
run gh repo edit "$REPO" --homepage "$PAGES_URL"

# 5. The Apple signing secrets, read from 1Password.
if [ "$secrets" -eq 1 ]; then
  if [ "$execute" -eq 1 ]; then
    run scripts/github-secrets.sh
  else
    printf '\n$ scripts/github-secrets.sh\n'
  fi
else
  echo
  echo "skipping scripts/github-secrets.sh (--skip-secrets)"
fi

echo
if [ "$execute" -eq 1 ]; then
  echo "done. Next:"
  echo "  - the docs workflow deploys the book on the next push to ${BRANCH}"
  echo "  - cut the first release with: make release-tag VERSION=0.1.0"
else
  echo "nothing was executed. Re-run with --yes."
fi
