use super::silver;
use std::{collections::HashMap, sync::LazyLock};

#[derive(Default)]
pub(super) struct Glyph {
    pub width: i32,
    pub height: i32,
    pub x: i32,
    pub y: i32,
    pub rows: Vec<u8>,
}

pub(super) struct BitmapFont {
    pub width: u32,
    pub height: u32,
    pub ascent: i32,
    pub glyphs: HashMap<char, Glyph>,
}

pub(super) fn font(name: &str) -> Option<&'static BitmapFont> {
    static SMALL: LazyLock<BitmapFont> =
        LazyLock::new(|| parse_bdf(include_str!("../../fonts/5x8.bdf"), 5, 8));
    static UNSCII: LazyLock<BitmapFont> =
        LazyLock::new(|| parse_hex(include_str!("../../fonts/unscii-8.hex")));
    static MEDIUM: LazyLock<BitmapFont> =
        LazyLock::new(|| parse_bdf(include_str!("../../fonts/6x12.bdf"), 6, 12));
    static LARGE: LazyLock<BitmapFont> =
        LazyLock::new(|| parse_bdf(include_str!("../../fonts/8x13.bdf"), 8, 13));
    match name {
        "5x8" => Some(&SMALL),
        "8x8" => Some(&UNSCII),
        "6x12" => Some(&MEDIUM),
        "8x13" => Some(&LARGE),
        _ => None,
    }
}

/// Exact membership in the named font. Controls the renderer handles itself always count.
pub fn covers(name: &str, ch: char) -> bool {
    let control = matches!(ch, '\u{e}' | '\u{f}' | '█' | '\n');
    if name == "silver" {
        return control || silver::covers(ch);
    }
    font(name).is_some_and(|font| control || font.glyphs.contains_key(&ch))
}

pub fn cell_width(ch: char) -> u32 {
    match ch as u32 {
        0x0e | 0x0f => 0,
        0x1100..=0x115f
        | 0x2e80..=0x2eff
        | 0x2f00..=0x2fdf
        | 0x3000..=0x303e
        | 0x3041..=0x33ff
        | 0x3400..=0x4dbf
        | 0x4e00..=0x9fff
        | 0xa000..=0xa4cf
        | 0xac00..=0xd7a3
        | 0xf900..=0xfaff
        | 0xfe30..=0xfe4f
        | 0xff00..=0xff60
        | 0xffe0..=0xffe6
        | 0x20000..=0x2fffd
        | 0x30000..=0x3fffd => 2,
        _ => 1,
    }
}

fn parse_bdf(source: &str, width: u32, height: u32) -> BitmapFont {
    let mut font = BitmapFont {
        width,
        height,
        ascent: 0,
        glyphs: HashMap::new(),
    };
    let mut glyph = Glyph::default();
    let mut encoding = -1_i32;
    let mut bitmap = false;
    for line in source.lines() {
        let mut words = line.split_whitespace();
        match words.next().unwrap_or_default() {
            "FONT_ASCENT" => font.ascent = words.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            "STARTCHAR" => {
                glyph = Glyph::default();
                encoding = -1;
                bitmap = false;
            }
            "ENCODING" => encoding = words.next().and_then(|s| s.parse().ok()).unwrap_or(-1),
            "BBX" => {
                let mut next = || {
                    words
                        .next()
                        .and_then(|s| s.parse::<i32>().ok())
                        .unwrap_or(0)
                };
                glyph.width = next().clamp(0, 8);
                glyph.height = next().max(0);
                glyph.x = next();
                glyph.y = next();
            }
            "BITMAP" => bitmap = true,
            "ENDCHAR" => {
                if let Ok(code) = u32::try_from(encoding)
                    && let Some(ch) = char::from_u32(code)
                {
                    font.glyphs.insert(ch, std::mem::take(&mut glyph));
                }
                bitmap = false;
            }
            _ if bitmap => glyph
                .rows
                .push(u8::from_str_radix(line.trim(), 16).unwrap_or(0)),
            _ => {}
        }
    }
    font
}

fn parse_hex(source: &str) -> BitmapFont {
    let mut font = BitmapFont {
        width: 8,
        height: 8,
        ascent: 7,
        glyphs: HashMap::new(),
    };
    for line in source.lines() {
        let Some((code, bitmap)) = line.split_once(':') else {
            continue;
        };
        if bitmap.len() != 16 || !bitmap.is_ascii() {
            continue;
        }
        let Some(ch) = u32::from_str_radix(code, 16).ok().and_then(char::from_u32) else {
            continue;
        };
        let rows: Result<Vec<_>, _> = (0..8)
            .map(|row| u8::from_str_radix(&bitmap[row * 2..row * 2 + 2], 16))
            .collect();
        if let Ok(rows) = rows {
            font.glyphs.insert(
                ch,
                Glyph {
                    width: 8,
                    height: 8,
                    x: 0,
                    y: -1,
                    rows,
                },
            );
        }
    }
    font
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers_keep_offsets_and_skip_invalid_encodings() {
        let bdf = parse_bdf("FONT_ASCENT 7\nSTARTCHAR A\nENCODING 65\nBBX 9 2 -1 -2\nBITMAP\n80\nZZ\nENDCHAR\nSTARTCHAR bad\nENCODING -1\nENDCHAR", 5, 8);
        let glyph = &bdf.glyphs[&'A'];
        assert_eq!(
            (
                bdf.ascent,
                glyph.width,
                glyph.height,
                glyph.x,
                glyph.y,
                glyph.rows.as_slice()
            ),
            (7, 8, 2, -1, -2, &[128, 0][..])
        );
        assert_eq!(bdf.glyphs.len(), 1);
        let hex = parse_hex(
            "0041:0102030405060708\n0042:ZZ00000000000000\nnope:0000000000000000\n0043:0000",
        );
        assert_eq!(hex.glyphs.len(), 1);
        assert_eq!(hex.glyphs[&'A'].rows, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(covers("5x8", 'é'));
        assert!(!covers("5x8", '界'));
    }
}
