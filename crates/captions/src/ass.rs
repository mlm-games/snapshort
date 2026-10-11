//! Advanced SubStation Alpha (`ass`) writing: any parsed caption document as an
//! ASS script, which is the format the render path burns.

use crate::Document;

/// How an ASS script lays its cues out.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub font: String,
    pub font_size: u32,
    /// ARGB hex, `AARRGGBB`.
    pub color: String,
    /// ARGB hex.
    pub outline_color: String,
    pub outline_width: f32,
    pub shadow: bool,
    pub bold: bool,
    pub italic: bool,
    /// ASS alignment numeral: 2 = bottom centre.
    pub alignment: u8,
    pub margin: u32,
    pub play_width: u32,
    pub play_height: u32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            font: "sans-serif".into(),
            font_size: 48,
            color: "FFFFFFFF".into(),
            outline_color: "FF000000".into(),
            outline_width: 2.0,
            shadow: true,
            bold: false,
            italic: false,
            alignment: 2,
            margin: 40,
            play_width: 1920,
            play_height: 1080,
        }
    }
}

/// `AARRGGBB` as ASS's `&HAABBGGRR`. ASS runs alpha the other way: 00 is
/// opaque and FF is fully transparent, so it is inverted here.
pub fn ass_color(argb: &str) -> String {
    let digits: String = argb.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let padded = format!("{digits:0>8}");
    let (alpha, rest) = padded.split_at(2);
    let (red, rest) = rest.split_at(2);
    let (green, blue) = rest.split_at(2);
    let opacity = 255u16 - u16::from_str_radix(alpha, 16).unwrap_or(255);
    format!("&H{:02X}{}{}{}", opacity, blue, green, red)
}

/// `h:mm:ss.cc`.
pub fn stamp(us: i64) -> String {
    let cs = (us / 10_000).max(0);
    format!(
        "{}:{:02}:{:02}.{:02}",
        cs / 360_000,
        (cs / 6_000) % 60,
        (cs / 100) % 60,
        cs % 100
    )
}

fn escape_text(text: &str) -> String {
    text.replace('\r', "").replace('\n', "\\N")
}

pub fn write(doc: &Document, style: &Style) -> String {
    let mut out = String::new();
    out.push_str("[Script Info]\n");
    out.push_str("ScriptType: v4.00+\n");
    out.push_str("WrapStyle: 0\n");
    out.push_str(&format!("PlayResX: {}\n", style.play_width));
    out.push_str(&format!("PlayResY: {}\n", style.play_height));
    out.push_str("ScaledBorderAndShadow: yes\n");
    out.push_str("YCbCr Matrix: None\n\n");

    out.push_str("[V4+ Styles]\n");
    out.push_str(
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, \
         BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, \
         BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n",
    );
    out.push_str(&format!(
        "Style: Default,{},{},{},{},{},{},{},{},0,0,100,100,0,0,1,{},{},{},{},{},{},1\n",
        style.font,
        style.font_size,
        ass_color(&style.color),
        ass_color(&style.color),
        ass_color(&style.outline_color),
        "&H80000000",
        i32::from(style.bold),
        i32::from(style.italic),
        style.outline_width,
        i32::from(style.shadow),
        style.alignment,
        style.margin,
        style.margin,
        style.margin,
    ));
    out.push('\n');

    out.push_str("[Events]\n");
    out.push_str(
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
    );
    for cue in &doc.cues {
        out.push_str(&format!(
            "Dialogue: 0,{},{},Default,,0,0,0,,{}\n",
            stamp(cue.start),
            stamp(cue.end),
            escape_text(&cue.text)
        ));
    }
    out
}
