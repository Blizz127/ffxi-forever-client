#!/usr/bin/env bash
# Offline synthetic tests only. Usage: run.sh BINARY SHA256 OWN_DAT_ROOT EMPTY_EVIDENCE_DIR
set -euo pipefail
test "$#" -eq 4
binary=$(realpath "$1")
expected_sha=$2
dat=$(realpath "$3")
evidence=$(realpath "$4")
scripts=$(cd -- "$(dirname -- "$0")" && pwd)
test -f "$binary"
test -d "$dat/FINAL FANTASY XI"
test -d "$evidence"
test -z "$(ls -A "$evidence")"
docker run --rm --name vana-synthetic-launcher --network none --read-only --cpuset-cpus 0-11 \
  --user 1000:1000 --cap-drop ALL --security-opt no-new-privileges \
  --tmpfs /tmp --tmpfs /run/user/1000:mode=700,uid=1000,gid=1000 \
  -v /usr/lib/x86_64-linux-gnu:/usr/lib/x86_64-linux-gnu:ro \
  -v [redacted-path] \
  -v /usr/share/vulkan:/usr/share/vulkan:ro \
  -v /usr/bin/Xvfb:/usr/bin/Xvfb:ro -v /usr/bin/xkbcomp:/usr/bin/xkbcomp:ro \
  -v /usr/share/X11/xkb:/usr/share/X11/xkb:ro \
  -v "$dat:/game:ro" -v "$binary:/app/vana-client:ro" \
  -v "$scripts:/tests:ro" -v "$evidence:/evidence" \
  -e SYNTHETIC_NETWORK_NONE=1 -e EXPECTED_BINARY_SHA256="$expected_sha" \
  -e DISPLAY=:88 -e XDG_RUNTIME_DIR=/run/user/1000 \
  -e VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json \
  -e FFXI_MAIN_DLL='/game/FINAL FANTASY XI/FFXiMain.dll' \
  vana-sandbox:build python3 /tests/synthetic.py
