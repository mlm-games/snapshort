use snapshort_edit::{
    DEFAULT_FILLERS, Segment, Span, Word, find_fillers, find_pauses, is_through_edit, merge_spans,
    normalize_word, paragraphs, search, snap, total_removed_us, track_gaps, track_through_edits,
    word_at, word_span,
};

fn words(spec: &[(&str, i64, i64)]) -> Vec<Word> {
    spec.iter()
        .map(|(text, start, end)| Word::new(*text, *start, *end))
        .collect()
}

#[test]
fn normalize_word_strips_punctuation_and_case() {
    assert_eq!(normalize_word("  Hello, "), "hello");
    assert_eq!(
        normalize_word("don't"),
        "don't",
        "interior punctuation stays"
    );
    assert_eq!(normalize_word("...wait..."), "wait");
    assert_eq!(normalize_word("---"), "");
}

#[test]
fn snap_lands_on_frame_boundaries_at_30fps() {
    let fps = 30.0;
    assert_eq!(snap(1_000_000, fps), 999_990, "30.0003 frames rounds down");
    assert_eq!(snap(0, fps), 0);
    let frame = 1_000_000 / 30;
    for at in [0, 1, 999_990, 1_999_980] {
        assert!(snap(at, fps) % frame <= 1, "{at} must land on a frame");
    }
    assert!(
        snap(1_000_000, 0.0) == 1_000_000,
        "a zero fps must not snap"
    );
}

#[test]
fn pauses_keep_silence_on_both_sides() {
    let words = words(&[("one", 0, 2_000_000), ("two", 5_000_000, 7_000_000)]);
    let spans = find_pauses(&words, 500_000, 200_000, 30.0);
    assert_eq!(spans.len(), 1);
    let span = spans[0];
    assert!(
        span.start_us >= 2_000_000,
        "starts after the first word plus keep"
    );
    assert!(
        span.end_us <= 5_000_000,
        "ends before the next word minus keep"
    );
    let kept = span.end_us - span.start_us;
    assert!(
        (kept - 2_600_000).abs() < 34_000,
        "kept {kept}us of the 3s gap, 400ms of padding aside, to a frame"
    );
}

#[test]
fn short_gaps_are_not_pauses() {
    let words = words(&[("one", 0, 1_000_000), ("two", 1_100_000, 2_000_000)]);
    assert!(find_pauses(&words, 500_000, 0, 30.0).is_empty());
}

#[test]
fn pauses_need_at_least_two_words() {
    assert!(find_pauses(&words(&[("one", 0, 1_000_000)]), 0, 0, 30.0).is_empty());
}

#[test]
fn fillers_match_words_and_phrases() {
    let words = words(&[
        ("so", 0, 100_000),
        ("um,", 100_000, 200_000),
        ("you", 300_000, 400_000),
        ("know", 400_000, 500_000),
        ("erm", 600_000, 700_000),
    ]);
    let hits = find_fillers(&words, &["um".to_string()]);
    assert_eq!(hits, vec![1..2]);

    let phrases = find_fillers(&words, &["you know".to_string()]);
    assert_eq!(phrases, vec![2..4], "a phrase match beats lone words");

    let defaults = find_fillers(
        &words,
        &DEFAULT_FILLERS
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    );
    assert_eq!(defaults, vec![1..2, 4..5]);
}

#[test]
fn a_phrase_filler_is_not_split_into_lone_words() {
    let words = words(&[("you", 0, 100_000), ("know", 100_000, 200_000)]);
    let hits = find_fillers(&words, &["you know".to_string()]);
    assert_eq!(hits, vec![0..2]);
}

#[test]
fn merging_joins_overlapping_and_touching_spans() {
    let merged = merge_spans(vec![
        Span::new(10, 20),
        Span::new(20, 30),
        Span::new(5, 12),
        Span::new(40, 50),
    ]);
    assert_eq!(merged, vec![Span::new(5, 30), Span::new(40, 50)]);
    assert_eq!(total_removed_us(&merged), 25 + 10);
}

#[test]
fn word_search_matches_a_sequence_case_insensitively() {
    let words = words(&[
        ("The", 0, 1_000_000),
        ("QUICK,", 1_000_000, 2_000_000),
        ("fox", 3_000_000, 4_000_000),
    ]);
    assert_eq!(search(&words, "the quick"), vec![0..2]);
    assert_eq!(search(&words, "fox"), vec![2..3]);
    assert!(search(&words, "dog").is_empty());
    assert!(search(&words, "   ").is_empty());
}

#[test]
fn paragraphs_split_on_speaker_change_and_gap() {
    let mut a = Word::new("one", 0, 1_000_000);
    a.speaker = Some("Ann".into());
    let mut b = Word::new("two", 1_000_000, 2_000_000);
    b.speaker = Some("Ann".into());
    let mut c = Word::new("three", 9_000_000, 10_000_000);
    c.speaker = Some("Bob".into());
    let all = vec![a, b, c];

    assert_eq!(paragraphs(&all, 5_000_000), vec![0..2, 2..3]);
}

#[test]
fn word_at_and_word_span_follow_the_word_list() {
    let words = words(&[("one", 0, 1_000_000), ("two", 2_000_000, 4_000_000)]);
    assert_eq!(word_at(&words, 500_000), Some(0));
    assert_eq!(word_at(&words, 1_500_000), None);
    assert_eq!(word_span(&words, 0, 1, 30.0), Some(Span::new(0, 3_999_960)));
    assert!(word_span(&words, 0, 9, 30.0).is_none());
}

#[test]
fn a_through_edit_continues_the_same_source_in_media_time() {
    let left = Segment {
        source: "a.mp4".into(),
        start_us: 0,
        duration_us: 1_000_000,
        source_in_us: 0,
        source_out_us: 1_000_000,
        speed: 1.0,
    };
    let continuing = Segment {
        source_in_us: 1_000_000,
        source_out_us: 2_000_000,
        start_us: 1_000_000,
        ..left.clone()
    };
    let other_source = Segment {
        source: "b.mp4".into(),
        ..continuing.clone()
    };
    let wrong_time = Segment {
        source_in_us: 5_000_000,
        source_out_us: 6_000_000,
        ..continuing.clone()
    };
    assert!(is_through_edit(&left, &continuing));
    assert!(!is_through_edit(&left, &other_source));
    assert!(!is_through_edit(&left, &wrong_time));
}

#[test]
fn through_edits_and_gaps_are_found_on_a_track() {
    let segments = vec![
        Segment {
            source: "a.mp4".into(),
            start_us: 0,
            duration_us: 1_000_000,
            source_in_us: 0,
            source_out_us: 1_000_000,
            speed: 1.0,
        },
        Segment {
            source: "a.mp4".into(),
            start_us: 2_000_000,
            duration_us: 1_000_000,
            source_in_us: 1_000_000,
            source_out_us: 2_000_000,
            speed: 1.0,
        },
    ];
    let edits = track_through_edits(&segments);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].at_us, 2_000_000);

    let gapped = track_gaps(&segments);
    assert_eq!(gapped, vec![Span::new(1_000_000, 2_000_000)]);
    assert!(track_gaps(&segments[..1]).is_empty());
}
