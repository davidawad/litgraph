#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Stamp a release: set the workspace version, refresh Cargo.lock, and turn
# CHANGELOG.md's `## [Unreleased]` section into `## [<version>] - <date>`
# (leaving a fresh, empty Unreleased section above it).
#
# Usage: scripts/release/prepare.sh <version>
set -euo pipefail

version="$1"
today="$(date -u +%Y-%m-%d)"

# The workspace version is the first `version = "..."` in [workspace.package].
python3 - "$version" <<'EOF'
import re, sys
v = sys.argv[1]
s = open("Cargo.toml").read()
start = s.index("[workspace.package]")
m = re.compile(r'^version = "[^"]*"$', re.M).search(s, start)
if not m:
    sys.exit("no version in [workspace.package]")
open("Cargo.toml", "w").write(s[:m.start()] + f'version = "{v}"' + s[m.end():])
EOF
cargo update --workspace --quiet

python3 - "$version" "$today" <<'EOF'
import sys
v, today = sys.argv[1], sys.argv[2]
s = open("CHANGELOG.md").read()
marker = "## [Unreleased]\n"
if marker not in s:
    sys.exit("CHANGELOG.md has no '## [Unreleased]' section")
s = s.replace(marker, f"{marker}\n## [{v}] - {today}\n", 1)
open("CHANGELOG.md", "w").write(s)
EOF
