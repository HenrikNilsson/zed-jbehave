#!/usr/bin/env bash
set -euo pipefail

MEM_MAX="${ZED_REPRO_MEM_MAX:-10G}"
MEM_HIGH="${ZED_REPRO_MEM_HIGH:-8G}"
UNIT="${ZED_REPRO_UNIT:-zed-repro.scope}"
DATA_DIR="${ZED_REPRO_DATA_DIR:-/tmp/zed-repro/share/zed}"
ZED_EDITOR="${ZED_EDITOR_BIN:-/home/henrik.nilsson@fnox.it/.local/zed.app/libexec/zed-editor}"

if [ ! -d "$DATA_DIR/extensions" ]; then
    echo "ERROR: repro data dir not found: $DATA_DIR" >&2
    echo "  expected a zed data dir with an 'extensions' subdir." >&2
    exit 2
fi

# Force an extension-index rebuild on next launch, so a changed
# installed/ dir (e.g. the jbehave symlink) is picked up at startup.
touch "$DATA_DIR/extensions/installed"

systemctl --user reset-failed "$UNIT" 2>/dev/null || true

echo "Launching Zed repro under a hard memory cap (unit $UNIT)"
echo "  memory.max=$MEM_MAX  memory.high=$MEM_HIGH  swap=0"
echo "  binary: $ZED_EDITOR"
echo "  data dir: $DATA_DIR (isolated from the real ~/.local/share/zed)"

systemd-run --user --scope --unit="$UNIT" \
    -p "MemoryMax=$MEM_MAX" -p "MemorySwapMax=0" -p "MemoryHigh=$MEM_HIGH" \
    -- "$ZED_EDITOR" --user-data-dir "$DATA_DIR" "$@" &
RUN_PID=$!

sleep 5

CGROUP=$(systemctl --user show -p ControlGroup --value "$UNIT" 2>/dev/null || true)
if [ -n "$CGROUP" ] && [ -f "/sys/fs/cgroup/$CGROUP/cgroup.procs" ]; then
    NPROC=$(wc -l < "/sys/fs/cgroup/$CGROUP/cgroup.procs")
    if [ "$NPROC" -gt 0 ]; then
        MAIN=$(head -1 "/sys/fs/cgroup/$CGROUP/cgroup.procs")
        echo "repro live: $NPROC process(es) in cgroup, main pid $MAIN"
        echo "monitor with: scripts/zed-mem-watch.sh"
        wait "$RUN_PID"
        RC=$?
        echo "repro scope exited (code $RC)"
        if [ "$RC" -eq 137 ]; then
            echo "  (137 = OOM-killed inside the scope)"
        fi
        exit 0
    fi
fi

echo "WARNING: no process landed in the repro scope." >&2
wait "$RUN_PID" || true
exit 1
