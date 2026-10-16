#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Makes the small Stems test files in crates/media/tests/data (self-authored, see
docs/spec/stems-format.md).

Each file has five stereo audio tracks, sine tones at 110, 220, 440, 880 and 1760 Hz (the
master first), made with ffmpeg, and a `moov/udta/stem` box with stem metadata added here.
ffmpeg writes `moov` after `mdat`, so growing `moov` moves no sample data.

Usage: python scripts/stems-fixture.py  (needs ffmpeg on PATH)
"""

import base64
import json
import os
import struct
import subprocess
import sys

OUT = os.path.join(os.path.dirname(__file__), "..", "crates", "media", "tests", "data")

METADATA = {
    "version": 1,
    "mastering_dsp": {
        "compressor": {
            "enabled": True,
            "ratio": 4,
            "output_gain": 0.6,
            "release": 0.25,
            "attack": 0.005,
            "input_gain": 0.5,
            "threshold": -6,
            "hp_cutoff": 120,
            "dry_wet": 80,
        },
        "limiter": {"enabled": True, "release": 0.05, "threshold": -1, "ceiling": -0.3},
    },
    "stems": [
        {"name": "Drums", "color": "#FD4A4A"},
        {"name": "Bass", "color": "#FFA56D"},
        {"name": "Other", "color": "#00E8E8"},
        {"name": "Vox", "color": "#AD65FF"},
    ],
}


def boxes(data, start, end):
    at = start
    while at + 8 <= end:
        size, kind = struct.unpack(">I4s", data[at : at + 8])
        if size < 8:
            sys.exit("unexpected box size")
        yield at, size, kind
        at += size


def add_stem_box(path, payload):
    data = bytearray(open(path, "rb").read())
    stem = struct.pack(">I4s", 8 + len(payload), b"stem") + payload
    for at, size, kind in boxes(data, 0, len(data)):
        if kind != b"moov":
            continue
        if at + size != len(data):
            sys.exit("moov is not the last box; sample offsets would move")
        udta = next(((a, s) for a, s, k in boxes(data, at + 8, at + size) if k == b"udta"), None)
        if udta:
            ua, us = udta
            data[ua + us : ua + us] = stem
            data[ua : ua + 4] = struct.pack(">I", us + len(stem))
            added = len(stem)
        else:
            new = struct.pack(">I4s", 8 + len(stem), b"udta") + stem
            data[at + size : at + size] = new
            added = len(new)
        data[at : at + 4] = struct.pack(">I", size + added)
        open(path, "wb").write(data)
        return
    sys.exit("no moov box")


def make(name, codec, seconds, rate, payload):
    path = os.path.join(OUT, name)
    inputs = []
    for f in (110, 220, 440, 880, 1760):
        inputs += ["-f", "lavfi", "-i", f"sine=f={f}:d={seconds}:r={rate}"]
    maps = [a for i in range(5) for a in ("-map", str(i))]
    extra = ["-b:a", "64k"] if codec == "aac" else []
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", *inputs, *maps, "-ac", "2", "-ar", str(rate), "-c:a", codec, *extra,
         "-map_metadata", "-1", "-fflags", "+bitexact", path],
        check=True,
    )
    if payload is not None:
        add_stem_box(path, payload)
    print(name, os.path.getsize(path), "bytes")


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    text = json.dumps(METADATA).encode()
    make("tones-aac.stem.mp4", "aac", 1, 44100, text)
    make("tones-alac.stem.m4a", "alac", 0.25, 22050, base64.b64encode(text))
    make("tones-plain.stem.mp4", "aac", 0.5, 44100, None)
