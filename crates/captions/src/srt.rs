//! SubRip (`.srt`).
//!
//! ```text
//! 1
//! 00:00:01,000 --> 00:00:03,500
//! First line
//! <i>second line</i>
//!
//! 2
//! …
//! ```
//!
//! Reading accepts what real files contain: missing or wrong indexes, `.` instead of `,`, 1–3
//! digit fractions, missing hours, extra spaces, trailing SSA position coordinates
//! (`X1:… Y2:…`), and cues not separated by a blank line (a new cue starts at a timing line, or at
//! an integer line directly followed by one). Writing renumbers from 1 and rounds to milliseconds.

use crate::{Cue, Document, Error, Result, format_clock, parse_clock};

pub fn parse_timing_line(line: &str) -> Option<(i64, i64, &str)> {
    let (a, rest) = line.split_once("-->")?;
    let start = parse_clock(a.trim())?;
    let rest = rest.trim_start();
    let end_len = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let end = parse_clock(&rest[..end_len])?;
    Some((start, end, rest[end_len..].trim()))
}

fn is_index(line: &str) -> bool {
    let l = line.trim();
    !l.is_empty() && l.len() <= 10 && l.bytes().all(|b| b.is_ascii_digit())
}

pub fn parse(text: &str) -> Result<Document> {
    let lines: Vec<&str> = text.lines().collect();
    let mut doc = Document::default();
    let mut i = 0;
    let mut found_any = false;
    while i < lines.len() {
        let Some((start, end, _settings)) = parse_timing_line(lines[i]) else {
            let l = lines[i].trim();
            if !l.is_empty() && !is_index(l) && found_any {
                doc.warnings
                    .push(format!("line {}: ignored stray text \"{l}\"", i + 1));
            }
            i += 1;
            continue;
        };
        found_any = true;
        i += 1;
        let mut text_lines: Vec<&str> = Vec::new();
        while i < lines.len() {
            let l = lines[i];
            if l.trim().is_empty() {
                break;
            }
            if parse_timing_line(l).is_some() {
                break;
            }
            if is_index(l)
                && lines
                    .get(i + 1)
                    .is_some_and(|n| parse_timing_line(n).is_some())
            {
                break;
            }
            text_lines.push(l.trim_end());
            i += 1;
        }
        if end <= start {
            doc.warnings.push(format!(
                "cue at {} has end before start; skipped",
                format_clock(start, ',')
            ));
            continue;
        }
        doc.cues.push(Cue {
            start,
            end,
            text: text_lines.join("\n"),
            ..Default::default()
        });
    }
    if !found_any && !text.trim().is_empty() {
        return Err(Error::NotFormat("SubRip"));
    }
    Ok(doc)
}

pub fn write(doc: &Document) -> String {
    let mut out = String::new();
    for (n, c) in doc.cues.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n",
            n + 1,
            format_clock(c.start, ','),
            format_clock(c.end, ',')
        ));
        for l in c.text.lines().filter(|l| !l.trim().is_empty()) {
            out.push_str(l);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}
