#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Show what changed between two extracted Live Set XML files, as element paths.

Usage: python scripts/als-diff.py a.als.xml b.als.xml

Each line is one changed, added or removed element or attribute, with its path from the root,
for example `LiveSet/MasterTrack/.../Tempo/Manual@Value: 120 -> 128`. Elements are matched by
tag and position among siblings with the same tag, or by their `Id` attribute when they have one.
"""

import sys
import xml.etree.ElementTree as ET


def key(elem, index):
    ident = elem.get("Id")
    return f"{elem.tag}[Id={ident}]" if ident is not None else f"{elem.tag}[{index}]"


def children(elem):
    counts = {}
    result = {}
    for child in elem:
        n = counts.get(child.tag, 0)
        counts[child.tag] = n + 1
        result[key(child, n)] = child
    return result


def label(path):
    # Drop "[0]" for readability; keep other indices and ids.
    return "/".join(p.replace("[0]", "") for p in path)


def summary(elem):
    attrs = " ".join(f'{k}="{v}"' for k, v in elem.attrib.items())
    return f"<{elem.tag}{' ' + attrs if attrs else ''}> ({sum(1 for _ in elem.iter()) - 1} descendants)"


def diff(a, b, path, out):
    for name in sorted(set(a.attrib) | set(b.attrib)):
        va, vb = a.get(name), b.get(name)
        if va != vb:
            out.append(f"{label(path)}@{name}: {va} -> {vb}")
    if (a.text or "").strip() != (b.text or "").strip():
        out.append(f"{label(path)} text: {(a.text or '').strip()!r} -> {(b.text or '').strip()!r}")
    ca, cb = children(a), children(b)
    for k in ca:
        if k not in cb:
            out.append(f"- {label(path + [k])} {summary(ca[k])}")
    for k in cb:
        if k not in ca:
            out.append(f"+ {label(path + [k])} {summary(cb[k])}")
    for k in ca:
        if k in cb:
            diff(ca[k], cb[k], path + [k], out)


def main():
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    a = ET.parse(sys.argv[1]).getroot()
    b = ET.parse(sys.argv[2]).getroot()
    out = []
    diff(a, b, [a.tag], out)
    print("\n".join(out))


if __name__ == "__main__":
    main()
