#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Print the next semver implied by the Conventional Commits since the last
# `v*` tag, or nothing when no commit warrants a release.
#
#   breaking (`type!:` or a `BREAKING CHANGE:` footer) -> major (minor while 0.x)
#   feat                                                -> minor
#   fix, perf                                           -> patch
#   anything else (docs, ci, chore, refactor, test, ...) -> no release
#
# Usage: scripts/release/next-version.sh [<rev>]   (default HEAD)
set -euo pipefail

rev="${1:-HEAD}"
last_tag="$(git describe --tags --abbrev=0 --match 'v[0-9]*' "$rev" 2>/dev/null || true)"
current="${last_tag#v}"
current="${current:-0.0.0}"
range="${last_tag:+$last_tag..}$rev"

IFS=. read -r major minor patch <<<"$current"

bump=none
rank() { case "$1" in major) echo 3 ;; minor) echo 2 ;; patch) echo 1 ;; *) echo 0 ;; esac; }
raise() { if [ "$(rank "$1")" -gt "$(rank "$bump")" ]; then bump="$1"; fi; }

while IFS= read -r -d $'\x1e' commit; do
  commit="${commit#"${commit%%[![:space:]]*}"}" # git log separates records with a newline
  subject="${commit%%$'\n'*}"
  if [[ "$subject" =~ ^[a-z]+(\([^\)]*\))?!: ]] || grep -qE '^BREAKING[ -]CHANGE:' <<<"$commit"; then
    raise major
  elif [[ "$subject" =~ ^feat(\([^\)]*\))?: ]]; then
    raise minor
  elif [[ "$subject" =~ ^(fix|perf)(\([^\)]*\))?: ]]; then
    raise patch
  fi
done < <(git log --format='%B%x1e' "$range")

# Pre-1.0 (semver item 4): a breaking change bumps the minor version.
if [ "$bump" = major ] && [ "$major" = 0 ]; then bump=minor; fi

case "$bump" in
  major) echo "$((major + 1)).0.0" ;;
  minor) echo "$major.$((minor + 1)).0" ;;
  patch) echo "$major.$minor.$((patch + 1))" ;;
  none) ;;
esac
