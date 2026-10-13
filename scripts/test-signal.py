#!/usr/bin/env python3
"""Writes the test signal for the device corpus (docs/spec/als-corpus.md, series 6).

  python scripts/test-signal.py corpus/devices/test-signal.wav

A 48 kHz, 24-bit stereo WAV, 8 seconds, one second per section, so each device's effect shows
in a section and a bounce can be compared with Suhina's render second by second:

  0  100 Hz sine, both sides, -12 dBFS         lows: bass mono, EQ low band, filters
  1  1 kHz sine, both sides, -12 dBFS          mids: EQ mid band, distortion harmonics
  2  6 kHz sine, both sides, -12 dBFS          highs: EQ high band, tone
  3  white noise, both sides, -18 dBFS         every band at once
  4  440 Hz on the left only, -12 dBFS         balance, channel modes, phase, width
  5  300 Hz left, 500 Hz right, -12 dBFS       stereo: width, mid/side, swap
  6  200 Hz sine + DC offset 0.1, both sides   DC filter
  7  sine sweep 20 Hz to 20 kHz, -12 dBFS      the whole response at once

Standard library only. The noise uses a fixed seed, so every run writes the same file.
"""

from __future__ import annotations

import math
import random
import struct
import sys
import wave
from pathlib import Path

RATE = 48_000
SECONDS = 8


def db(value: float) -> float:
    return 10 ** (value / 20)


def frame(n: int, noise: random.Random) -> tuple[float, float]:
    section, t = divmod(n, RATE)
    time = t / RATE
    sine = lambda hz, level=-12.0: db(level) * math.sin(2 * math.pi * hz * time)  # noqa: E731
    if section == 0:
        return sine(100), sine(100)
    if section == 1:
        return sine(1_000), sine(1_000)
    if section == 2:
        return sine(6_000), sine(6_000)
    if section == 3:
        v = db(-18) * noise.uniform(-1, 1) * math.sqrt(3)
        return v, v
    if section == 4:
        return sine(440), 0.0
    if section == 5:
        return sine(300), sine(500)
    if section == 6:
        v = sine(200) + 0.1
        return v, v
    # An exponential sweep: the phase is the integral of the frequency.
    low, high = 20.0, 20_000.0
    k = math.log(high / low)
    phase = 2 * math.pi * low * (math.exp(k * time) - 1) / k
    v = db(-12) * math.sin(phase)
    return v, v


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    out = Path(sys.argv[1])
    out.parent.mkdir(parents=True, exist_ok=True)
    noise = random.Random(20261011)
    scale = 2**23 - 1
    data = bytearray()
    for n in range(RATE * SECONDS):
        for v in frame(n, noise):
            sample = max(-scale, min(scale, round(v * scale)))
            data += struct.pack("<i", sample)[:3]
    with wave.open(str(out), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(3)
        w.setframerate(RATE)
        w.writeframes(bytes(data))
    print(f"wrote {out} ({SECONDS} s, {RATE} Hz, 24-bit stereo)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
