#!/usr/bin/env bash
# Keep release aliases under the same ancestry gate as rc/*.
set -euo pipefail
mode=${1:?expected base or current}
base=${2:?expected PR base}
case "$base" in
  main) integration=false ;;
  rc/?*|release/?*|releases/?*) integration=true ;;
  *) echo "::error::Target main, rc/*, release/* or releases/*; retarget stacked PRs before merging."; exit 1 ;;
esac
case "$mode" in
  base) echo "Base '$base' is allowed." ;;
  current)
    if [ "$integration" = true ]; then
      # The workflow fetches origin/main. HEAD is the proposed merge result.
      if ! git merge-base --is-ancestor origin/main HEAD; then
        echo "::error::Merging this would leave '$base' behind main. Merge origin/main into this PR first."
        exit 1
      fi
      echo "Merging this keeps '$base' current with main."
    fi
    ;;
  *) echo "Unknown mode: $mode" >&2; exit 1 ;;
esac
