---
description: DX7 voice data as system-exclusive messages (one voice, a bank of 32), the voice layout and what the FM synth does with bad data
read_when: DX7, sysex, syx, voice, bank, cartridge, FM synth patch, patch format, fm-synth
type: spec
status: current
last_verified: 2026-10-12
covers:
  - devices/fm-synth/src/patch.rs
---

# DX7 voice data

The FM synth (`devices/fm-synth`) stores its voice in the layout of a DX7 single-voice message
and reads and writes DX7 voice files (`.syx`), so the large body of voices made for that
instrument loads directly. Everything here comes from Yamaha's published MIDI data format for
the DX7 and from files we write ourselves; no voice from a product's library is committed.

## Messages

Both messages are system exclusive: `F0 43 0n <format> <length high> <length low> <data>
<checksum> F7`, where `n` is the MIDI channel (0 is channel 1) and the length is 7 bits per byte.

| Message | Format | Length | Data | Whole message |
|---|---|---|---|---|
| One voice | 0 | `01 1B` (155) | 155 bytes, the voice layout below | 163 bytes |
| Bank of 32 voices | 9 | `20 00` (4096) | 32 × 128 packed bytes | 4104 bytes |

The checksum is the two's complement of the sum of the data bytes, low 7 bits. A bank is also
accepted as its bare 4096 bytes of packed voices, as some files hold it.

## The voice layout (155 bytes)

Six operator groups of 21 bytes, **operator 6 first**, then the global bytes:

| Byte in group | Meaning | Range |
|---|---|---|
| 0–3 | envelope rates 1–4 | 0–99 |
| 4–7 | envelope levels 1–4 | 0–99 |
| 8 | keyboard level scaling break point (39 is C3) | 0–99 |
| 9, 10 | left and right scaling depth | 0–99 |
| 11, 12 | left and right curve: -LIN, -EXP, +EXP, +LIN | 0–3 |
| 13 | keyboard rate scaling | 0–7 |
| 14 | amplitude modulation sensitivity | 0–3 |
| 15 | key velocity sensitivity | 0–7 |
| 16 | output level | 0–99 |
| 17 | oscillator mode: ratio, fixed | 0–1 |
| 18 | frequency coarse | 0–31 |
| 19 | frequency fine | 0–99 |
| 20 | detune, 7 is none | 0–14 |

| Byte | Meaning | Range |
|---|---|---|
| 126–129 | pitch envelope rates 1–4 | 0–99 |
| 130–133 | pitch envelope levels 1–4 (50 is no change) | 0–99 |
| 134 | algorithm, 0 is algorithm 1 | 0–31 |
| 135 | feedback | 0–7 |
| 136 | oscillator key sync | 0–1 |
| 137 | LFO speed | 0–99 |
| 138 | LFO delay | 0–99 |
| 139 | LFO pitch modulation depth | 0–99 |
| 140 | LFO amplitude modulation depth | 0–99 |
| 141 | LFO key sync | 0–1 |
| 142 | LFO wave: triangle, saw down, saw up, square, sine, sample and hold | 0–5 |
| 143 | pitch modulation sensitivity | 0–7 |
| 144 | transpose, 24 is none (C3) | 0–48 |
| 145–154 | name, 10 ASCII characters | 32–127 |

The synth keeps one more byte after these, not part of any message: bit `n` set means operator
`6 - n` plays (the DX7's operator on/off switches, which it does not store in a voice).

## A packed voice (128 bytes, in a bank)

Per operator, 17 bytes, operator 6 first:

| Byte | Holds |
|---|---|
| 0–10 | bytes 0–10 of the group, unchanged |
| 11 | right curve × 4 + left curve |
| 12 | detune × 8 + rate scaling |
| 13 | velocity sensitivity × 4 + amplitude modulation sensitivity |
| 14 | output level |
| 15 | coarse × 2 + mode |
| 16 | fine |

Then: 102–109 the pitch envelope, 110 the algorithm, 111 key sync × 8 + feedback, 112–115 LFO
speed, delay, pitch depth and amplitude depth, 116 pitch modulation sensitivity × 16 + wave × 2
+ LFO key sync, 117 transpose, 118–127 the name. The top bit of every byte is ignored.

## Bad data

Files in circulation are often damaged or written by other programs. The reader:

- refuses data that is not one of the shapes above (wrong length, header or end byte);
- **reports** a wrong checksum but still reads the voices, because many good banks carry one;
- limits every value to its range, so any byte string of the right length becomes voices that
  play (tested with random bytes).

Writing produces messages on channel 1 with a correct checksum; a bank written from fewer than
32 voices is filled with the initial voice.
