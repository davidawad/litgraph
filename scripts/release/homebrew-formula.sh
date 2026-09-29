#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Render Formula/litgraph.rb for the tap from a release's .sha256 files.
#
# Usage: scripts/release/homebrew-formula.sh <version> <dir-with-sha256-files>
set -euo pipefail

version="$1"
dir="$2"
base="https://github.com/davidawad/litgraph/releases/download/v$version"

sha() {
  local f="$dir/litgraph-v$version-$1.tar.gz.sha256"
  [ -s "$f" ] || { echo "missing $f" >&2; exit 1; }
  cut -d' ' -f1 "$f"
}

cat <<EOF
class Litgraph < Formula
  desc "Litigation procedure graph engine: solve, simulate, and explore legal procedure"
  homepage "https://github.com/davidawad/litgraph"
  version "$version"
  license "GPL-3.0-or-later"

  on_macos do
    on_arm do
      url "$base/litgraph-v$version-aarch64-apple-darwin.tar.gz"
      sha256 "$(sha aarch64-apple-darwin)"
    end
    on_intel do
      url "$base/litgraph-v$version-x86_64-apple-darwin.tar.gz"
      sha256 "$(sha x86_64-apple-darwin)"
    end
  end

  on_linux do
    on_intel do
      url "$base/litgraph-v$version-x86_64-unknown-linux-musl.tar.gz"
      sha256 "$(sha x86_64-unknown-linux-musl)"
    end
  end

  def install
    bin.install "litgraph"
    bin.install "litgraph-mcp"
    pkgshare.install "packs"
  end

  test do
    assert_match "\"ok\":true", shell_output("#{bin}/litgraph describe --compact")
  end
end
EOF
