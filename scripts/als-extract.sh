#!/usr/bin/env sh
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Decompress Live project/preset files (.als .alc .adg .adv: gzip-compressed XML) to .xml next to them.
# Usage: scripts/als-extract.sh <file-or-directory>...
# Then diff two variants: git diff --no-index a.xml b.xml   (or: code --diff a.xml b.xml)
set -eu
[ "$#" -gt 0 ] || { echo "usage: $0 <file-or-directory>..." >&2; exit 2; }
for arg in "$@"; do
  find "$arg" -type f \( -name '*.als' -o -name '*.alc' -o -name '*.adg' -o -name '*.adv' \) -not -path '*/Backup/*' |
  while IFS= read -r f; do
    gzip -dc < "$f" > "$f.xml" && echo "$f.xml"
  done
done
