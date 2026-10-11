//! CEA-608 (line 21) character sets and control codes, from the public specification
//! (ANSI/CTA-608-E; 47 CFR §15.119).
//!
//! Bytes travel in pairs with odd parity in bit 7. A pair whose first byte (parity stripped) is
//! 0x10–0x1F is a control code (channel 1: 0x10–0x17, channel 2: 0x18–0x1F); otherwise both bytes
//! are characters from the basic set (0x00 = padding).

pub fn with_parity(b: u8) -> u8 {
    let b = b & 0x7f;
    if b.count_ones().is_multiple_of(2) {
        b | 0x80
    } else {
        b
    }
}

pub fn basic_char(b: u8) -> Option<char> {
    Some(match b {
        0x2a => 'á',
        0x5c => 'é',
        0x5e => 'í',
        0x5f => 'ó',
        0x60 => 'ú',
        0x7b => 'ç',
        0x7c => '÷',
        0x7d => 'Ñ',
        0x7e => 'ñ',
        0x7f => '█',
        0x20..=0x7e => b as char,
        _ => return None,
    })
}

pub const SPECIAL: [char; 16] = [
    '®', '°', '½', '¿', '™', '¢', '£', '♪', 'à', '\u{a0}', 'è', 'â', 'ê', 'î', 'ô', 'û',
];

pub const EXT_12: [char; 32] = [
    'Á', 'É', 'Ó', 'Ú', 'Ü', 'ü', '‘', '¡', '*', '\'', '—', '©', '℠', '•', '“', '”', 'À', 'Â', 'Ç',
    'È', 'Ê', 'Ë', 'ë', 'Î', 'Ï', 'ï', 'Ô', 'Ù', 'ù', 'Û', '«', '»',
];

pub const EXT_13: [char; 32] = [
    'Ã', 'ã', 'Í', 'Ì', 'ì', 'Ò', 'ò', 'Õ', 'õ', '{', '}', '\\', '^', '_', '|', '~', 'Ä', 'ä', 'Ö',
    'ö', 'ß', '¥', '¤', '¦', 'Å', 'å', 'Ø', 'ø', '┌', '┐', '└', '┘',
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoded {
    Basic(u8),
    Pair {
        b1: u8,
        b2: u8,
        fallback: Option<u8>,
    },
}

fn basic_byte(c: char) -> Option<u8> {
    (0x20u8..=0x7f).find(|&b| basic_char(b) == Some(c))
}

pub fn encode_char(c: char) -> Option<Encoded> {
    let c = match c {
        '’' | 'ʼ' => '\'',
        '\t' => ' ',
        '–' => '-',
        _ => c,
    };
    if let Some(b) = basic_byte(c) {
        return Some(Encoded::Basic(b));
    }
    if let Some(i) = SPECIAL.iter().position(|&s| s == c) {
        return Some(Encoded::Pair {
            b1: 0x11,
            b2: 0x30 + i as u8,
            fallback: None,
        });
    }
    let fallback = |c: char| -> u8 {
        let f = match c {
            'Á' | 'À' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
            'É' | 'È' | 'Ê' | 'Ë' => 'E',
            'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
            'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => 'O',
            'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
            'ü' | 'ù' => 'u',
            'ë' => 'e',
            'ï' | 'ì' => 'i',
            'ã' | 'ä' | 'å' => 'a',
            'ò' | 'õ' | 'ö' | 'ø' => 'o',
            'Ç' => 'C',
            'ß' => 's',
            '‘' | '“' | '”' | '«' | '»' => '"',
            '{' | '┌' | '└' => '[',
            '}' | '┐' | '┘' => ']',
            '¡' => '!',
            '*' | '•' => '.',
            _ => '-',
        };
        basic_byte(f).unwrap_or(0x2d)
    };
    if let Some(i) = EXT_12.iter().position(|&s| s == c) {
        return Some(Encoded::Pair {
            b1: 0x12,
            b2: 0x20 + i as u8,
            fallback: Some(fallback(c)),
        });
    }
    if let Some(i) = EXT_13.iter().position(|&s| s == c) {
        return Some(Encoded::Pair {
            b1: 0x13,
            b2: 0x20 + i as u8,
            fallback: Some(fallback(c)),
        });
    }
    None
}

pub mod misc {
    pub const RCL: u8 = 0x20;
    pub const BS: u8 = 0x21;
    pub const AOF: u8 = 0x22;
    pub const AON: u8 = 0x23;
    pub const DER: u8 = 0x24;
    pub const RU2: u8 = 0x25;
    pub const RU3: u8 = 0x26;
    pub const RU4: u8 = 0x27;
    pub const FON: u8 = 0x28;
    pub const RDC: u8 = 0x29;
    pub const TR: u8 = 0x2a;
    pub const RTD: u8 = 0x2b;
    pub const EDM: u8 = 0x2c;
    pub const CR: u8 = 0x2d;
    pub const ENM: u8 = 0x2e;
    pub const EOC: u8 = 0x2f;
}

pub fn decode_pac(b1: u8, b2: u8) -> Option<(usize, usize)> {
    if !(0x40..=0x7f).contains(&b2) {
        return None;
    }
    let hi = b2 >= 0x60;
    let row = match (b1, hi) {
        (0x11, false) => 1,
        (0x11, true) => 2,
        (0x12, false) => 3,
        (0x12, true) => 4,
        (0x15, false) => 5,
        (0x15, true) => 6,
        (0x16, false) => 7,
        (0x16, true) => 8,
        (0x17, false) => 9,
        (0x17, true) => 10,
        (0x10, false) => 11,
        (0x13, false) => 12,
        (0x13, true) => 13,
        (0x14, false) => 14,
        (0x14, true) => 15,
        _ => return None,
    };
    let a = b2 & 0x1f;
    let indent = if a & 0x10 != 0 {
        ((a & 0x0e) >> 1) as usize * 4
    } else {
        0
    };
    Some((row, indent))
}

pub fn encode_pac(row: usize, indent: usize) -> (u8, u8) {
    let (b1, hi) = match row {
        1 => (0x11, false),
        2 => (0x11, true),
        3 => (0x12, false),
        4 => (0x12, true),
        5 => (0x15, false),
        6 => (0x15, true),
        7 => (0x16, false),
        8 => (0x16, true),
        9 => (0x17, false),
        10 => (0x17, true),
        11 => (0x10, false),
        12 => (0x13, false),
        13 => (0x13, true),
        14 => (0x14, false),
        _ => (0x14, true),
    };
    let attr = if indent >= 4 {
        0x10 | (((indent / 4).min(7) as u8) << 1)
    } else {
        0
    };
    (b1, (if hi { 0x60 } else { 0x40 }) | attr)
}
