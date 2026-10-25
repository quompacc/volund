#!/bin/sh
# Enforces the AGENTS.md limit of 600 physical lines for handwritten code files.
# Generated or vendored files opt out with an "@generated" marker in their
# first five lines. Run from the repository root.
set -eu

limit=600
violations=$(
  git ls-files -- '*.rs' '*.ts' '*.tsx' '*.mjs' '*.js' '*.css' '*.html' \
    '*.cpp' '*.cc' '*.h' '*.hpp' '*.hxx' '*.sql' '*.sh' '*.py' '*.cmake' '*CMakeLists.txt' |
  while IFS= read -r file; do
    [ -f "$file" ] || continue
    if head -n 5 "$file" | grep -q '@generated'; then continue; fi
    lines=$(wc -l < "$file" | tr -d ' ')
    if [ "$lines" -gt "$limit" ]; then echo "$file: $lines lines (limit $limit)"; fi
  done
)

if [ -n "$violations" ]; then
  echo "Handwritten code files must stay at or below $limit lines; split them by responsibility:" >&2
  echo "$violations" >&2
  exit 1
fi
echo "All handwritten code files are at or below $limit lines."
