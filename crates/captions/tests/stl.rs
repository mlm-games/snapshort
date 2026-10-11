use snapshort_captions::stl;
use snapshort_captions::time::FrameRate;

const GSI: usize = 1024;
const TTI: usize = 128;

fn cue(start: i64, end: i64, text: &str) -> snapshort_captions::Cue {
    snapshort_captions::Cue {
        start: FrameRate::FPS_25.tick_of(start),
        end: FrameRate::FPS_25.tick_of(end),
        text: text.into(),
        ..Default::default()
    }
}

#[test]
fn iso6937_text_survives_a_write_and_read() {
    let text = "Ñandú café Škoda Łódź Ærø ß £ ½ ♪ « » ‘q’ “q”";
    let doc = snapshort_captions::Document {
        cues: vec![cue(25, 75, text)],
        ..Default::default()
    };
    let b = stl::write(&doc, Some(FrameRate::FPS_25));
    let back = stl::parse(&b).unwrap();
    assert_eq!(back.cues[0].text, text);
}

#[test]
fn gsi_counts_and_extension_blocks() {
    let long = "x".repeat(150);
    let doc = snapshort_captions::Document {
        cues: vec![cue(25, 75, "<i>Hello</i>\nWörld"), cue(100, 150, &long)],
        ..Default::default()
    };
    let b = stl::write(&doc, Some(FrameRate::FPS_25));
    assert_eq!(&b[3..11], b"STL25.01");
    assert_eq!(b.len(), GSI + 3 * TTI);
    assert_eq!(&b[238..243], b"00003");
    assert_eq!(&b[243..248], b"00002");
    let tti0 = &b[GSI..GSI + TTI];
    assert_eq!(&tti0[5..9], &[0, 0, 1, 0]);
    assert_eq!(&tti0[9..13], &[0, 0, 3, 0]);
    assert_eq!(tti0[3], 0xff);
    assert_eq!(
        &tti0[16..24],
        &[0x80, b'H', b'e', b'l', b'l', b'o', 0x81, 0x8a]
    );
    assert_eq!(b[GSI + TTI + 3], 0);
    assert_eq!(b[GSI + 2 * TTI + 3], 0xff);
    let back = stl::parse(&b).unwrap();
    assert_eq!(back.cues.len(), 2);
    assert_eq!(back.cues[0].text, "<i>Hello</i>\nWörld");
    assert_eq!(back.cues[1].text, long);
    assert_eq!(back.cues[1].start, FrameRate::FPS_25.tick_of(100));
    assert!(stl::parse(&b[..500]).is_err());
}

#[test]
fn programme_start_is_subtracted() {
    let doc = snapshort_captions::Document {
        cues: vec![cue(250, 300, "A")],
        ..Default::default()
    };
    let mut b = stl::write(&doc, None);
    b[256..264].copy_from_slice(b"10000000");
    b[GSI + 5] = 10;
    b[GSI + 9] = 10;
    let back = stl::parse(&b).unwrap();
    assert_eq!(back.cues[0].start, FrameRate::FPS_25.tick_of(250));
}
