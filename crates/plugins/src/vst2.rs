// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the standard VST2 bank (an `.fxb` file's contents) the model keeps for a VST2
//! plug-in (layout in `crates/als/src/vst2.rs`). A bank of programs (`FxBk`) holds parameter
//! values rather than the plug-in's own chunk; not every VST3 version of a plug-in reads such a
//! bank in `setState`, so the host also sets the current program's values as parameters.

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let field = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes(field.try_into().ok()?))
}

/// Size of a bank's header, before its programs or chunk.
const BANK_HEADER: usize = 156;
/// Offset of a program's parameter count, and of its values.
const PROGRAM_COUNT: usize = 24;
const PROGRAM_VALUES: usize = 56;
/// Most parameters read.
const MAX_PARAMETERS: usize = 1 << 16;

/// The current program's parameter values, in parameter order, when `bank` is a bank of
/// programs (`CcnK` … `FxBk`); `None` for a chunk bank or anything else.
pub fn program_values(bank: &[u8]) -> Option<Vec<f32>> {
    if bank.get(..4)? != b"CcnK" || bank.get(8..12)? != b"FxBk" {
        return None;
    }
    let programs = word(bank, 24)? as usize;
    let current = word(bank, 28)? as usize;
    let wanted = if current < programs { current } else { 0 };
    let mut at = BANK_HEADER;
    for index in 0..programs {
        if bank.get(at..at + 4)? != b"CcnK" {
            return None;
        }
        let size = word(bank, at + 4)? as usize;
        if index == wanted {
            let program = bank.get(at..at.checked_add(8)?.checked_add(size)?)?;
            if program.get(8..12)? != b"FxCk" {
                return None;
            }
            let count = (word(program, PROGRAM_COUNT)? as usize).min(MAX_PARAMETERS);
            let values = program.get(PROGRAM_VALUES..PROGRAM_VALUES.checked_add(count.checked_mul(4)?)?)?;
            return Some(values.as_chunks::<4>().0.iter().map(|v| f32::from_bits(u32::from_be_bytes(*v))).collect());
        }
        at = at.checked_add(8)?.checked_add(size)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bank of programs as the importer writes it, each program's values given.
    fn bank(current: u32, programs: &[&[f32]]) -> Vec<u8> {
        let mut out = b"CcnK".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(b"FxBk");
        for word in [2, 7, 1, programs.len() as u32, current] {
            out.extend_from_slice(&word.to_be_bytes());
        }
        out.resize(BANK_HEADER, 0);
        for values in programs {
            out.extend_from_slice(b"CcnK");
            out.extend_from_slice(&(4 * 5 + 28 + 4 * values.len() as u32).to_be_bytes());
            out.extend_from_slice(b"FxCk");
            for word in [1, 7, 1, values.len() as u32] {
                out.extend_from_slice(&word.to_be_bytes());
            }
            out.extend_from_slice(&[0; 28]);
            values.iter().for_each(|v| out.extend_from_slice(&v.to_be_bytes()));
        }
        out
    }

    #[test]
    fn the_current_programs_values_are_read() {
        let two = bank(1, &[&[0.1, 0.2], &[0.3, 0.4, 0.5]]);
        assert_eq!(program_values(&two), Some(vec![0.3, 0.4, 0.5]));
        assert_eq!(program_values(&bank(0, &[&[0.25]])), Some(vec![0.25]));
        // A current program out of range reads the first.
        assert_eq!(program_values(&bank(9, &[&[0.75]])), Some(vec![0.75]));
    }

    #[test]
    fn other_and_broken_banks_give_nothing() {
        let mut chunk = bank(0, &[&[0.5]]);
        chunk[8..12].copy_from_slice(b"FBCh");
        assert_eq!(program_values(&chunk), None);
        let whole = bank(1, &[&[0.1], &[0.2, 0.3]]);
        for cut in [0, 10, BANK_HEADER + 3, whole.len() - 1] {
            assert_eq!(program_values(&whole[..cut]), None, "cut at {cut}");
        }
        assert_eq!(program_values(&[]), None);
        assert_eq!(program_values(&bank(0, &[])), None);
    }
}
