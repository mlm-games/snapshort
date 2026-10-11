use snapshort_captions::scc;

const RATE: snapshort_captions::time::FrameRate = snapshort_captions::time::FrameRate::FPS_29_97;

fn cue(start: i64, end: i64, text: &str) -> snapshort_captions::Cue {
    snapshort_captions::Cue {
        start: RATE.tick_of(start),
        end: RATE.tick_of(end),
        text: text.into(),
        ..Default::default()
    }
}

fn frames(c: &snapshort_captions::Cue) -> (i64, i64) {
    (RATE.frame_at(c.start), RATE.frame_at(c.end))
}

#[test]
fn drop_frame_timecodes_convert_exactly() {
    assert_eq!(scc::parse_timecode("00:01:00;02"), Some(1800));
    assert_eq!(scc::parse_timecode("00:01:00:00"), Some(1800));
    assert_eq!(scc::format_timecode(1800, true), "00:01:00;02");
    assert_eq!(scc::format_timecode(1800, false), "00:01:00:00");
    assert_eq!(scc::parse_timecode("00:10:00;00"), Some(17982));
}

#[test]
fn pop_on_write_then_read_keeps_frames_and_text() {
    let doc = snapshort_captions::Document {
        cues: vec![
            cue(60, 120, "Hello, world!"),
            cue(120, 200, "Second caption\nwith two lines"),
            cue(260, 300, "Ñandú ♪ {x} * café \"q\""),
        ],
        ..Default::default()
    };
    let s = scc::write(&doc, true);
    assert!(s.starts_with("Scenarist_SCC V1.0\n\n"));
    let back = scc::parse(&s).unwrap();
    let got: Vec<(i64, i64, &str)> = back
        .cues
        .iter()
        .map(|c| (frames(c).0, frames(c).1, c.text.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (60, 120, "Hello, world!"),
            (120, 200, "Second caption\nwith two lines"),
            (260, 300, "Ñandú ♪ {x} * café \"q\"")
        ]
    );
}

#[test]
fn roll_up_captions_decode_from_a_known_stream() {
    let scc = "Scenarist_SCC V1.0\n\n00:00:00:00\t9425 9425 94ad 94ad c1c2 94ad 94ad 4344\n\n00:00:02:00\t942c 942c\n";
    let texts: Vec<String> = scc::parse(scc)
        .unwrap()
        .cues
        .into_iter()
        .map(|c| c.text)
        .collect();
    assert_eq!(texts, vec!["AB".to_string(), "AB\nCD".to_string()]);
}

#[test]
fn long_lines_wrap_without_losing_text() {
    let rows =
        scc::wrap_rows("the quick brown fox jumps over the lazy dog and keeps running far away");
    assert!(rows.iter().all(|r| r.chars().count() <= 32));
    assert_eq!(
        rows.join(" "),
        "the quick brown fox jumps over the lazy dog and keeps running far away"
    );
}
