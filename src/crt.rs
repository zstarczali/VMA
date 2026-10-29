//! Magic Desk cartridge (CRT type 19, 8 × 8K banks at $8000) of a PRG: the app's `buildMagicDeskCrt`
//! (Build → Save CRT).
//!
//! Bank 0: the cold / warm vectors ($8009), the "CBM80" signature and a loader ($8009–$807F): the
//! KERNAL init, then a copy routine at $0100 in RAM that streams the payload from $8080 on —
//! switching banks through $DE00 — to its load address, turns the cartridge off and jumps to the
//! entry. The payload continues in banks 1–7.

use std::fmt;

pub const BANK_SIZE: usize = 0x2000;
pub const BANK_COUNT: usize = 8;
pub const LOADER_SIZE: usize = 0x80;
/// 8 × 8192 − 128 = 65 408 bytes
pub const MAX_PAYLOAD: usize = BANK_SIZE * BANK_COUNT - LOADER_SIZE;
pub const DEFAULT_NAME: &str = "VA CART";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CrtError {
    /// no bytes behind the load address (the app's `saveCrtEmpty`)
    Empty,
    /// the app's `saveCrtTooLarge` {size, max}
    TooLarge { size: usize, max: usize },
}

impl fmt::Display for CrtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrtError::Empty => write!(f, "the program is empty, there is nothing to put on the cartridge"),
            CrtError::TooLarge { size, max } => write!(f, "the program is too large for a Magic Desk cartridge ({size} > {max} bytes)"),
        }
    }
}

impl std::error::Error for CrtError {}

/// `buildMagicDeskLoaderBank0`: bank 0 with the loader and the first payload chunk.
fn loader_bank0(load: u16, payload: &[u8], entry: u16) -> (Vec<u8>, usize) {
    let mut bank = vec![0u8; BANK_SIZE];
    let [ll, lh] = load.to_le_bytes();
    let [nl, nh] = (payload.len() as u16).to_le_bytes();
    let [el, eh] = entry.to_le_bytes();
    bank[..9].copy_from_slice(&[
        0x09, 0x80, // cold vector -> $8009
        0x09, 0x80, // warm vector -> $8009
        0xC3, 0xC2, 0xCD, 0x38, 0x30, // "CBM80"
    ]);
    #[rustfmt::skip]
    let code: [u8; LOADER_SIZE - 9] = [
        0x78,                         // SEI
        0xA2, 0xFF,                   // LDX #$FF
        0x9A,                         // TXS
        0xD8,                         // CLD
        0x20, 0xA3, 0xFD,             // JSR $FDA3 (IOINIT)
        0x20, 0x50, 0xFD,             // JSR $FD50 (RAMTAS)
        0x20, 0x15, 0xFD,             // JSR $FD15 (RESTOR)
        0x20, 0x5B, 0xFF,             // JSR $FF5B (CINT)
        0xA9, 0x80, 0x85, 0xFB,       // src = $8080
        0xA9, 0x80, 0x85, 0xFC,
        0xA9, ll,   0x85, 0xFD,       // dst = load address
        0xA9, lh,   0x85, 0xFE,
        0xA9, nl,   0x85, 0x02,       // remaining = length
        0xA9, nh,   0x85, 0x03,
        0xA9, 0x00, 0x85, 0x04,       // bank = 0
        // @ $8036: the copy routine ($8044–$807F) to $0100 and run it from RAM — the loop
        // switches banks under $8000, so it cannot run from the cartridge
        0xA2, 0x3B,                   // LDX #$3B
        0xBD, 0x44, 0x80,             // LDA $8044,X
        0x9D, 0x00, 0x01,             // STA $0100,X
        0xCA,                         // DEX
        0x10, 0xF7,                   // BPL -9
        0x4C, 0x00, 0x01,             // JMP $0100
        // copy_loop @ $0100
        0xA5, 0x02, 0x05, 0x03,       // LDA rem_lo / ORA rem_hi
        0xF0, 0x2E,                   // BEQ done ($0134)
        0xA0, 0x00,                   // LDY #$00
        0xB1, 0xFB,                   // LDA (src),Y
        0x91, 0xFD,                   // STA (dst),Y
        0xE6, 0xFB, 0xD0, 0x02,       // INC src_lo / BNE +2
        0xE6, 0xFC,                   // INC src_hi
        0xA5, 0xFC, 0xC9, 0xA0,       // LDA src_hi / CMP #$A0
        0xD0, 0x0B,                   // BNE no_bank_sw ($0123)
        0xE6, 0x04,                   // INC bank
        0xA5, 0x04,                   // LDA bank
        0x8D, 0x00, 0xDE,             // STA $DE00 (bank select)
        0xA9, 0x80, 0x85, 0xFC,       // src_hi = $80
        // no_bank_sw @ $0123
        0xE6, 0xFD, 0xD0, 0x02,       // INC dst_lo / BNE +2
        0xE6, 0xFE,                   // INC dst_hi
        0xA5, 0x02, 0xD0, 0x02,       // LDA rem_lo / BNE +2
        0xC6, 0x03,                   // DEC rem_hi
        0xC6, 0x02,                   // DEC rem_lo
        0x4C, 0x00, 0x01,             // JMP copy_loop
        // done @ $0134
        0xA9, 0x80,                   // LDA #$80 (bit 7: cartridge off)
        0x8D, 0x00, 0xDE,             // STA $DE00
        0x4C, el,   eh,               // JMP entry
    ];
    bank[9..LOADER_SIZE].copy_from_slice(&code);
    let first = payload.len().min(BANK_SIZE - LOADER_SIZE);
    bank[LOADER_SIZE..LOADER_SIZE + first].copy_from_slice(&payload[..first]);
    (bank, first)
}

