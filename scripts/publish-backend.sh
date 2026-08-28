#!/usr/bin/env bash
set -euo pipefail

source_root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
upload=0

usage() {
  cat <<'EOF'
Usage: scripts/publish-backend.sh [--upload]

Build a stripped omamusic release binary and SHA256SUMS. Writes to a
temp dir (never the plugin tree). Pass --upload to create or update the
GitHub Release pinned in scripts/backend-release.
EOF
}

while (( $# > 0 )); do
  case $1 in
    --upload) upload=1 ;;
    -h|--help) usage; exit 0 ;;
    *)
      echo "publish-backend.sh: unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

# shellcheck disable=SC1091
source "$source_root/scripts/backend-release"
[[ -n ${RELEASE_REPO:-} && -n ${RELEASE_VERSION:-} ]]

command -v cargo >/dev/null 2>&1 || {
  echo "publish-backend.sh: cargo is required" >&2
  exit 1
}
command -v sha256sum >/dev/null 2>&1 || {
  echo "publish-backend.sh: sha256sum is required" >&2
  exit 1
}

case $(uname -m) in
  x86_64|amd64) asset=omamusic-x86_64-linux ;;
  aarch64|arm64) asset=omamusic-aarch64-linux ;;
  *)
    echo "publish-backend.sh: unsupported architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

cache_root=${XDG_CACHE_HOME:-"$HOME/.cache"}
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$cache_root/omamusic/target}"
install -d -m 700 -- "$CARGO_TARGET_DIR"

cargo build --release --locked --manifest-path "$source_root/Cargo.toml"
strip "$CARGO_TARGET_DIR/release/omamusic"

out=$(mktemp -d "${TMPDIR:-/tmp}/omamusic-release.XXXXXX")
install -m 755 -- "$CARGO_TARGET_DIR/release/omamusic" "$out/$asset"
(cd "$out" && sha256sum -- "$asset" > SHA256SUMS)

echo "Built $out/$asset"
cat "$out/SHA256SUMS"

if (( upload == 0 )); then
  echo "Re-run with --upload to publish GitHub release v${RELEASE_VERSION}."
  echo "Keep $out until then, or rebuild."
  printf '%s\n' "$out"
  exit 0
fi

command -v gh >/dev/null 2>&1 || {
  echo "publish-backend.sh: gh is required for --upload" >&2
  exit 1
}

tag="v${RELEASE_VERSION}"
target=$(git -C "$source_root" rev-parse HEAD)
if gh release view "$tag" --repo "$RELEASE_REPO" >/dev/null 2>&1; then
  gh release upload "$tag" "$out/$asset" "$out/SHA256SUMS" \
    --repo "$RELEASE_REPO" --clobber
else
  gh release create "$tag" "$out/$asset" "$out/SHA256SUMS" \
    --repo "$RELEASE_REPO" \
    --target "$target" \
    --title "omamusic ${RELEASE_VERSION}" \
    --notes "Prebuilt OMA Music playback daemon. scripts/setup.sh downloads this asset; cargo is not required."
fi
echo "Published $tag ($asset)"
rm -rf -- "$out"
