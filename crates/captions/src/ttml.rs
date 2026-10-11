//! TTML (W3C Timed Text Markup Language): IMSC1 Text profile (`.ttml`, written per TTML2 /
//! IMSC 1.1 Text) and DFXP (`.dfxp`, TTML1's Distribution Format Exchange Profile).
//!
//! **Writing.** One `<p>` per cue inside `body/div`, lines separated by `<br/>`; `<i>`, `<b>` and
//! `<u>` become `<span>`s with `tts:fontStyle="italic"`, `tts:fontWeight="bold"` and
//! `tts:textDecoration="underline"`. IMSC1 files signal `ttp:profile` (the IMSC1 Text profile
//! designator), a bottom region and a default style. With a frame rate, IMSC1 times are frame
//! offsets (`123f`) at `ttp:frameRate` / `ttp:frameRateMultiplier`, which are exact; without
//! one, and always for DFXP (for older players), times are clock times with milliseconds.
//!
//! **Reading** is namespace-agnostic (TTML1 2010, the 2006 `ttaf1` drafts and TTML2 all read):
//! clock times (`HH:MM:SS`, `.fraction`, `:FF` frames and `.subframes`), offset times (`h`, `m`,
//! `s`, `ms`, `f`, `t` with `ttp:tickRate`), `begin` / `end` / `dur`, nested time containers
//! (`body` / `div` begins add up), timed `<span>`s inside an untimed `<p>`, `<br/>`, whitespace
//! collapsing, styles referenced by id (italic / bold / underline) and `ttm:agent` / `xml:id`.

use crate::time::{FrameRate, TICKS_PER_SECOND};
use crate::{Cue, Document, Error, Result, TICKS_PER_MS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    Imsc1,
    Dfxp,
}

const NS_TT: &str = "http://www.w3.org/ns/ttml";
const NS_TTP: &str = "http://www.w3.org/ns/ttml#parameter";
const NS_TTS: &str = "http://www.w3.org/ns/ttml#styling";
const NS_TTM: &str = "http://www.w3.org/ns/ttml#metadata";
const IMSC1_TEXT: &str = "http://www.w3.org/ns/ttml/profile/imsc1/text";
const DFXP_PROFILE: &str = "http://www.w3.org/ns/ttml/profile/dfxp-presentation";

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c => o.push(c),
        }
    }
    o
}

fn clock(t: i64) -> String {
    let ms = (t.max(0) + TICKS_PER_MS / 2) / TICKS_PER_MS;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}

fn time_expr(t: i64, rate: Option<FrameRate>) -> String {
    match rate {
        Some(r) if r.tick_of(r.frame_at(t)) == t => format!("{}f", r.frame_at(t)),
        _ => clock(t),
    }
}

fn inline(text: &str) -> String {
    let mut out = String::new();
    let mut open: Vec<&str> = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('<')
            && let Some(end) = after.find('>')
        {
            let tag = after[..end].trim().to_ascii_lowercase();
            let (closing, name) = match tag.strip_prefix('/') {
                Some(n) => (true, n.trim().to_string()),
                None => (
                    false,
                    tag.split_whitespace().next().unwrap_or("").to_string(),
                ),
            };
            let attr = match name.as_str() {
                "i" => Some("tts:fontStyle=\"italic\""),
                "b" => Some("tts:fontWeight=\"bold\""),
                "u" => Some("tts:textDecoration=\"underline\""),
                _ => None,
            };
            if let Some(a) = attr {
                if closing {
                    if let Some(pos) = open.iter().rposition(|x| *x == a) {
                        let inner: Vec<&str> = open.drain(pos..).skip(1).collect();
                        for _ in 0..=inner.len() {
                            out.push_str("</span>");
                        }
                        for x in inner {
                            out.push_str(&format!("<span {x}>"));
                            open.push(x);
                        }
                    }
                } else {
                    out.push_str(&format!("<span {a}>"));
                    open.push(a);
                }
            }
            rest = &after[end + 1..];
            continue;
        }
        let Some(ch) = rest.chars().next() else { break };
        if ch == '\n' {
            out.push_str("<br/>");
        } else {
            out.push_str(&esc(&ch.to_string()));
        }
        rest = &rest[ch.len_utf8()..];
    }
    for _ in open {
        out.push_str("</span>");
    }
    out
}

fn is_ncname(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_alphabetic() || f == '_')
        && c.all(|x| x.is_alphanumeric() || matches!(x, '_' | '-' | '.'))
}

