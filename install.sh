#!/bin/sh
# Install chap-devops, which deploys Chap and its services with Docker Compose.
#
#   curl -fsSL https://raw.githubusercontent.com/dhis2-chap/chap-devops/main/install.sh | sh
#
# Detects the platform, downloads the release archive for it, checks it
# against the release's SHA256SUMS, and installs the binary. POSIX sh on
# purpose: this has to run on a minimal container, a BSD and a Mac without
# assuming bash is there.
#
# Options (also readable as environment variables):
#
#   --version TAG        CHAP_DEVOPS_VERSION         release to install, e.g. v0.2.0
#   --dir DIR            CHAP_DEVOPS_INSTALL_DIR     where to put the binary
#   --here               CHAP_DEVOPS_INSTALL_DIR=.   just ./chap-devops, nothing else
#   --dry-run                                  print what would happen
#   --help
#
# --version dev installs the rolling pre-release built from every push to
# main. It carries the version of the last tag and is not a stable release;
# --version vX.Y.Z or no --version at all is the stable one.
#
# --here downloads, verifies and unpacks the binary into the current directory
# as ./chap-devops and stops there: no PATH advice, no completions, nothing outside
# this directory. It is the shape a CI job or a one-off trial wants.
#
# CHAP_DEVOPS_DOWNLOAD_BASE overrides where the archive and SHA256SUMS are fetched
# from. It exists to test this script against a locally built release and is
# not something an install needs.

set -eu

REPO="dhis2-chap/chap-devops"
RELEASES_URL="https://github.com/${REPO}/releases"
API_LATEST="https://api.github.com/repos/${REPO}/releases/latest"

VERSION="${CHAP_DEVOPS_VERSION:-}"
INSTALL_DIR="${CHAP_DEVOPS_INSTALL_DIR:-}"
DOWNLOAD_BASE="${CHAP_DEVOPS_DOWNLOAD_BASE:-}"
DRY_RUN=0
HERE=0

# The tag of the rolling pre-release built from main.
DEV_TAG="dev"

say() {
  printf '%s\n' "$*"
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'USAGE'
install chap-devops, which deploys Chap and its services with Docker Compose

Usage: install.sh [OPTIONS]

Options:
      --version TAG   Release to install (default: the newest stable one).
                      `dev` is the rolling pre-release built from every push
                      to main: same version number as the last tag, but not a
                      stable release.
      --dir DIR       Directory to install into (default: /usr/local/bin if
                      writable, otherwise ~/.local/bin)
      --here          Put the binary in the current directory as ./chap-devops and
                      do nothing else: no PATH advice, no completions
      --dry-run       Print what would be done and change nothing
  -h, --help          Print this message

Environment:
  CHAP_DEVOPS_VERSION       Same as --version
  CHAP_DEVOPS_INSTALL_DIR   Same as --dir; `.` is the same as --here

Examples:
  install.sh                      the newest stable release
  install.sh --version dev        the rolling build of main
  install.sh --here               ./chap-devops, and nothing else
USAGE
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version)
      [ $# -ge 2 ] || die "--version needs a tag, e.g. --version v0.2.0"
      VERSION="$2"
      shift 2
      ;;
    --version=*)
      VERSION="${1#--version=}"
      shift
      ;;
    --dir)
      [ $# -ge 2 ] || die "--dir needs a directory"
      INSTALL_DIR="$2"
      shift 2
      ;;
    --dir=*)
      INSTALL_DIR="${1#--dir=}"
      shift
      ;;
    --here)
      HERE=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1 (try --help)"
      ;;
  esac
done

# `--dir .` and CHAP_DEVOPS_INSTALL_DIR=. ask for exactly what --here does, so they
# are the same thing rather than a directory that happens to be this one: the
# difference people mean by "just put it here" is the completions and the PATH
# advice that --here leaves out. Any other directory alongside --here is two
# different answers to the same question, and picking one quietly would be
# worse than saying so.
case "$INSTALL_DIR" in
  . | ./) HERE=1 ;;
  ?*)
    [ "$HERE" -eq 0 ] ||
      die "--here installs into the current directory; pass one of --here and --dir (CHAP_DEVOPS_INSTALL_DIR), not both"
    ;;
esac
if [ "$HERE" -eq 1 ]; then
  INSTALL_DIR="$(pwd)"
fi

# ---------------------------------------------------------------------------
# Tools
# ---------------------------------------------------------------------------

have() {
  command -v "$1" >/dev/null 2>&1
}

have curl || die "curl is required to download the release"
have tar || die "tar is required to unpack the release"
have install || die "install(1) is required to put the binary in place"

