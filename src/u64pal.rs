//! `.u64_palette "f"` / `.u64_palette_color idx, $RRGGBB` / `.u64_palette_reset`: the Ultimate 64 /
//! C64U palette through the Ultimate Command Interface (`_u64UciBytes`, `_u64PalColors` in the app).
//! Control target $04: $52 + 16 × R,G,B sets all colours, $53 idx,R,G,B one, $54 the menu's palette.

/// The routine before its data (`_U64_UCI_CODE`).
pub const UCI_CODE: i64 = 59;
pub const PAL_SIZE: i64 = UCI_CODE + 50;
pub const COLOR_SIZE: i64 = UCI_CODE + 6;
pub const RESET_SIZE: i64 = UCI_CODE + 2;

/// `_u64UciBytes`: skip without an interface ($DF1D ≠ $C9), wait for idle (accepting leftover
/// data), write the command, push it, wait while busy, accept the answer.
pub fn uci_bytes(addr: i64, data: &[u8]) -> Vec<u8> {
    let len = data.len() as u8;
    let at = (addr + UCI_CODE) & 0xFFFF;
    let mut out = vec![
        0xAD, 0x1D, 0xDF, 0xC9, 0xC9, 0xD0, 52 + len,
        0xAD, 0x1C, 0xDF, 0x29, 0x30, 0xF0, 0x0B, 0x29, 0x20, 0xF0, 0xF5,
        0xA9, 0x02, 0x8D, 0x1C, 0xDF, 0xD0, 0xEE,
        0xA2, 0x00, 0xBD, at as u8, (at >> 8) as u8, 0x8D, 0x1D, 0xDF, 0xE8, 0xE0, len, 0xD0, 0xF5,
        0xA9, 0x01, 0x8D, 0x1C, 0xDF,
        0xAD, 0x1C, 0xDF, 0x29, 0x30, 0xC9, 0x10, 0xF0, 0xF7,
        0xA9, 0x02, 0x8D, 0x1C, 0xDF, 0xD0, len,
    ];
    out.extend_from_slice(data);
    out
}

pub fn palette_bytes(addr: i64, colors: &[u32]) -> Vec<u8> {
    let mut data = vec![0x04, 0x52];
    for c in colors {
        data.extend([(c >> 16) as u8, (c >> 8) as u8, *c as u8]);
    }
    uci_bytes(addr, &data)
}

pub fn color_bytes(addr: i64, index: i64, color: i64) -> Vec<u8> {
    uci_bytes(addr, &[0x04, 0x53, index as u8, (color >> 16) as u8, (color >> 8) as u8, color as u8])
}

pub fn reset_bytes(addr: i64) -> Vec<u8> {
    uci_bytes(addr, &[0x04, 0x54])
}

/// `_u64PalColorParse`: "index, colour" split at the comma.
pub fn color_parse(raw: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = raw.split(',').map(js_trim).collect();
    (parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty()).then(|| (parts[0].to_string(), parts[1].to_string()))
}

/// `_u64PalColors`: the file as a text palette (bytes as Latin-1), or a Photoshop .act.
pub fn palette_colors(bytes: &[u8]) -> Option<Vec<u32>> {
    if bytes.is_empty() {
        return None;
    }
    let text: Vec<char> = bytes.iter().map(|&b| b as char).collect();
    parse_text(&text).or_else(|| if bytes.len() >= 768 { parse_act(bytes) } else { None })
}

// JS `\s` over Latin-1 text
fn js_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}')
}

fn js_trim(s: &str) -> &str {
    s.trim_matches(js_space)
}

fn hex_val(c: char) -> Option<u32> {
    c.to_digit(16).filter(|_| c.is_ascii())
}

/// `^\s*(\d{1,3})\s+(\d{1,3})\s+(\d{1,3})(?:\s|$)` (GIMP) or, after `#…` is cut and the line trimmed,
/// `^([0-9a-f]{1,2})\s+…(?:\s|$)` (VICE); `max` = digits per field, `radix` 10 / 16.
fn triple(line: &[char], radix: u32, max: usize) -> Option<[u32; 3]> {
    let mut i = 0;
    let mut out = [0u32; 3];
    for (k, v) in out.iter_mut().enumerate() {
        if k > 0 {
            let s = i;
            while i < line.len() && js_space(line[i]) {
                i += 1;
            }
            if i == s {
                return None;
            }
        }
        let s = i;
        while i < line.len() && i - s < max && line[i].is_ascii() && line[i].to_digit(radix).is_some() {
            i += 1;
        }
        if i == s {
            return None;
        }
        // a longer run of digits does not match `{1,max}` followed by \s or the end
        *v = line[s..i].iter().fold(0, |a, c| a * radix + c.to_digit(radix).unwrap());
    }
    (i == line.len() || js_space(line[i])).then_some(out)
}