pub fn write(doc: &Document, flavor: Flavor, rate: Option<FrameRate>, lang: &str) -> String {
    let mut o = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let rate = if flavor == Flavor::Imsc1 { rate } else { None };
    o.push_str(&format!(
        "<tt xmlns=\"{NS_TT}\" xmlns:ttp=\"{NS_TTP}\" xmlns:tts=\"{NS_TTS}\" xmlns:ttm=\"{NS_TTM}\""
    ));
    o.push_str(&format!(
        " ttp:profile=\"{}\" ttp:timeBase=\"media\"",
        if flavor == Flavor::Imsc1 {
            IMSC1_TEXT
        } else {
            DFXP_PROFILE
        }
    ));
    if let Some(r) = rate {
        let base = r.timecode_base();
        o.push_str(&format!(" ttp:frameRate=\"{base}\""));
        let (mn, md) = (r.num, r.den * base);
        let g = gcd(mn, md);
        if (mn / g, md / g) != (1, 1) {
            o.push_str(&format!(
                " ttp:frameRateMultiplier=\"{} {}\"",
                mn / g,
                md / g
            ));
        }
    }
    o.push_str(&format!(
        " xml:lang=\"{}\">\n",
        esc(if lang.is_empty() { "en" } else { lang })
    ));
    o.push_str("  <head>\n");
    o.push_str("    <styling>\n      <style xml:id=\"s1\" tts:color=\"white\" tts:fontFamily=\"proportionalSansSerif\" tts:fontSize=\"100%\" tts:textAlign=\"center\"/>\n    </styling>\n");
    o.push_str("    <layout>\n      <region xml:id=\"r1\" tts:origin=\"10% 10%\" tts:extent=\"80% 80%\" tts:displayAlign=\"after\"/>\n    </layout>\n");
    o.push_str("  </head>\n  <body region=\"r1\" style=\"s1\">\n    <div>\n");
    for (i, c) in doc.cues.iter().enumerate() {
        let id =
            c.id.as_deref()
                .filter(|x| is_ncname(x))
                .map(str::to_string)
                .unwrap_or_else(|| format!("c{}", i + 1));
        let agent = c
            .speaker
            .as_deref()
            .map(|s| {
                format!(
                    " ttm:role=\"x-speaker\" ttm:agent=\"{}\"",
                    esc(&agent_id(s))
                )
            })
            .unwrap_or_default();
        o.push_str(&format!(
            "      <p xml:id=\"{}\" begin=\"{}\" end=\"{}\"{agent}>{}</p>\n",
            esc(&id),
            time_expr(c.start, rate),
            time_expr(c.end, rate),
            inline(&c.text)
        ));
    }
    o.push_str("    </div>\n  </body>\n</tt>\n");
    o
}

fn agent_id(s: &str) -> String {
    let v: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if v.chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
    {
        v
    } else {
        format!("_{v}")
    }
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 {
        a.abs().max(1)
    } else {
        gcd(b, a % b)
    }
}

#[derive(Clone, Copy)]
struct Timing {
    rate: FrameRate,
    sub_frame_rate: i64,
    tick_rate: i64,
}

fn positive_integer(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<i64>().ok().filter(|v| *v > 0)
}

fn parse_ratio(s: &str) -> Option<(i64, i64)> {
    let mut it = s.split_whitespace().map(positive_integer);
    let a = it.next()??;
    let b = it.next()??;
    it.next().is_none().then_some((a, b))
}

fn attr<'a>(n: roxmltree::Node<'a, '_>, local: &str) -> Option<&'a str> {
    n.attributes()
        .find(|a| a.name() == local)
        .map(|a| a.value())
}

fn timing(root: roxmltree::Node) -> Result<Timing> {
    let invalid = |name: &str| Error::Syntax {
        line: root.document().text_pos_at(root.range().start).row as usize,
        msg: format!("invalid TTML timing parameter {name}"),
    };
    let integer = |name: &str, default| match attr(root, name) {
        Some(v) => positive_integer(v).ok_or_else(|| invalid(name)),
        None => Ok(default),
    };
    let base = integer("frameRate", 30)?;
    let (mn, md) = match attr(root, "frameRateMultiplier") {
        Some(v) => parse_ratio(v).ok_or_else(|| invalid("frameRateMultiplier"))?,
        None => (1, 1),
    };
    let rate = FrameRate::new(
        base.checked_mul(mn)
            .ok_or_else(|| invalid("frameRateMultiplier"))?,
        md,
    );
    let sub = integer("subFrameRate", 1)?;
    let tick_rate = match attr(root, "tickRate") {
        Some(v) => positive_integer(v).ok_or_else(|| invalid("tickRate"))?,
        None if attr(root, "frameRate").is_some() => base
            .checked_mul(sub)
            .ok_or_else(|| invalid("subFrameRate"))?,
        None => 1,
    };
    Ok(Timing {
        rate,
        sub_frame_rate: sub,
        tick_rate,
    })
}

