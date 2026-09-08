#!/bin/sh
set -eu

root=$(mktemp -d)
cleanup() {
    rm -rf -- "$root"
}
trap cleanup EXIT HUP INT TERM

mkdir -p "$root/bin" "$root/source" "$root/backups"

cat > "$root/bin/findmnt" <<'EOF'
#!/bin/sh
set -eu
eval "target=\${$#}"
case "$target" in
    */source) printf 'source-device\n' ;;
    *) printf '%s\n' "${TEST_BACKUP_DEVICE:-backup-device}" ;;
esac
EOF
cat > "$root/bin/pg_dump" <<'EOF'
#!/bin/sh
set -eu
while [ "$#" -gt 0 ]; do
    if [ "$1" = "--file" ]; then
        printf 'valid custom dump' > "$2"
        exit 0
    fi
    shift
done
exit 9
EOF
cat > "$root/bin/pg_restore" <<'EOF'
#!/bin/sh
set -eu
test "${FAIL_RESTORE:-0}" != 1
test "$1" = "--list"
test -s "$2"
EOF
cat > "$root/bin/psql" <<'EOF'
#!/bin/sh
set -eu
printf '%s\n' "$*" >> "${TEST_PSQL_LOG:?}"
EOF
chmod 750 "$root/bin/findmnt" "$root/bin/pg_dump" "$root/bin/pg_restore" "$root/bin/psql"

PATH="$root/bin:$PATH" \
TEST_PSQL_LOG="$root/psql.log" \
VOLUND_BACKUP_DIR="$root/backups" \
VOLUND_BACKUP_SOURCE_PATH="$root/source" \
VOLUND_BACKUP_DATABASE=volund_test \
VOLUND_BACKUP_RETENTION_DAYS=0 \
VOLUND_VERSION=0.39.0 \
"${BACKUP_SCRIPT:?BACKUP_SCRIPT is required}" >"$root/output.log"

test "$(find "$root/backups" -name 'volund-*.dump' | wc -l)" -eq 1
test "$(find "$root/backups" -name 'volund-*.sha256' | wc -l)" -eq 1
test -z "$(find "$root/backups" -name '.volund-*' -print -quit)"
dump=$(find "$root/backups" -name 'volund-*.dump' -print -quit)
test "$(stat -c %a "$dump")" = 600
(cd "$root/backups" && sha256sum -c ./*.sha256)
grep -q "operational_components" "$root/psql.log"
grep -q "operational_log_events" "$root/psql.log"
grep -q '"event":"backup.completed"' "$root/output.log"
grep -q '"version":"0.39.0"' "$root/output.log"
if grep -q "$root" "$root/output.log"; then
    echo "backup structured log leaked a private path" >&2
    exit 1
fi

rm -f -- "$root/backups"/*
rm -f -- "$root/psql.log"
if PATH="$root/bin:$PATH" FAIL_RESTORE=1 \
    TEST_PSQL_LOG="$root/psql.log" \
    VOLUND_BACKUP_DIR="$root/backups" VOLUND_BACKUP_SOURCE_PATH="$root/source" \
    "$BACKUP_SCRIPT" >/dev/null 2>&1; then
    echo "invalid dump was accepted" >&2
    exit 1
fi
test -z "$(find "$root/backups" -type f -print -quit)"
grep -q "last_outcome = 'failed'" "$root/psql.log"

rm -f -- "$root/psql.log"
if PATH="$root/bin:$PATH" TEST_BACKUP_DEVICE=source-device \
    TEST_PSQL_LOG="$root/psql.log" \
    VOLUND_BACKUP_DIR="$root/backups" VOLUND_BACKUP_SOURCE_PATH="$root/source" \
    "$BACKUP_SCRIPT" >/dev/null 2>&1; then
    echo "same-device backup was accepted" >&2
    exit 1
fi
grep -q "last_outcome = 'failed'" "$root/psql.log"

repository_root=$(CDPATH= cd -- "$(dirname "$0")/../../.." && pwd)
workspace_version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$repository_root/Cargo.toml" | head -n 1)
unit_version=$(sed -n 's/^Environment=VOLUND_VERSION=//p' "$repository_root/deploy/systemd/volund-backup.service.example")
test "$workspace_version" = "$unit_version"

echo "backup tests passed"