fn parse_text(text: &[char]) -> Option<Vec<u32>> {
    let joined: String = text.iter().filter(|&&c| c != '\r').collect();
    let lines: Vec<Vec<char>> = joined.split('\n').map(|l| l.chars().collect()).collect();
    let mut out = Vec::new();
    let first: Vec<char> = lines[0].iter().copied().skip_while(|&c| js_space(c)).collect();
    if first.len() >= 12 && first[..12].iter().zip("GIMP Palette".chars()).all(|(a, b)| a.eq_ignore_ascii_case(&b)) {
        for line in &lines {
            let start = line.iter().position(|&c| !js_space(c)).unwrap_or(line.len());
            if let Some([r, g, b]) = triple(&line[start..], 10, 3) {
                out.push((r.min(255) << 16) | (g.min(255) << 8) | b.min(255));
            }
        }
        return (out.len() >= 16).then(|| out[..16].to_vec());
    }
    // (?:#|\$|0x)[0-9a-f]{6}\b, case-insensitive, over the whole text
    let mut hex6 = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let pre = if text[i] == '#' || text[i] == '$' {
            1
        } else if text[i] == '0' && i + 1 < text.len() && (text[i + 1] == 'x' || text[i + 1] == 'X') {
            2
        } else {
            0
        };
        if pre > 0 && i + pre + 6 <= text.len() && text[i + pre..i + pre + 6].iter().all(|&c| hex_val(c).is_some()) {
            let end = i + pre + 6;
            if end == text.len() || !(text[end].is_ascii_alphanumeric() || text[end] == '_') {
                hex6.push(text[i + pre..end].iter().fold(0, |a, &c| a * 16 + hex_val(c).unwrap()));
                i = end;
                continue;
            }
        }
        i += 1;
    }
    if hex6.len() >= 16 {
        return Some(hex6[..16].to_vec());
    }
    for line in &lines {
        let cut = line.iter().position(|&c| c == '#').unwrap_or(line.len());
        let body = &line[..cut];
        let s = body.iter().position(|&c| !js_space(c)).unwrap_or(body.len());
        let e = body.iter().rposition(|&c| !js_space(c)).map_or(s, |p| p + 1);
        if let Some([r, g, b]) = triple(&body[s..e.max(s)], 16, 2) {
            out.push((r << 16) | (g << 8) | b);
        }
    }
    (out.len() >= 16).then(|| out[..16].to_vec())
}

fn parse_act(bytes: &[u8]) -> Option<Vec<u32>> {
    (bytes.len() >= 48).then(|| (0..16).map(|i| ((bytes[i * 3] as u32) << 16) | ((bytes[i * 3 + 1] as u32) << 8) | bytes[i * 3 + 2] as u32).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors(text: &str) -> Option<Vec<u32>> {
        palette_colors(text.as_bytes())
    }

    #[test]
    fn vice_gimp_and_hex_palettes() {
        let vpl: String = (0..16).map(|i| format!("# c{i}\n{:02X} {:02x} {:X} 4\n", i, 255 - i, i)).collect();
        assert_eq!(colors(&vpl).unwrap()[15], 0x0FF00F);
        let gpl: String = "GIMP Palette\nName: x\n#\n".to_string() + &(0..16).map(|i| format!("{} 300 {}\tname\n", i, i * 2)).collect::<String>();
        assert_eq!(colors(&gpl).unwrap()[3], 0x03FF06);
        let hex: String = (0..16).map(|i| format!("$1020{:02X}, ", i)).collect();
        assert_eq!(colors(&hex).unwrap()[9], 0x102009);
        assert_eq!(colors("$1234567 not a colour"), None);
        assert_eq!(colors("AB CD"), None);
        let mut act = vec![0u8; 772];
        act[3..6].copy_from_slice(&[1, 2, 3]);
        assert_eq!(palette_colors(&act).unwrap()[1], 0x010203);
    }

    #[test]
    fn the_routine() {
        let b = reset_bytes(0x1000);
        assert_eq!(b.len() as i64, RESET_SIZE);
        assert_eq!(&b[27..30], &[0xBD, 0x3B, 0x10]);
        assert_eq!(b[6], 54);
        assert_eq!(&b[57..], &[0xD0, 2, 0x04, 0x54]);
        assert_eq!(color_parse(" 6 , $3050C0 "), Some(("6".into(), "$3050C0".into())));
        assert_eq!(color_parse("6"), None);
    }
}
