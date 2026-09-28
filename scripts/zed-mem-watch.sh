#!/usr/bin/env bash
set -euo pipefail

UNIT="${ZED_HARNESS_UNIT:-zed-harness.scope}"
OUT="${1:-/tmp/zed-mem-watch}"

CGROUP=$(systemctl --user show -p ControlGroup --value "$UNIT" 2>/dev/null || true)
if [ -z "$CGROUP" ] || [ ! -d "/sys/fs/cgroup/$CGROUP" ]; then
    echo "scope unit $UNIT not running (start it with scripts/zed-harness.sh)"
    exit 1
fi
CGFULL="/sys/fs/cgroup/$CGROUP"

mkdir -p "$OUT/smaps" "$OUT/threads" "$OUT/captures"
RSS_LOG="$OUT/rss.log"

echo "# ts total_kib nproc [pid rss_kib comm ...]" > "$RSS_LOG"

kib2g() { awk -v k="$1" 'BEGIN{printf "%.1fG", k/1048576}'; }

thresholds=(2097152 4194304 6291456 8388608 10485760)
last_crossed=0
prev_total=0
empty_ticks=0

snapshot() {
    local why="$1" ts="$2" pid
    local dir="$OUT/captures/$ts"
    mkdir -p "$dir"
    {
        echo "cgroup memory.current: $(kib2g "$(awk '{print $2}' "$CGFULL/memory.current" 2>/dev/null || echo 0)")"
        echo "cgroup memory.peak:    $(kib2g "$(awk '{print $2}' "$CGFULL/memory.peak" 2>/dev/null || echo 0)")"
        echo "cgroup memory.events:"
        cat "$CGFULL/memory.events" 2>/dev/null
        echo "--- process tree (rss in KiB) ---"
        ps -eo pid,ppid,rss,vsz,comm,args --forest
    } > "$dir/context.txt" 2>&1
    for pid in $(cat "$CGFULL/cgroup.procs" 2>/dev/null); do
        [ -r "/proc/$pid/smaps" ] && cat "/proc/$pid/smaps" > "$OUT/smaps/$ts-$pid.smaps" 2>/dev/null
        for t in /proc/$pid/task/*; do
            tid=${t##*/}
            cat "$t/stat" > "$OUT/threads/$pid-$tid-$ts.stat" 2>/dev/null
        done
    done
    echo "evidence captured -> $dir"
}

echo "watching $CGROUP -> $OUT"
echo "evidence snapshots at 2/4/6/8/10G and on >1G/tick growth."
echo "after the repro, cgroup memory.peak + memory.events oom_kill prove the cap worked."

while true; do
    if [ ! -e "$CGFULL/cgroup.procs" ]; then
        echo "scope gone; final state:"
        echo "  memory.peak: $(kib2g "$(awk '{print $2}' "$CGFULL/memory.peak" 2>/dev/null || echo 0)")"
        echo "  memory.events:"; cat "$CGFULL/memory.events" 2>/dev/null || true
        break
    fi

    ts=$(date +%H:%M:%S.%N)
    line="$ts"
    total=0
    n=0
    for pid in $(cat "$CGFULL/cgroup.procs" 2>/dev/null); do
        rss=$(awk '/VmRSS/{print $2}' "/proc/$pid/status" 2>/dev/null || echo 0)
        comm=$(cat "/proc/$pid/comm" 2>/dev/null || echo "?")
        line="$line $pid $rss $comm"
        total=$((total + rss))
        n=$((n + 1))
    done
    echo "$line" >> "$RSS_LOG"

    if [ "$n" -eq 0 ]; then
        empty_ticks=$((empty_ticks + 1))
        if [ "$empty_ticks" -ge 15 ]; then
            echo "cgroup emptied (zed died); final state:"
            echo "  memory.peak: $(kib2g "$(awk '{print $2}' "$CGFULL/memory.peak" 2>/dev/null || echo 0)")"
            echo "  memory.events:"; cat "$CGFULL/memory.events" 2>/dev/null || true
            break
        fi
    else
        empty_ticks=0
    fi

    growth=$((total - prev_total))
    if [ "$n" -gt 0 ]; then
        take=0
        why=""
        for t in "${thresholds[@]}"; do
            if [ "$total" -ge "$t" ] && [ "$last_crossed" -lt "$t" ]; then
                take=1
                why="crossed $(kib2g "$t")"
                last_crossed=$t
                break
            fi
        done
        if [ "$take" -eq 0 ] && [ "$growth" -gt 1048576 ]; then
            take=1
            why="growth +$(kib2g "$growth") in one tick"
        fi
        if [ "$take" -eq 1 ]; then
            snapshot "$why" "$ts"
        fi
    fi

    prev_total=$total
    echo "[$ts] total=$(kib2g "$total") growth=+$(kib2g "$growth") procs=$n"
    sleep 0.2
done