fn decimal_ticks(int: &str, frac: &str, unit_ticks: i128) -> Option<i128> {
    let i: i128 = if int.is_empty() { 0 } else { int.parse().ok()? };
    let mut t = i * unit_ticks;
    if !frac.is_empty() {
        if frac.len() > 12 || !frac.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let f: i128 = frac.parse().ok()?;
        t += f * unit_ticks / 10i128.pow(frac.len() as u32);
    }
    Some(t)
}

fn parse_time(s: &str, tm: Timing) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let tps = TICKS_PER_SECOND as i128;
    if s.contains(':') {
        let parts: Vec<&str> = s.split(':').collect();
        let num = |p: &str| -> Option<i128> {
            (!p.is_empty() && p.len() <= 6 && p.bytes().all(|b| b.is_ascii_digit()))
                .then(|| p.parse().ok())
                .flatten()
        };
        let (h, m) = (num(parts.first()?)?, num(parts.get(1)?)?);
        let sec_part = parts.get(2)?;
        let (si, sf) = sec_part.split_once('.').unwrap_or((sec_part, ""));
        let mut t = (h * 3600 + m * 60) * tps + decimal_ticks(si, sf, tps)?;
        if let Some(fp) = parts.get(3) {
            if parts.len() > 4 {
                return None;
            }
            let (fi, fs) = fp.split_once('.').unwrap_or((fp, ""));
            let frames = num(fi)?;
            let sub: i128 = if fs.is_empty() { 0 } else { num(fs)? };
            let ft = tm.rate.tick_of(1) as i128;
            t += frames * ft + sub * ft / tm.sub_frame_rate.max(1) as i128;
        }
        return Some(t as i64);
    }
    let unit_start = s.find(|c: char| c.is_ascii_alphabetic())?;
    let (v, unit) = s.split_at(unit_start);
    let (vi, vf) = v.split_once('.').unwrap_or((v, ""));
    if !vi.bytes().all(|b| b.is_ascii_digit()) || vi.len() > 15 {
        return None;
    }
    let t = match unit {
        "h" => decimal_ticks(vi, vf, tps * 3600)?,
        "m" => decimal_ticks(vi, vf, tps * 60)?,
        "s" => decimal_ticks(vi, vf, tps)?,
        "ms" => decimal_ticks(vi, vf, tps / 1000)?,
        "f" => {
            let n: i128 = vi.parse().ok()?;
            let fd = tm.rate.tick_of(1) as i128;
            let whole = tm.rate.tick_of(n as i64) as i128;
            whole + decimal_ticks("0", vf, fd)?
        }
        "t" => {
            let n: i128 = vi.parse().ok()?;
            n * tps / tm.tick_rate.max(1) as i128
        }
        _ => return None,
    };
    Some(t as i64)
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Fmt {
    italic: bool,
    bold: bool,
    underline: bool,
}

fn style_of(
    n: roxmltree::Node,
    styles: &std::collections::HashMap<String, Fmt>,
    parent: Fmt,
) -> Fmt {
    let mut f = parent;
    if let Some(refs) = attr(n, "style") {
        for r in refs.split_whitespace() {
            if let Some(s) = styles.get(r) {
                f.italic |= s.italic;
                f.bold |= s.bold;
                f.underline |= s.underline;
            }
        }
    }
    own_fmt(n, &mut f);
    f
}

fn own_fmt(n: roxmltree::Node, f: &mut Fmt) {
    if let Some(v) = attr(n, "fontStyle") {
        f.italic = v.trim() == "italic" || v.trim() == "oblique";
    }
    if let Some(v) = attr(n, "fontWeight") {
        f.bold = v.trim() == "bold";
    }
    if let Some(v) = attr(n, "textDecoration") {
        let v = v.trim();
        if v.contains("noUnderline") || v == "none" {
            f.underline = false;
        } else if v.contains("underline") {
            f.underline = true;
        }
    }
}

struct Out {
    s: String,
    pending: bool,
}

impl Out {
    fn flush(&mut self) {
        if self.pending && !self.s.is_empty() && !self.s.ends_with('\n') {
            self.s.push(' ');
        }
        self.pending = false;
    }
}

