use snapshort_captions::{Document, ass, srt};

#[test]
fn ass_has_the_three_sections_and_every_cue() {
    let doc = srt::parse(
        "1\n00:00:01,000 --> 00:00:02,000\nHello\n\n2\n00:00:02,500 --> 00:00:04,000\nWorld\n",
    )
    .unwrap();
    let text = ass::write(&doc, &ass::Style::default());
    assert!(text.contains("[Script Info]"), "missing script info");
    assert!(text.contains("[V4+ Styles]"), "missing styles");
    assert!(text.contains("[Events]"), "missing events");
    assert_eq!(text.matches("Dialogue:").count(), 2);
    assert!(text.contains("0:00:01.00,0:00:02.00"), "{text}");
}

#[test]
fn ass_times_are_centiseconds() {
    assert_eq!(ass::stamp(0), "0:00:00.00");
    assert_eq!(ass::stamp(3_661_500_000), "1:01:01.50");
    assert_eq!(
        ass::stamp(-5),
        "0:00:00.00",
        "a negative time must not panic"
    );
}

#[test]
fn ass_colours_swap_into_abgr() {
    assert_eq!(ass::ass_color("FFFFFFFF"), "&H00FFFFFF");
    assert_eq!(ass::ass_color("FF0000FF"), "&H00FF0000");
    assert_eq!(
        ass::ass_color("80FF0000"),
        "&H7F0000FF",
        "alpha 0x80 inverts to 0x7F"
    );
}

#[test]
fn ass_multiline_cues_become_line_breaks() {
    let doc = Document {
        cues: vec![snapshort_captions::Cue {
            start: 0,
            end: 1_000_000,
            text: "one\ntwo".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let text = ass::write(&doc, &ass::Style::default());
    assert!(
        text.contains("\\N"),
        "a newline must become an ASS break: {text}"
    );
    assert!(!text.contains("one\ntwo}"));
}
