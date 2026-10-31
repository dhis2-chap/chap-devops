#!/usr/bin/env bash
# Write the body of a GitHub release, for a `vX.Y.Z` tag or for the rolling
# `dev` pre-release, to stdout.
#
#   scripts/release-notes.sh v0.2.3            # notes for that tag
#   scripts/release-notes.sh v0.2.3 v0.2.1     # ... against a given previous tag
#   scripts/release-notes.sh dev               # the rolling build of main
#
# `.github/workflows/release.yml` redirects the output into notes.md and hands
# that to the release action. It lives here rather than in the workflow so the
# notes for a tag can be read before the tag is pushed:
#
#   scripts/release-notes.sh v0.2.3 | less
#
# What changed is taken from the commit subjects, grouped by their Conventional
# Commit type. This repository pushes to `main` rather than merging pull
# requests, so GitHub's own generated notes have nothing to list and are turned
# off in the workflow.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# Only version tags are candidates for "the previous tag": the rolling build
# publishes under the moving tag `dev`, which sits on a `main` commit and would
# otherwise be what `git describe` finds.
tag_glob='v[0-9]*'

usage() {
  cat <<USAGE
usage: scripts/release-notes.sh <tag> [previous-tag]

  <tag>            a version tag, e.g. v0.2.3, or dev for the rolling build
  [previous-tag]   what to compare against; the default is the version tag
                   before <tag>, or the last version tag for dev

environment: GITHUB_REPOSITORY, GITHUB_SHA
USAGE
}

die() {
  echo "release-notes: $1" >&2
  exit 1
}

tag="${1:-}"
previous="${2:-}"

case "$tag" in
  "" | -h | --help)
    usage
    [ -n "$tag" ] && exit 0
    exit 1
    ;;
esac

repo="${GITHUB_REPOSITORY:-dhis2-chap/chap-devops}"
script="https://raw.githubusercontent.com/${repo}/main/install.sh"
base="https://github.com/${repo}/releases/download/${tag}"

# The commit subjects to report, and what the compare link spans.
if [ "$tag" = dev ]; then
  previous="${previous:-$(git describe --tags --abbrev=0 --match "$tag_glob" HEAD 2>/dev/null || true)}"
  span="HEAD"
else
  git rev-parse -q --verify "refs/tags/${tag}" >/dev/null ||
    die "no such tag in this checkout: $tag"
  previous="${previous:-$(git describe --tags --abbrev=0 --match "$tag_glob" "${tag}^" 2>/dev/null || true)}"
  span="$tag"
fi

if [ -n "$previous" ]; then
  git rev-parse -q --verify "refs/tags/${previous}" >/dev/null ||
    die "no such tag in this checkout: $previous"
  range="${previous}..${span}"
  compare="https://github.com/${repo}/compare/${previous}...${tag}"
else
  # The first tag: everything up to it, and no compare link to draw.
  range="$span"
  compare="https://github.com/${repo}/commits/${tag}"
fi

subjects="$(git log --no-merges --format='%s' "$range" --)"

# The groups, in the order they are printed: heading, then the Conventional
# Commit types that land in it. What changed for a user comes first, then the
# docs, then the changes that alter no behaviour, then the repository's own
# plumbing. A refactor belongs in that second-to-last group and not in the
# tooling one: it is usually the groundwork a later feature sits on, and filed
# as tooling a reader scanning the notes takes it for build noise. Anything
# left over goes under "Other changes", so a type nobody thought of here is
# still reported rather than dropped.
group_headings=(
  "Features"
  "Fixes"
  "Performance"
  "Reverts"
  "Documentation"
  "Internal changes"
  "CI and tooling"
)
group_types=(
  "feat"
  "fix"
  "perf"
  "revert"
  "docs"
  "refactor style test"
  "ci build chore"
)
# Every type the groups above claim, derived rather than repeated: a type
# listed here but in no group would be dropped from the notes silently, and one
# in a group but not here would be reported twice.
known_types="${group_types[*]}"