/// The CRT file of `payload` (loaded at `load`, started at `entry`); `name` = the cartridge name
/// (upper case, at most 32 bytes).
pub fn magic_desk_crt(payload: &[u8], load: u16, entry: u16, name: &str) -> Result<Vec<u8>, CrtError> {
    if payload.is_empty() {
        return Err(CrtError::Empty);
    }
    if payload.len() > MAX_PAYLOAD {
        return Err(CrtError::TooLarge { size: payload.len(), max: MAX_PAYLOAD });
    }
    let chip_len = 0x10 + BANK_SIZE;
    let mut out = Vec::with_capacity(0x40 + chip_len * BANK_COUNT);
    // header: signature, length $40, version 1.00, type 19, EXROM 0, GAME 1, name
    out.extend_from_slice(b"C64 CARTRIDGE   ");
    out.extend_from_slice(&[0, 0, 0, 0x40, 0x01, 0x00, 0x00, 0x13, 0x00, 0x01]);
    out.resize(0x20, 0);
    let name = if name.is_empty() { DEFAULT_NAME } else { name }.to_uppercase();
    let name = name.as_bytes();
    out.extend((0..32).map(|i| name.get(i).copied().unwrap_or(0x20)));

    let (bank0, mut offset) = loader_bank0(load, payload, entry);
    for b in 0..BANK_COUNT {
        // CHIP packet: length, ROM, bank, $8000, $2000
        out.extend_from_slice(b"CHIP");
        out.extend_from_slice(&(chip_len as u32).to_be_bytes());
        out.extend_from_slice(&[0x00, 0x00, 0x00, b as u8, 0x80, 0x00, 0x20, 0x00]);
        if b == 0 {
            out.extend_from_slice(&bank0);
        } else {
            let chunk = &payload[offset.min(payload.len())..(offset + BANK_SIZE).min(payload.len())];
            offset += chunk.len();
            out.extend_from_slice(chunk);
            out.resize(out.len() + BANK_SIZE - chunk.len(), 0);
        }
    }
    Ok(out)
}

/// The cartridge of a PRG as the app saves it: the load address from the PRG, the entry = the SYS
/// address (or the load address without one).
pub fn crt_of_prg(prg: &[u8], sys_address: Option<i64>, name: &str) -> Result<Vec<u8>, CrtError> {
    if prg.len() <= 2 {
        return Err(CrtError::Empty);
    }
    let load = u16::from_le_bytes([prg[0], prg[1]]);
    let entry = sys_address.filter(|&a| a > 0).map_or(load, |a| a as u16);
    magic_desk_crt(&prg[2..], load, entry, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_header_and_banks() {
        let crt = crt_of_prg(&[0x01, 0x08, 0xAA, 0xBB], Some(0x080D), "demo").unwrap();
        assert_eq!(crt.len(), 0x40 + 8 * (0x10 + BANK_SIZE));
        assert_eq!(&crt[..16], b"C64 CARTRIDGE   ");
        assert_eq!(&crt[0x16..0x1A], &[0x00, 0x13, 0x00, 0x01]);
        assert_eq!(&crt[0x20..0x24], b"DEMO");
        assert_eq!(crt[0x3F], 0x20);
        let bank0 = &crt[0x50..0x50 + BANK_SIZE];
        assert_eq!(&bank0[..9], &[0x09, 0x80, 0x09, 0x80, 0xC3, 0xC2, 0xCD, 0x38, 0x30]);
        // dst = $0801, length 2, JMP $080D, payload at $8080
        assert_eq!(&bank0[0x22..0x26], &[0xA9, 0x01, 0x85, 0xFD]);
        assert_eq!(&bank0[0x2A..0x2E], &[0xA9, 0x02, 0x85, 0x02]);
        assert_eq!(&bank0[0x7D..0x80], &[0x4C, 0x0D, 0x08]);
        assert_eq!(&bank0[0x80..0x82], &[0xAA, 0xBB]);
        // bank 7's CHIP packet
        let last = 0x40 + 7 * (0x10 + BANK_SIZE);
        assert_eq!(&crt[last..last + 16], &[b'C', b'H', b'I', b'P', 0, 0, 0x20, 0x10, 0, 0, 0, 7, 0x80, 0, 0x20, 0]);
    }

    #[test]
    fn payload_spans_banks_and_limits() {
        let payload: Vec<u8> = (0..MAX_PAYLOAD).map(|i| (i % 251) as u8).collect();
        let crt = magic_desk_crt(&payload, 0x1000, 0x1000, "").unwrap();
        let bank = |b: usize| &crt[0x40 + b * (0x10 + BANK_SIZE) + 0x10..][..BANK_SIZE];
        assert_eq!(&bank(0)[LOADER_SIZE..], &payload[..BANK_SIZE - LOADER_SIZE]);
        assert_eq!(bank(1), &payload[BANK_SIZE - LOADER_SIZE..2 * BANK_SIZE - LOADER_SIZE]);
        assert_eq!(&bank(7)[BANK_SIZE - 1], payload.last().unwrap());
        assert_eq!(&crt[0x20..0x27], b"VA CART");
        assert_eq!(magic_desk_crt(&vec![0; MAX_PAYLOAD + 1], 0x1000, 0x1000, ""), Err(CrtError::TooLarge { size: MAX_PAYLOAD + 1, max: MAX_PAYLOAD }));
        assert_eq!(crt_of_prg(&[0x00, 0x10], None, ""), Err(CrtError::Empty));
    }
}
