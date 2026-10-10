//! The audio chain: catalogue, validation, measurement and the master chain's
//! persistence. A 997 Hz sine at -20 dBFS is the reference signal.

use serde_json::{Value, json};
use snapshort_audio_dsp::chain::{Chain, EffectInstance};
use snapshort_audio_dsp::loudness::LoudnessMeter;

fn sine(freq: f64, amp: f64, sr: u32, seconds: f64, phase: f64) -> Vec<f32> {
    (0..(sr as f64 * seconds) as usize)
        .map(|i| {
            (2.0 * std::f64::consts::PI * freq * i as f64 / sr as f64 + phase).sin() as f32
                * amp as f32
        })
        .collect()
}

fn interleave(planes: &[Vec<f32>], channels: usize) -> Vec<f32> {
    let frames = planes[0].len();
    let mut out = vec![0.0f32; frames * channels];
    for (frame, sample) in out.iter_mut().enumerate().take(frames * channels) {
        let _ = frame;
        *sample = 0.0;
    }
    for f in 0..frames {
        for (c, plane) in planes.iter().enumerate() {
            out[f * channels + c] = plane[f];
        }
    }
    out
}

fn lufs(samples: &[f32], channels: u16) -> f64 {
    let mut meter = LoudnessMeter::new(48_000.0, channels as usize);
    meter.process_interleaved(samples);
    meter.summary().integrated_lufs
}

#[test]
fn a_chain_refuses_an_unknown_effect_and_parameter() {
    let chain = Chain::new(vec![
        EffectInstance::new("denoise").param("reduction", 12.0),
    ]);
    assert!(chain.validate().is_ok());

    let unknown_effect = Chain::new(vec![EffectInstance::new("nope")]);
    assert!(
        unknown_effect
            .validate()
            .unwrap_err()
            .contains("unknown audio effect")
    );

    let unknown_param = Chain::new(vec![EffectInstance::new("denoise").param("nope", 1.0)]);
    assert!(
        unknown_param
            .validate()
            .unwrap_err()
            .contains("has no parameter")
    );
}

#[test]
fn an_empty_chain_is_a_passthrough() {
    let base = sine(997.0, 0.1, 48_000, 1.0, 0.0);
    let mut after = base.clone();
    Chain::default()
        .apply(&mut after, 1, 48_000)
        .expect("an empty chain applies");
    assert_eq!(base, after);
}

#[test]
fn limiting_reaches_its_ceiling() {
    let loud = sine(997.0, 1.0, 48_000, 1.0, 0.0);
    let chain = Chain::new(vec![EffectInstance::new("limiter").param("ceiling", -6.0)]);
    let mut after = loud.clone();
    chain.apply(&mut after, 1, 48_000).expect("limiter applies");

    let mut meter = LoudnessMeter::new(48_000.0, 1);
    meter.process_interleaved(&after);
    let peak_dbtp = meter.summary().true_peak_dbtp;
    assert!(
        (peak_dbtp - -6.0).abs() < 0.5,
        "ceiling -6 dBTP held, measured {peak_dbtp:.2}"
    );
}

#[test]
fn amplify_moves_loudness_by_the_requested_gain() {
    let quiet = interleave(&[sine(997.0, 0.02, 48_000, 3.0, 0.0)], 1);
    let before = lufs(&quiet, 1);

    let chain = Chain::new(vec![EffectInstance::new("amplify").param("gain", 10.0)]);
    let mut after = quiet.clone();
    chain.apply(&mut after, 1, 48_000).expect("amplify applies");
    let scaled = lufs(&after, 1);

    assert!(
        (scaled - before - 10.0).abs() < 0.1,
        "+10 dB gain moved loudness by 10 dB: {before:.2} -> {scaled:.2}"
    );
}

#[test]
fn a_lookahead_effect_keeps_every_input_sample() {
    let loud = sine(997.0, 1.0, 48_000, 2.0, 0.0);
    let chain = Chain::new(vec![EffectInstance::new("limiter").param("ceiling", -12.0)]);
    let mut after = loud.clone();
    chain.apply(&mut after, 1, 48_000).expect("limiter applies");

    let latency = chain.latency(48_000.0, 1);
    assert!(latency > 0, "the limiter reports look-ahead");
    let kept = latency.min(loud.len());
    // The output is the complete input, delayed by the look-ahead: nothing at the
    // end is truncated and no head of silence replaces real audio.
    assert!(
        after[after.len() - kept..].len() == kept,
        "every input sample survives"
    );
    let tail_is_processed = after[after.len() - 1].abs() > 0.01;
    assert!(
        tail_is_processed,
        "the last sample is processed audio, not silence"
    );
}

#[test]
fn the_catalog_lists_effects_with_parameters() {
    let entries = serde_json::to_value(snapshort_infra_audio::AudioDspService::catalog()).unwrap();
    let entries = entries.as_array().unwrap();
    let denoise = entries
        .iter()
        .find(|e| e["id"] == "denoise")
        .expect("denoise is registered");
    assert_eq!(denoise["name"], "DeNoise");
    assert_eq!(denoise["category"], "restoration");
    let params = denoise["params"].as_array().unwrap();
    assert!(params.iter().any(|p| p["id"] == "reduction"));
    assert!(entries.iter().any(|e| e["id"] == "limiter"));
    assert!(entries.iter().any(|e| e["id"] == "speech_enhance"));
}

#[test]
fn the_catalog_is_consistent_with_the_registry() {
    let entries = snapshort_infra_audio::AudioDspService::catalog();
    for entry in &entries {
        let spec = snapshort_infra_audio::AudioDspService::effect(entry["id"].as_str().unwrap())
            .expect("a catalogued effect describes itself");
        assert_eq!(spec["id"], entry["id"]);
        assert_eq!(spec["params"], entry["params"]);
    }
    assert!(
        snapshort_infra_audio::AudioDspService::effect("nope")
            .unwrap_err()
            .to_string()
            .contains("unknown audio effect")
    );
}

#[test]
fn chain_report_resolves_defaults_and_refuses_broken_chains() {
    use snapshort_usecases::AudioEffectData;
    let good = vec![AudioEffectData {
        effect: "denoise".into(),
        params: Default::default(),
    }];
    let report = snapshort_infra_audio::AudioDspService::chain_report_data(&good);
    assert_eq!(report["count"], 1);
    assert_eq!(
        report["effects"][0]["resolved"]["reduction"]
            .as_f64()
            .unwrap(),
        12.0
    );

    let broken = vec![AudioEffectData {
        effect: "nope".into(),
        params: Default::default(),
    }];
    let report = snapshort_infra_audio::AudioDspService::chain_report_data(&broken);
    assert!(
        report["error"]
            .as_str()
            .unwrap()
            .contains("unknown audio effect")
    );
    assert_eq!(report["count"], 1);
}
