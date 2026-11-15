#!/usr/bin/env bash
# Run inside the existing offline build container. No upload or channel change.
# Usage: release/build-flavor.sh classic|forever linux|windows
set -euo pipefail
cd "$(dirname "$0")/.."
flavor=${1:?choose classic or forever}
platform=${2:?choose linux or windows}
case "$flavor" in classic|forever) ;; *) echo "invalid flavor" >&2; exit 1;; esac
case "$platform" in linux|windows) ;; *) echo "invalid platform" >&2; exit 1;; esac
test -z "$(git status --porcelain)" || { echo "commit the build source first" >&2; exit 1; }
build_commit=$(git rev-parse HEAD)
build_tree=$(git rev-parse HEAD^{tree})
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$PWD/target/flavor-build}
args=(--profile dist --locked --offline -p vana-client --no-default-features --features "$flavor")
asset_dir="target/assets/$flavor"
mkdir -p "$asset_dir"
if [ "$platform" = windows ]; then
    mkdir -p target/winstatic
    cp "$(x86_64-w64-mingw32-gcc -print-file-name=libwinpthread.a)" target/winstatic/libpthread.a
    export RUSTFLAGS="-C target-cpu=x86-64-v2 -C target-feature=+crt-static -C link-arg=-static -L native=$PWD/target/winstatic"
    cargo build "${args[@]}" --target x86_64-pc-windows-gnu
    binary="$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/dist/vana-client.exe"
    if x86_64-w64-mingw32-objdump -p "$binary" | grep -qi 'DLL Name: libwinpthread'; then
        echo "unexpected dynamic winpthread dependency" >&2; exit 1
    fi
    asset="$asset_dir/vana-client-windows-x86_64.exe"
else
    cargo build "${args[@]}"
    binary="$CARGO_TARGET_DIR/dist/vana-client"
    asset="$asset_dir/vana-client-linux-x86_64"
fi
test "$(git rev-parse HEAD)" = "$build_commit" && test -z "$(git status --porcelain)" || {
    echo "source changed during build; no artifact recorded" >&2; exit 1;
}
install -m755 "$binary" "$asset"
python3 - "$asset" "$flavor" "$platform" "$build_commit" "$build_tree" <<'PY'
import hashlib,json,pathlib,sys
asset,flavor,platform,commit,tree=sys.argv[1:]
p=pathlib.Path(asset)
p.with_suffix(p.suffix+'.build.json').write_text(json.dumps(dict(flavor=flavor,platform=platform,build_commit=commit,source_tree=tree,binary_sha256=hashlib.sha256(p.read_bytes()).hexdigest()),indent=2)+'\n')
PY
echo "$asset"
