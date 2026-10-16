// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A VST2 plug-in's saved state as the standard VST2 bank (an `.fxb` file's contents), the form
//! the model keeps and the form a VST3 version of the plug-in reads when it stands in for the
//! VST2 one (ADR 0006, decision 8).
//!
//! A Live Set keeps the state in `VstPreset/Buffer` and says in `VstPreset/Type` which of two
//! kinds it is (`docs/spec/als-format.md`):
//!
//! - `FBCh`: the plug-in's own chunk, as the plug-in gave it.
//! - `FxBk`: the programs, each a 28-byte name and then every parameter as a 32-bit float, in
//!   the byte order `ByteOrder` names.
//!
//! The bank is big-endian: `CcnK`, the size of the rest, `FBCh` or `FxBk`, the bank version (2),
//! the plug-in's id and version, the number of programs, the current program, 124 zero bytes;
//! then the chunk (its size, its bytes) or each program (`CcnK`, the size of the rest, `FxCk`,
//! version 1, id, plug-in version, number of parameters, the 28-byte name, the parameters).

/// `FBCh`: a bank that holds the plug-in's own chunk.
pub(crate) const CHUNK_BANK: u32 = u32::from_be_bytes(*b"FBCh");
/// `FxBk`: a bank of programs, each a list of parameter values.
pub(crate) const PROGRAM_BANK: u32 = u32::from_be_bytes(*b"FxBk");
/// Length of a program's name.
const NAME: usize = 28;
/// Most programs or parameters a bank may say it has.
const MAX_COUNT: usize = 1 << 16;

/// What a Live Set says about a VST2 state besides its bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Preset {
    /// [`CHUNK_BANK`] or [`PROGRAM_BANK`].
    pub kind: u32,
    pub id: u32,
    pub version: i32,
    pub programs: usize,
    pub current: i32,
    pub parameters: usize,
    /// Whether parameter values are stored little-endian.
    pub little_endian: bool,
}

fn push(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

/// Writes `CcnK` and a size to fill in later; returns where the size goes.
fn open(out: &mut Vec<u8>) -> usize {
    out.extend_from_slice(b"CcnK");
    out.extend_from_slice(&[0; 4]);
    out.len()
}

/// Fills in the size written at `at`: the bytes after it.
fn close(out: &mut [u8], at: usize) -> Result<(), String> {
    let size = u32::try_from(out.len() - at).map_err(|_| "the state is too large")?;
    let slot = out.get_mut(at - 4..at).ok_or("bank layout")?;
    slot.copy_from_slice(&size.to_be_bytes());
    Ok(())
}

/// The standard VST2 bank for `buffer`, as `preset` describes it.
pub(crate) fn bank(preset: &Preset, buffer: &[u8]) -> Result<Vec<u8>, String> {
    if preset.programs > MAX_COUNT || preset.parameters > MAX_COUNT {
        return Err("the preset says it has too many programs or parameters".into());
    }
    let mut out = Vec::with_capacity(buffer.len() + 160 + preset.programs * 48);
    let bank = open(&mut out);
    push(&mut out, preset.kind);
    push(&mut out, 2);
    push(&mut out, preset.id);
    push(&mut out, preset.version as u32);
    match preset.kind {
        CHUNK_BANK => {
            push(&mut out, preset.programs.max(1) as u32);
            push(&mut out, preset.current as u32);
            out.extend_from_slice(&[0; 124]);
            push(&mut out, u32::try_from(buffer.len()).map_err(|_| "the state is too large")?);
            out.extend_from_slice(buffer);
        }
        PROGRAM_BANK => {
            let program = NAME + 4 * preset.parameters;
            if preset.programs == 0 || buffer.len() != preset.programs * program {
                return Err(format!(
                    "the state holds {} bytes, not {} programs of {} parameters",
                    buffer.len(),
                    preset.programs,
                    preset.parameters
                ));
            }
            push(&mut out, preset.programs as u32);
            push(&mut out, preset.current as u32);
            out.extend_from_slice(&[0; 124]);
            for stored in buffer.chunks_exact(program) {
                let (name, values) = stored.split_at(NAME);
                let at = open(&mut out);
                out.extend_from_slice(b"FxCk");
                push(&mut out, 1);
                push(&mut out, preset.id);
                push(&mut out, preset.version as u32);
                push(&mut out, preset.parameters as u32);
                out.extend_from_slice(name);
                for value in values.as_chunks::<4>().0 {
                    let bits =
                        if preset.little_endian { u32::from_le_bytes(*value) } else { u32::from_be_bytes(*value) };
                    push(&mut out, bits);
                }
                close(&mut out, at)?;
            }
        }
        other => return Err(format!("unknown kind of VST2 state {other}")),
    }
    close(&mut out, bank)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(bytes: &[u8], at: usize) -> u32 {
        u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn a_chunk_becomes_a_chunk_bank() {
        let preset = Preset {
            kind: CHUNK_BANK,
            id: 0x5465_7374,
            version: 3,
            programs: 0,
            current: 0,
            parameters: 9,
            little_endian: true,
        };
        let out = bank(&preset, &[1, 2, 3]).unwrap();
        assert_eq!(&out[..4], b"CcnK");
        assert_eq!(word(&out, 4) as usize, out.len() - 8);
        assert_eq!(&out[8..12], b"FBCh");
        assert_eq!((word(&out, 12), word(&out, 16), word(&out, 20), word(&out, 24)), (2, 0x5465_7374, 3, 1));
        assert!(out[32..156].iter().all(|b| *b == 0));
        assert_eq!(word(&out, 156), 3);
        assert_eq!(&out[160..], [1, 2, 3]);
    }

    #[test]
    fn programs_become_a_program_bank() {
        let preset = Preset {
            kind: PROGRAM_BANK,
            id: 7,
            version: 1,
            programs: 2,
            current: 1,
            parameters: 2,
            little_endian: true,
        };
        let mut buffer = Vec::new();
        for (name, values) in [("One", [0.25f32, 1.0]), ("Two", [0.5, 0.0])] {
            let mut field = [0u8; 28];
            field[..name.len()].copy_from_slice(name.as_bytes());
            buffer.extend_from_slice(&field);
            values.iter().for_each(|v| buffer.extend_from_slice(&v.to_le_bytes()));
        }
        let out = bank(&preset, &buffer).unwrap();
        assert_eq!(&out[8..12], b"FxBk");
        assert_eq!((word(&out, 24), word(&out, 28)), (2, 1));
        // The first program after the bank header: CcnK, size, FxCk, 1, id, version, count, name.
        let program = &out[156..];
        assert_eq!(&program[..4], b"CcnK");
        assert_eq!(word(program, 4), 4 * 5 + 28 + 8);
        assert_eq!(&program[8..12], b"FxCk");
        assert_eq!((word(program, 12), word(program, 16), word(program, 24)), (1, 7, 2));
        assert_eq!(&program[28..31], b"One");
        assert_eq!(f32::from_bits(word(program, 56)), 0.25);
        assert_eq!(f32::from_bits(word(program, 60)), 1.0);
        assert_eq!(out.len(), 156 + 2 * (8 + 4 * 5 + 28 + 8));
        assert_eq!(&out[156 + 64 + 28..156 + 64 + 31], b"Two");

        assert!(bank(&preset, &buffer[1..]).is_err());
        assert!(bank(&Preset { programs: 0, ..preset }, &[]).is_err());
        assert!(bank(&Preset { kind: 5, ..preset }, &buffer).is_err());
        assert!(bank(&Preset { parameters: 1 << 20, ..preset }, &buffer).is_err());
    }
}
