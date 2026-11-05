#!/bin/sh
# Behaviour tests for deploy/docker/volund-loop (POSIX sh, no container needed).
set -eu

here=$(cd "$(dirname "$0")" && pwd)
loop="$here/../volund-loop"
root=$(mktemp -d)
cleanup() {
    rm -rf -- "$root"
}
trap cleanup EXIT HUP INT TERM

fail() {
    echo "FAIL: $1" >&2
    exit 1
}

# Wait up to $2 tenths of a second for condition command $1.
wait_for() {
    i=0
    while [ "$i" -lt "$2" ]; do
        if eval "$1"; then
            return 0
        fi
        sleep 0.1
        i=$((i + 1))
    done
    return 1
}

# 1. Usage errors exit 64.
set +e
sh "$loop" >/dev/null 2>&1
test "$?" -eq 64 || fail "missing arguments must exit 64"
sh "$loop" 5 >/dev/null 2>&1
test "$?" -eq 64 || fail "missing command must exit 64"
sh "$loop" 1.5 true >/dev/null 2>&1
test "$?" -eq 64 || fail "non-integer interval must exit 64"
sh "$loop" -1 true >/dev/null 2>&1
test "$?" -eq 64 || fail "negative interval must exit 64"
set -e

# 2. The command repeats, and a failing run does not end the loop.
cat > "$root/count.sh" <<EOF
#!/bin/sh
echo run >> "$root/runs"
exit 3
EOF
sh "$loop" 0 sh "$root/count.sh" 2> "$root/stderr" &
pid=$!
wait_for '[ -f "$root/runs" ] && [ "$(wc -l < "$root/runs")" -ge 3 ]' 50 ||
    fail "command was not repeated"
kill -TERM "$pid"
wait "$pid" || fail "loop must exit 0 after SIGTERM"
grep -q "exited with status 3" "$root/stderr" || fail "failed run was not logged"

# 3. SIGTERM reaches the running command, which may finish cleanly.
cat > "$root/worker.sh" <<EOF
#!/bin/sh
trap 'echo requeued > "$root/term"; exit 0' TERM
echo started > "$root/started"
sleep 30 &
wait
EOF
sh "$loop" 60 sh "$root/worker.sh" &
pid=$!
wait_for '[ -f "$root/started" ]' 50 || fail "worker did not start"
kill -TERM "$pid"
wait_for '! kill -0 "$pid" 2>/dev/null' 50 || fail "loop did not stop within 5 s"
wait "$pid" || fail "loop must exit 0 after forwarding SIGTERM"
test -f "$root/term" || fail "SIGTERM was not forwarded to the command"

# 4. SIGTERM during the interval stops at once without another run.
rm -f "$root/runs"
sh "$loop" 60 sh -c "echo run >> '$root/runs'" &
pid=$!
wait_for '[ -f "$root/runs" ]' 50 || fail "first run missing"
kill -TERM "$pid"
wait_for '! kill -0 "$pid" 2>/dev/null' 50 || fail "interval sleep was not interrupted"
test "$(wc -l < "$root/runs")" -eq 1 || fail "an extra run started after SIGTERM"

echo "volund-loop: all tests passed"
