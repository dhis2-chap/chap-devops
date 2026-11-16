#!/usr/bin/env bash
# Cut a release in two steps, because main is protected and takes changes
# only through pull requests:
#
#   scripts/release.sh 0.2.0 --pr     # 1. bump, verify, commit on release/v0.2.0, open the PR
#   scripts/release.sh 0.2.0 --tag    # 2. after the merge: tag the release commit on main, push the tag
#   make release-pr VERSION=0.2.0     # step 1 through make
#   make release-tag VERSION=0.2.0    # step 2 through make
#
# Without --pr or --tag, step 1 runs locally and nothing is pushed.
#
# Pushing the tag is what starts .github/workflows/release.yml, which builds
# the seven archives and creates the GitHub release. A tag push is not
# blocked by the protection of main.

set -euo pipefail

CARGO="${CARGO:-cargo}"
MAKE="${MAKE:-make}"
BRANCH="${BRANCH:-main}"
REMOTE="${REMOTE:-origin}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

usage() {
  cat <<USAGE
usage: scripts/release.sh X.Y.Z [--pr | --tag]

  X.Y.Z    the new version, without a leading "v"
  --pr     push release/vX.Y.Z and open the pull request into ${BRANCH}
  --tag    after the merge: tag the release commit on ${BRANCH} and push the tag

environment: CARGO, MAKE, BRANCH, REMOTE
USAGE
}

version=""
mode="local"

while [ $# -gt 0 ]; do
  case "$1" in
    --pr)
      mode="pr"
      ;;
    --tag)
      mode="tag"
      ;;
    --push)
      echo "error: --push is gone, because ${BRANCH} takes changes only through pull requests;" \
        "run with --pr, then with --tag after the merge" >&2
      exit 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    -*)
      echo "error: unknown flag: $1" >&2
      usage >&2
      exit 2
      ;;
    *)
      if [ -n "$version" ]; then
        echo "error: more than one version given" >&2
        exit 2
      fi
      version="$1"
      ;;
  esac
  shift
done

if [ -z "$version" ]; then
  echo "error: no version given" >&2
  usage >&2
  exit 2
fi

version="${version#v}"
if ! printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-.0-9A-Za-z]*)$'; then
  echo "error: '${version}' is not a version like 0.2.0" >&2
  exit 2
fi
tag="v${version}"
subject="chore: release ${tag}"
release_branch="release/${tag}"

if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
  echo "error: tag ${tag} already exists" >&2
  exit 1
fi

# --- step 2: tag the merged release commit ----------------------------------

if [ "$mode" = "tag" ]; then
  git fetch --quiet --tags "$REMOTE" "$BRANCH"
  if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
    echo "error: tag ${tag} already exists on ${REMOTE}" >&2
    exit 1
  fi
  # A squash merge adds " (#NN)" to the subject; a rebase merge keeps it.
  # The log is read whole first: awk stops at the first match, and a git log
  # still writing into the closed pipe would fail the pipeline (pipefail).
  history="$(git log "${REMOTE}/${BRANCH}" --format='%H %s')"
  commit="$(awk -v s="$subject" '{ t = substr($0, 42) }
    t == s || index(t, s " (#") == 1 { print $1; exit }' <<<"$history")"
  if [ -z "$commit" ]; then
    echo "error: ${REMOTE}/${BRANCH} has no commit '${subject}'; merge the release pull" \
      "request first (scripts/release.sh ${version} --pr opens it)" >&2
    exit 1
  fi
  at="$(git show "${commit}:Cargo.toml" | grep -m1 '^version = "' |
    sed 's/^version = "\(.*\)"$/\1/')"
  if [ "$at" != "$version" ]; then
    echo "error: Cargo.toml at ${commit:0:7} has version ${at}, not ${version}" >&2
    exit 1
  fi
  git tag -a "$tag" -m "chap-devops ${tag}" "$commit"
  echo "==> tagged ${commit:0:7} (${subject}) as ${tag}"
  echo "==> git push ${REMOTE} ${tag}"
  if ! git push "$REMOTE" "$tag"; then
    git tag -d "$tag" >/dev/null
    echo "error: the push of ${tag} failed, and the local tag was removed" >&2
    exit 1
  fi
  echo
  echo "the release workflow is building; follow it with:"
  echo "  gh run list --workflow release --limit 1"
  exit 0
