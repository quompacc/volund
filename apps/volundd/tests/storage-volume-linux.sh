#!/bin/sh
# Run only the full-volume regression on a private, bounded tmpfs.
# Usage: sudo env VOLUND_TEST_DATABASE_URL=... sh storage-volume-linux.sh BINARY USER
set -eu
if [ "${1:-}" != --private ]; then
    exec unshare --mount --propagation private sh "$0" --private "$@"
fi
shift
test "$(id -u)" = 0
test "$#" = 2
test -n "${VOLUND_TEST_DATABASE_URL:-}"
test "$(id -u "$2")" != 0
binary=$(realpath "$1")
test -x "$binary"
volume=$(mktemp -d /var/tmp/volund-storage-test.XXXXXX)
mounted=false
cleanup() {
    if [ "$mounted" = true ]; then umount -- "$volume"; fi
    rmdir -- "$volume"
}
trap cleanup EXIT
mount -t tmpfs -o size=4m,nr_inodes=128,mode=0700 tmpfs "$volume"
mounted=true
chown "$2" "$volume"
runuser -u "$2" -- env VOLUND_TEST_DATABASE_URL="$VOLUND_TEST_DATABASE_URL" \
    VOLUND_TEST_FULL_VOLUME="$volume" "$binary" --ignored --exact \
    full_volume_upload_fails_without_original_loss_and_can_retry --nocapture
