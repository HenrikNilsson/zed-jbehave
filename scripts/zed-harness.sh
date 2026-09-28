#!/usr/bin/env bash
set -euo pipefail

MEM_MAX="${ZED_HARNESS_MEM_MAX:-10G}"
MEM_HIGH="${ZED_HARNESS_MEM_HIGH:-8G}"
UNIT="${ZED_HARNESS_UNIT:-zed-harness.scope}"
ZED_EDITOR="${ZED_EDITOR_BIN:-/home/henrik.nilsson@fnox.it/.local/zed.app/libexec/zed-editor}"

systemctl --user reset-failed "$UNIT" 2>/dev/null || true

echo "Launching Zed under a hard memory cap (unit $UNIT)"
echo "  memory.max=$MEM_MAX  memory.high=$MEM_HIGH  swap=0"
echo "  binary: $ZED_EDITOR"
echo "  Any ballooning allocation will be OOM-killed inside this cgroup only;"
echo "  the rest of the machine is never touched."

systemd-run --user --scope --unit="$UNIT" \
    -p "MemoryMax=$MEM_MAX" -p "MemorySwapMax=0" -p "MemoryHigh=$MEM_HIGH" \
    -- "$ZED_EDITOR" "$@" &
RUN_PID=$!

sleep 4

CGROUP=$(systemctl --user show -p ControlGroup --value "$UNIT" 2>/dev/null || true)
if [ -n "$CGROUP" ] && [ -f "/sys/fs/cgroup/$CGROUP/cgroup.procs" ]; then
    NPROC=$(wc -l < "/sys/fs/cgroup/$CGROUP/cgroup.procs")
    if [ "$NPROC" -gt 0 ]; then
        MAIN=$(head -1 "/sys/fs/cgroup/$CGROUP/cgroup.procs")
        echo "harness live: $NPROC process(es) in cgroup, main pid $MAIN"
        echo "monitor with: scripts/zed-mem-watch.sh"
        wait "$RUN_PID"
        echo "harness scope exited (code $?)"
        exit 0
    fi
fi

echo "WARNING: no process landed in the harness scope."
echo "  The window was probably handed off to an already-running zed instance,"
echo "  which is NOT under the memory cap. Quit all zed instances first, then"
echo "  re-run this script."
wait "$RUN_PID" || true
exit 1
