#!/bin/sh
set -eu

output_directory=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output)
      output_directory=$2
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

test -n "$output_directory"
sleep 2
mkdir -p "$output_directory"
printf '{"contractVersion":1,"status":"ready"}\n' > "$output_directory/result.json"
