#!/usr/bin/env bash
set -euo pipefail

TAG="${1:?usage: connect-release-notes.sh TAG}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"

PREV="$(git describe --tags --abbrev=0 --match 'connect-v*' "$TAG^" 2>/dev/null || true)"
if [[ -n "$PREV" ]]; then
  RANGE="$PREV..$TAG"
else
  RANGE="$TAG"
fi

printf "## What's Changed\n\n"
mapfile -t subjects < <(
  git log --format='%s' "$RANGE" -- \
    connect \
    .github/workflows/connect-release.yml \
    .github/workflows/connect.yml \
    .github/scripts/connect-update-manifest.py
)

if ((${#subjects[@]} == 0)); then
  printf '* No Connect changes since %s\n' "$PREV"
else
  for subject in "${subjects[@]}"; do
    if [[ "$subject" =~ ^(.*)\ \(\#([0-9]+)\)$ ]]; then
      printf '* %s in https://github.com/%s/pull/%s\n' \
        "${BASH_REMATCH[1]}" "$GITHUB_REPOSITORY" "${BASH_REMATCH[2]}"
    else
      printf '* %s\n' "$subject"
    fi
  done
fi

if [[ -n "$PREV" ]]; then
  printf '\n**Full Changelog**: https://github.com/%s/compare/%s...%s\n' \
    "$GITHUB_REPOSITORY" "$PREV" "$TAG"
fi