fn collect(
    n: roxmltree::Node,
    styles: &std::collections::HashMap<String, Fmt>,
    fmt: Fmt,
    out: &mut Out,
) {
    for c in n.children() {
        if c.is_text() {
            for ch in c.text().unwrap_or("").chars() {
                if ch.is_whitespace() {
                    out.pending = true;
                } else {
                    out.flush();
                    out.s.push(ch);
                }
            }
        } else if c.is_element() {
            match c.tag_name().name() {
                "br" => {
                    out.pending = false;
                    out.s.push('\n');
                }
                "span" => {
                    let f = style_of(c, styles, fmt);
                    let tags: Vec<&str> = [
                        (f.italic && !fmt.italic, "i"),
                        (f.bold && !fmt.bold, "b"),
                        (f.underline && !fmt.underline, "u"),
                    ]
                    .into_iter()
                    .filter(|x| x.0)
                    .map(|x| x.1)
                    .collect();
                    if !tags.is_empty() {
                        out.flush();
                    }
                    for t in &tags {
                        out.s.push_str(&format!("<{t}>"));
                    }
                    collect(c, styles, f, out);
                    for t in tags.iter().rev() {
                        out.s.push_str(&format!("</{t}>"));
                    }
                }
                _ => collect(c, styles, fmt, out),
            }
        }
    }
}

fn clean(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

pub fn parse(text: &str) -> Result<Document> {
    let xml = roxmltree::Document::parse(text).map_err(|_| Error::NotFormat("TTML"))?;
    let root = xml.root_element();
    if root.tag_name().name() != "tt" {
        return Err(Error::NotFormat("TTML"));
    }
    let tm = timing(root)?;
    let mut styles = std::collections::HashMap::new();
    for s in root
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "style")
    {
        if let Some(id) = s.attributes().find(|a| a.name() == "id").map(|a| a.value()) {
            let mut f = Fmt::default();
            own_fmt(s, &mut f);
            styles.insert(id.to_string(), f);
        }
    }
    let mut doc = Document::default();
    let Some(body) = root
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "body")
    else {
        return Ok(doc);
    };
    walk(body, 0, &tm, &styles, Fmt::default(), &mut doc);
    doc.cues.sort_by_key(|c| (c.start, c.end));
    Ok(doc)
}

fn times(
    n: roxmltree::Node,
    parent: i64,
    tm: &Timing,
    doc: &mut Document,
) -> (Option<i64>, Option<i64>) {
    let p = |k: &str, doc: &mut Document| -> Option<i64> {
        let v = attr(n, k)?;
        let t = parse_time(v, *tm);
        if t.is_none() {
            doc.warnings.push(format!("bad time expression \"{v}\""));
        }
        t
    };
    let begin = p("begin", doc).map(|b| parent + b);
    let end = match (p("end", doc), p("dur", doc)) {
        (Some(e), _) => Some(parent + e),
        (None, Some(d)) => Some(begin.unwrap_or(parent) + d),
        _ => None,
    };
    (begin, end)
}

fn walk(
    n: roxmltree::Node,
    parent: i64,
    tm: &Timing,
    styles: &std::collections::HashMap<String, Fmt>,
    fmt: Fmt,
    doc: &mut Document,
) {
    for c in n.children().filter(|c| c.is_element()) {
        let f = style_of(c, styles, fmt);
        match c.tag_name().name() {
            "div" => {
                let (b, _) = times(c, parent, tm, doc);
                walk(c, b.unwrap_or(parent), tm, styles, f, doc);
            }
            "p" => {
                let (b, e) = times(c, parent, tm, doc);
                let speaker =
                    attr(c, "agent").map(|a| a.trim().trim_start_matches('_').replace('_', " "));
                let id = c
                    .attributes()
                    .find(|a| a.name() == "id")
                    .map(|a| a.value().to_string());
                match (b, e) {
                    (Some(b), Some(e)) => {
                        let mut o = Out {
                            s: String::new(),
                            pending: false,
                        };
                        let tags: Vec<&str> = [(f.italic, "i"), (f.bold, "b"), (f.underline, "u")]
                            .into_iter()
                            .filter(|x| x.0)
                            .map(|x| x.1)
                            .collect();
                        for t in &tags {
                            o.s.push_str(&format!("<{t}>"));
                        }
                        collect(c, styles, f, &mut o);
                        for t in tags.iter().rev() {
                            o.s.push_str(&format!("</{t}>"));
                        }
                        let text = clean(&o.s);
                        if e > b && !text.is_empty() {
                            doc.cues.push(Cue {
                                start: b,
                                end: e,
                                text,
                                speaker,
                                id,
                                ..Default::default()
                            });
                        }
                    }
                    _ => {
                        let base = b.unwrap_or(parent);
                        for sp in c
                            .children()
                            .filter(|x| x.is_element() && x.tag_name().name() == "span")
                        {
                            let (sb, se) = times(sp, base, tm, doc);
                            if let (Some(sb), Some(se)) = (sb, se) {
                                let mut o = Out {
                                    s: String::new(),
                                    pending: false,
                                };
                                collect(sp, styles, style_of(sp, styles, f), &mut o);
                                let text = clean(&o.s);
                                if se > sb && !text.is_empty() {
                                    doc.cues.push(Cue {
                                        start: sb,
                                        end: se,
                                        text,
                                        speaker: speaker.clone(),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