# Either of the two sha256 tools will do; without one the download cannot be
# checked, and installing an unverified binary is not something this script
# does quietly.
SHA_CMD=""
if have sha256sum; then
  SHA_CMD="sha256sum"
elif have shasum; then
  SHA_CMD="shasum -a 256"
else
  die "neither sha256sum nor shasum was found; install one, or download from ${RELEASES_URL} and check by hand"
fi

# ---------------------------------------------------------------------------
# Platform
# ---------------------------------------------------------------------------

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
  Linux)
    case "$arch" in
      x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
      aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
      *)
        die "no chap-devops build for Linux ${arch}; see ${RELEASES_URL}"
        ;;
    esac
    ;;
  Darwin)
    # One universal archive covers Apple silicon and Intel.
    target="universal-apple-darwin"
    ;;
  MINGW* | MSYS* | CYGWIN* | Windows_NT)
    die "this script does not install on Windows; run it inside WSL 2 (with Docker Desktop's WSL integration on), which installs the Linux binary"
    ;;
  *)
    die "no chap-devops build for ${os} ${arch}; see ${RELEASES_URL}"
    ;;
esac

# ---------------------------------------------------------------------------
# Which release
# ---------------------------------------------------------------------------

# The API answers with the release JSON; tag_name is the one field needed, and
# sed gets it without a jq dependency.
latest_from_api() {
  curl -fsSL -H 'Accept: application/vnd.github+json' "$API_LATEST" 2>/dev/null |
    sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' |
    head -n 1
}

# Unauthenticated API calls are rate limited to 60 an hour per address, and a
# CI runner burns through that. The /releases/latest page redirects to the
# release, so its Location header carries the same tag.
latest_from_redirect() {
  curl -fsSI "${RELEASES_URL}/latest" 2>/dev/null |
    tr -d '\r' |
    sed -n 's|^[Ll]ocation:.*/tag/\(.*\)$|\1|p' |
    tail -n 1
}

if [ -z "$VERSION" ]; then
  VERSION="$(latest_from_api || true)"
fi
if [ -z "$VERSION" ]; then
  VERSION="$(latest_from_redirect || true)"
fi
[ -n "$VERSION" ] || die "could not work out the newest release; pass --version, or see ${RELEASES_URL}"

# The archive is named after the target alone; the release it comes from is
# the ${VERSION} in the URL. The directory inside it does carry the version.
archive="chap-devops-${target}.tar.gz"
base="${DOWNLOAD_BASE:-${RELEASES_URL}/download/${VERSION}}"

# ---------------------------------------------------------------------------
# Where it goes
# ---------------------------------------------------------------------------

writable_dir() {
  [ -d "$1" ] && [ -w "$1" ]
}

if [ -z "$INSTALL_DIR" ]; then
  if writable_dir /usr/local/bin; then
    INSTALL_DIR="/usr/local/bin"
  else
    INSTALL_DIR="${HOME}/.local/bin"
  fi
fi

say "chap-devops ${VERSION}"
if [ "$VERSION" = "$DEV_TAG" ]; then
  say "  channel   dev: the rolling build of main, not a stable release"
fi
say "  platform  ${os} ${arch} (${target})"
say "  archive   ${base}/${archive}"
if [ "$HERE" -eq 1 ]; then
  say "  install   ${INSTALL_DIR}/chap-devops (the binary alone)"
else
  say "  install   ${INSTALL_DIR}/chap-devops, and ${INSTALL_DIR}/cdx as its short form"
fi

if [ "$DRY_RUN" -eq 1 ]; then
  say ""
  say "dry run: nothing was downloaded or written"
  exit 0
fi

# ---------------------------------------------------------------------------
# Download, verify, install
# ---------------------------------------------------------------------------

