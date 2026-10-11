use snapshort_captions::mcc;

#[test]
fn maccaption_abbreviations_round_trip() {
    let b = [
        0x61, 0x01, 0x49, 0x96, 0x69, 0xfa, 0, 0, 0xfa, 0, 0, 0xfc, 0x80, 0x80, 0x00, 0x12, 0xe1,
        0, 0, 0, 0xfb, 0x80, 0x80, 0xfd, 0x80, 0x80,
    ];
    let s = mcc::compress(&b);
    assert_eq!(s, "T49SHQZ12UPR");
    assert_eq!(mcc::expand(&s).unwrap(), b);
    assert_eq!(mcc::expand("T52S5"), None);
}

#[test]
fn cdp_length_flags_and_checksum() {
    let d = mcc::FrameData {
        f1: Some((0x14, 0x20)),
        dtvcc: vec![(true, 0x02, 0x21), (false, 0x89, 0x01)],
    };
    let b = mcc::build_cdp(7, &d);
    assert_eq!(&b[..3], &[0x61, 0x01, 73]);
    let cdp = &b[3..];
    assert_eq!(&cdp[..9], &[0x96, 0x69, 73, 0x4f, 0x43, 0, 7, 0x72, 0xf4]);
    assert_eq!(cdp.iter().map(|&x| x as u32).sum::<u32>() % 256, 0);
    let t = mcc::cc_triplets(&b).unwrap();
    assert_eq!(t.len(), 20);
    assert_eq!(t[0], [0xfc, 0x94, 0x20]);
    assert_eq!(t[2], [0xff, 0x02, 0x21]);
    assert_eq!(t[19], [0xfa, 0, 0]);
}

#[test]
fn both_caption_streams_round_trip() {
    let r = mcc::RATE;
    let doc = snapshort_captions::Document {
        cues: vec![
            snapshort_captions::Cue {
                start: r.tick_of(60),
                end: r.tick_of(120),
                text: "Hello, world!".into(),
                ..Default::default()
            },
            snapshort_captions::Cue {
                start: r.tick_of(120),
                end: r.tick_of(200),
                text: "Second caption\nwith two lines".into(),
                ..Default::default()
            },
            snapshort_captions::Cue {
                start: r.tick_of(260),
                end: r.tick_of(300),
                text: "Café \"q\" ♪".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let expected = vec![
        (60, 120, "Hello, world!"),
        (120, 200, "Second caption\nwith two lines"),
        (260, 300, "Café \"q\" ♪"),
    ];
    for s in [
        mcc::write(&doc, true),
        mcc::write_with(&doc, false, false, true),
    ] {
        let back = mcc::parse(&s).unwrap();
        let got: Vec<(i64, i64, &str)> = back
            .cues
            .iter()
            .map(|c| (r.frame_at(c.start), r.frame_at(c.end), c.text.as_str()))
            .collect();
        assert_eq!(got, expected);
    }
    assert!(mcc::parse("garbage").is_err());
}