fi

# --- step 1: the tree has to be in a releasable state -----------------------

branch="$(git rev-parse --abbrev-ref HEAD)"
if [ "$branch" != "$BRANCH" ]; then
  echo "error: on branch '${branch}', releases start from '${BRANCH}'" >&2
  exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "error: the working tree has uncommitted changes" >&2
  git status --short >&2
  exit 1
fi

if [ -n "$(git ls-files --others --exclude-standard)" ]; then
  echo "error: the working tree has untracked files" >&2
  git ls-files --others --exclude-standard >&2
  exit 1
fi

if git rev-parse -q --verify "refs/heads/${release_branch}" >/dev/null; then
  echo "error: branch ${release_branch} already exists; remove it with" \
    "git branch -D ${release_branch}" >&2
  exit 1
fi

# The release must hold everything the remote has. Checked before anything
# is bumped, while being wrong costs nothing. Only with --pr, which is the run
# that talks to the remote anyway.
if [ "$mode" = "pr" ]; then
  git fetch --quiet "$REMOTE" "$BRANCH"
  if ! git merge-base --is-ancestor "${REMOTE}/${BRANCH}" HEAD; then
    echo "error: ${REMOTE}/${BRANCH} has commits this branch does not; pull them first" >&2
    exit 1
  fi
  if ! git diff --quiet "${REMOTE}/${BRANCH}" HEAD; then
    echo "error: ${BRANCH} has commits that ${REMOTE} does not; they go through a pull" \
      "request of their own first" >&2
    exit 1
  fi
fi

current="$(grep -m1 '^version = "' Cargo.toml | sed 's/^version = "\(.*\)"$/\1/')"
if [ "$current" = "$version" ]; then
  echo "error: Cargo.toml is already at ${version}" >&2
  exit 1
fi

echo "==> releasing ${current} -> ${version} on ${release_branch}"
git switch --quiet -c "$release_branch"

# --- bump ------------------------------------------------------------------

# Only the first `version = "..."` is touched, which is the [package] one;
# dependency versions further down the file are left alone.
awk -v v="$version" '
  /^version = "/ && !done { sub(/"[^"]*"/, "\"" v "\""); done = 1 }
  { print }
' Cargo.toml > Cargo.toml.new
mv Cargo.toml.new Cargo.toml

echo "==> updating Cargo.lock"
if ! "$CARGO" update --package chap-devops --offline; then
  # A lockfile that has never been built offline may need a real resolve.
  "$CARGO" build
fi

# --- verify ----------------------------------------------------------------

echo "==> make check test"
"$MAKE" check test

# --- commit ----------------------------------------------------------------

git add Cargo.toml Cargo.lock
git commit --quiet -m "$subject"
echo
echo "==> committed '${subject}' on ${release_branch}"

if [ "$mode" = "local" ]; then
  echo
  echo "nothing has been pushed. To open the pull request:"
  echo
  echo "  git push -u ${REMOTE} ${release_branch} && gh pr create --fill --base ${BRANCH}"
  echo
  echo "To undo instead:"
  echo
  echo "  git switch ${BRANCH} && git branch -D ${release_branch}"
  exit 0
fi

echo "==> git push -u ${REMOTE} ${release_branch}"
git push --quiet -u "$REMOTE" "$release_branch"
gh pr create --base "$BRANCH" --head "$release_branch" --title "$subject" \
  --body "Bumps the version to ${version}. After the merge, run \`scripts/release.sh ${version} --tag\` to tag the release commit and start the release workflow."
git switch --quiet "$BRANCH"
echo
echo "after the checks pass and the pull request is merged, run:"
echo "  git pull && scripts/release.sh ${version} --tag"