tmp="$(mktemp -d "${TMPDIR:-/tmp}/chap-devops-install.XXXXXX")"
cleanup() {
  rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

say ""
say "downloading ${archive}"
curl -fsSL "${base}/${archive}" -o "${tmp}/${archive}" ||
  die "could not download ${base}/${archive}; check the version and see ${RELEASES_URL}"

curl -fsSL "${base}/SHA256SUMS" -o "${tmp}/SHA256SUMS" ||
  die "could not download ${base}/SHA256SUMS; refusing to install something unverified"

# --ignore-missing would keep the check to the one archive that was
# downloaded rather than every file the release published, but not every
# sha256 tool has that flag, so the manifest is narrowed to one line here
# instead. The name goes through sed first because it is a file name being
# used as a regular expression, and it is full of dots.
pattern="$(printf '%s' "$archive" | sed 's/[][\\.*^$]/\\&/g')"
grep " [*]\{0,1\}\(\./\)\{0,1\}${pattern}\$" "${tmp}/SHA256SUMS" > "${tmp}/SHA256SUMS.one" ||
  die "SHA256SUMS does not list ${archive}"

say "verifying the checksum"
(cd "$tmp" && $SHA_CMD -c SHA256SUMS.one >/dev/null 2>&1) ||
  die "${archive} does not match its checksum; delete it and try again"

say "unpacking"
tar -xzf "${tmp}/${archive}" -C "$tmp" || die "could not unpack ${archive}"

# The directory inside the archive carries the version: the tag for a release
# build, dev-<short sha> for a rolling one. It is matched rather than spelled
# out, so `--version dev` unpacks like any other release.
binary=""
for candidate in "${tmp}"/chap-devops-*-"${target}"/chap-devops; do
  if [ -f "$candidate" ]; then
    binary="$candidate"
    break
  fi
done
[ -n "$binary" ] || die "${archive} does not contain chap-devops"

mkdir -p "$INSTALL_DIR" || die "could not create ${INSTALL_DIR}"
[ -w "$INSTALL_DIR" ] || die "${INSTALL_DIR} is not writable; re-run with sudo, or pass --dir DIR"

install -m 0755 "$binary" "${INSTALL_DIR}/chap-devops" ||
  die "could not install into ${INSTALL_DIR}"

# cdx is the short form of chap-devops: a link, so `chap-devops self update` updates both.
# Not with --here, which installs the binary alone. A cdx that is something else,
# such as the genome tool that Debian and Ubuntu ship, is left alone.
short_added=0
if [ "$HERE" -eq 0 ]; then
  short="${INSTALL_DIR}/cdx"
  if [ -e "$short" ] && [ "$(readlink "$short" 2>/dev/null)" != "chap-devops" ]; then
    say "  note: ${short} is another program, so it was left alone; use chap-devops"
  elif ln -sf chap-devops "$short"; then
    short_added=1
  else
    say "  note: could not add the short form ${short}; chap-devops works without it"
  fi
fi

# --here is the whole job: the binary, verified, in this directory. Anything
# further - completion scripts in a shell's directory, advice about PATH -
# would be a change outside it, which is exactly what was not asked for.
if [ "$HERE" -eq 1 ]; then
  say ""
  say "installed ${INSTALL_DIR}/chap-devops"
  "${INSTALL_DIR}/chap-devops" --version || true
  exit 0
fi

# ---------------------------------------------------------------------------
# Shell completions, best effort
# ---------------------------------------------------------------------------

# Only into directories that already exist: creating a completion directory a
# shell has not been told about would do nothing, and a failure here must
# never fail an install that worked.
install_completion() {
  src="$1"
  dir="$2"
  name="$3"
  [ -f "$src" ] || return 0
  [ -d "$dir" ] || return 0
  [ -w "$dir" ] || return 0
  if install -m 0644 "$src" "${dir}/${name}" 2>/dev/null; then
    say "  completions  ${dir}/${name}"
  fi
  return 0
}

completions="${binary%/chap-devops}/completions"
if [ -d "$completions" ]; then
  install_completion "${completions}/chap-devops.bash" \
    "${XDG_DATA_HOME:-${HOME}/.local/share}/bash-completion/completions" "chap-devops"
  install_completion "${completions}/_chap-devops" "${HOME}/.zsh/completions" "_chap-devops"
  install_completion "${completions}/chap-devops.fish" \
    "${XDG_CONFIG_HOME:-${HOME}/.config}/fish/completions" "chap-devops.fish"
  # bash and fish load a completion file by the name of the command, so cdx
  # gets the same script under its own name. zsh reads `#compdef chap-devops cdx`.
  if [ "$short_added" -eq 1 ]; then
    install_completion "${completions}/chap-devops.bash" \
      "${XDG_DATA_HOME:-${HOME}/.local/share}/bash-completion/completions" "cdx"
    install_completion "${completions}/chap-devops.fish" \
      "${XDG_CONFIG_HOME:-${HOME}/.config}/fish/completions" "cdx.fish"
  fi
fi

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

say ""
say "installed ${INSTALL_DIR}/chap-devops"
"${INSTALL_DIR}/chap-devops" --version || true

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    say ""
    say "${INSTALL_DIR} is not on your PATH. Add this to your shell profile:"
    say ""
    say "    export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac

say ""
say "next: chap-devops init mychap && cd mychap && chap-devops up"