# The bullets for one group, or nothing at all when no commit belongs to it.
# $1 is a space-separated list of types, or "other" for the leftovers.
bullets_for() {
  local want="$1"
  local subject type scope rest first out=""
  # type, optional (scope), optional ! for a breaking change, then the subject.
  # In a variable because shellcheck cannot parse the literal in place.
  local conventional='^([a-z]+)(\(([^)]*)\))?(!)?:[[:space:]]+(.*)$'

  while IFS= read -r subject; do
    [ -n "$subject" ] || continue

    if [[ "$subject" =~ $conventional ]]; then
      type="${BASH_REMATCH[1]}"
      scope="${BASH_REMATCH[3]}"
      rest="${BASH_REMATCH[5]}"
      [ -n "${BASH_REMATCH[4]}" ] && rest="${rest} (breaking change)"
    else
      type=""
      scope=""
      rest="$subject"
    fi

    # `chore: release v0.2.3` is the version bump the tag sits on: it says
    # nothing about what changed.
    if [ "$type" = chore ] && [[ "$rest" == release || "$rest" == release\ * ]]; then
      continue
    fi

    if [ "$want" = other ]; then
      # Everything whose type no group above claims, including a subject that
      # is not a Conventional Commit at all.
      case " $known_types " in *" ${type:-none} "*) continue ;; esac
    else
      case " $want " in *" $type "*) ;; *) continue ;; esac
    fi

    # Capitalise the first letter, the way a heading-less bullet list reads
    # best, without relying on bash 4's ${var^}. Not a name that is written in
    # lower case: "chap-devops chap takes ..." stays as the command is typed.
    # A subject that starts with a command name, "models test ..." or
    # "ps, stop and run ...", is about that command: it gets `chap-devops` in
    # front and keeps its case.
    word="${rest%% *}"
    case "${word%,}" in
      models | components | ps | status | dhis2 | backup | init | up | down | \
        logs | jobs | api | doctor | cleanup | auth | self | completions | \
        top | ui | registry)
        rest="chap-devops ${rest}"
        ;;
      # A verb that is also a command reads as the command only at the head
      # of a list: "run, ps, stop and open ...".
      run | stop | open | update | sync | restart | pull)
        [ "${word%,}" != "$word" ] && rest="chap-devops ${rest}"
        ;;
    esac
    case "${rest%% *}" in
      chap-devops | cdx | chap | chap-core | chapkit | docker | compose) ;;
      *)
        first="$(printf '%s' "${rest:0:1}" | tr '[:lower:]' '[:upper:]')"
        rest="${first}${rest:1}"
        ;;
    esac
    [ -n "$scope" ] && rest="**${scope}**: ${rest}"
    out="${out}- ${rest}"$'\n'
  done <<SUBJECTS
$subjects
SUBJECTS

  printf '%s' "$out"
}

# Every group that has anything in it, as `### heading` plus its bullets.
changelog() {
  local i heading bullets
  for i in "${!group_headings[@]}"; do
    heading="${group_headings[$i]}"
    bullets="$(bullets_for "${group_types[$i]}")"
    [ -n "$bullets" ] || continue
    printf '### %s\n\n%s\n\n' "$heading" "$bullets"
  done

  bullets="$(bullets_for other)"
  if [ -n "$bullets" ]; then
    printf '### %s\n\n%s\n\n' "Other changes" "$bullets"
  fi
}

# The archives, in one sentence rather than a table: GitHub already lists every
# asset below the notes, with its size.
platforms() {
  cat <<TEXT
The Linux archives are static musl and run on any distribution, the macOS
archive is universal and is signed and notarized, and the Windows archives are
unsigned. Every archive holds the binary, \`README.md\`, \`LICENSE\` and shell
completions under \`completions/\`.
TEXT
}

verify() {
  cat <<TEXT
## Verify

\`\`\`sh
curl -fsSLO ${base}/SHA256SUMS
shasum -a 256 -c SHA256SUMS --ignore-missing
\`\`\`

On macOS, check the signature of the extracted binary:

\`\`\`sh
codesign -dv --verbose=4 chap-devops
\`\`\`
TEXT
}

notes_for_tag() {
  local body
  body="$(changelog)"
  [ -n "$body" ] || body="Nothing but the release commit itself."

  cat <<TEXT
## What's changed

${body}

Full changelog: ${compare}

## Install

\`\`\`sh
curl -fsSL ${script} | sh
\`\`\`

An installed \`chap-devops\` updates itself with \`chap-devops self update\`.

$(platforms)

$(verify)
TEXT
}

notes_for_dev() {
  local version built sha body heading
  version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
  built="$(date -u '+%Y-%m-%d %H:%M UTC')"
  sha="${GITHUB_SHA:-$(git rev-parse HEAD)}"

  body="$(changelog)"
  [ -n "$body" ] || body="Nothing yet: this is the tagged release, rebuilt."
  if [ -n "$previous" ]; then
    heading="Since ${previous}"
  else
    heading="What's changed"
  fi

  # `chap-devops self update` reads the commit back out of these notes, from the
  # "Built from commit" line below: the word `commit` followed by the sha. Keep
  # the two together if this text is ever reworded; a unit test in
  # `src/selfupdate/tests.rs` checks that they are.
  cat <<TEXT
## Unstable

This is the rolling build of \`main\`. It is rebuilt and replaced on every
push, and it is not a release: it reports the version of the last tag,
\`v${version}\`, with \`channel dev\`, and anything in it may change or break.
The numbered releases are the stable ones.

Built from commit \`${sha}\` on ${built}.

## ${heading}

${body}

Full changelog: ${compare}

## Install

\`\`\`sh
curl -fsSL ${script} | sh -s -- --version dev
\`\`\`

Or into the current directory alone, as \`./chap-devops\`, touching nothing else:

\`\`\`sh
curl -fsSL ${script} | sh -s -- --version dev --here
\`\`\`

A dev build follows this release from then on: \`chap-devops self update\` moves it
to the newest build of \`main\`, comparing commits rather than version
numbers, and \`chap-devops self version\` says \`channel dev\`. To go back to the
stable release:

\`\`\`sh
chap-devops self update --version v${version}
\`\`\`

$(platforms)

$(verify)
TEXT
}

if [ "$tag" = dev ]; then
  notes_for_dev
else
  notes_for_tag
fi
