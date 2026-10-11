//! The master audio chain: the effect catalogue, validation, the timeline mix,
//! R128 measurement and the WAV render.
//!
//! The mix itself belongs to miniter (`mix_project_audio` / `write_pcm16_wav`);
//! this service only adds the chain, the catalogue and the numbers around it, so
//! export and the audio-only render always see the same volumes and effects.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use miniter_domain::Timeline;
use miniter_domain::TrackId;
use serde_json::{Value, json};
use snapshort_audio_dsp::chain::{Chain, EffectInstance};
use snapshort_audio_dsp::effects;
use snapshort_audio_dsp::loudness::LoudnessMeter;
use snapshort_usecases::{AppError, AppResult, AudioEffectData};

pub struct AudioDspService {
    render: snapshort_infra_render::RenderService,
}

impl Default for AudioDspService {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioDspService {
    pub fn new() -> Self {
        Self {
            render: snapshort_infra_render::RenderService::new(),
        }
    }

    /// Every effect the DSP crate can build, with its parameters, so a UI can
    /// list them and an agent can pick one by id.
    pub fn catalog() -> Vec<Value> {
        effects::effects()
            .iter()
            .map(|info| {
                json!({
                    "id": info.id,
                    "name": info.name,
                    "category": category_name(info.category),
                    "params": param_specs(info.params),
                })
            })
            .collect()
    }

    /// One effect by registry id, or a refusal naming the id.
    pub fn effect(effect: &str) -> AppResult<Value> {
        let info = effects::effect_info(effect)
            .ok_or_else(|| AppError::InvalidInput(format!("unknown audio effect `{effect}`")))?;
        Ok(json!({
            "id": info.id,
            "name": info.name,
            "category": category_name(info.category),
            "params": param_specs(info.params),
        }))
    }

    /// Saved chain data as a validated DSP chain.
    pub fn chain(data: &[AudioEffectData]) -> AppResult<Chain> {
        let chain = Chain::new(
            data.iter()
                .map(|item| EffectInstance {
                    effect: item.effect.clone(),
                    params: item.params.clone(),
                })
                .collect(),
        );
        chain
            .validate()
            .map_err(|why| AppError::InvalidInput(why))?;
        Ok(chain)
    }

    /// A chain as JSON, with each effect's parameters resolved against its spec.
    pub fn chain_report(chain: &Chain) -> Value {
        json!({
            "count": chain.len(),
            "effects": chain.effects.iter().map(|inst| {
                json!({
                    "effect": inst.effect,
                    "params": inst.params,
                    "resolved": resolved_params(&inst.effect, &inst.params),
                })
            }).collect::<Vec<_>>(),
        })
    }

    /// A chain report straight from saved data, reporting the refusal instead of
    /// failing, so a broken chain can be inspected and repaired.
    pub fn chain_report_data(data: &[AudioEffectData]) -> Value {
        match Self::chain(data) {
            Ok(chain) => {
                let mut report = Self::chain_report(&chain);
                if let Some(object) = report.as_object_mut() {
                    object.insert(
                        "saved".into(),
                        serde_json::to_value(data).unwrap_or_else(|_| json!([])),
                    );
                }
                report
            }
            Err(why) => json!({
                "count": data.len(),
                "effects": [],
                "saved": serde_json::to_value(data).unwrap_or_else(|_| json!([])),
                "error": why.to_string(),
            }),
        }
    }

    /// The timeline mix, with track and master volume baked in exactly as an
    /// export would.
    pub fn mix(
        &self,
        timeline: &Timeline,
        track_volumes: &HashMap<TrackId, f32>,
        master_volume: f32,
    ) -> AppResult<miniter_audio::mix::MixedAudio> {
        let settings = self.render.recommended_settings(timeline);
        let project = snapshort_infra_render::RenderService::project_for_export(
            timeline,
            &settings,
            track_volumes,
            master_volume,
        )
        .map_err(|why| AppError::InvalidInput(why.to_string()))?;
        miniter_audio::mix::mix_project_audio(&project, miniter_audio::mix::MixConfig::default())
            .map_err(|why| AppError::InvalidInput(why.to_string()))
    }

    /// Measure the mix, before and after the chain.
    pub fn measure(
        &self,
        timeline: &Timeline,
        track_volumes: &HashMap<TrackId, f32>,
        master_volume: f32,
        chain: &Chain,
    ) -> AppResult<Value> {
        let mixed = self.mix(timeline, track_volumes, master_volume)?;
        let input = measure(&mixed.samples, mixed.channels, mixed.sample_rate);
        let mut samples = mixed.samples.clone();
        chain
            .apply(&mut samples, mixed.channels, mixed.sample_rate)
            .map_err(|why| AppError::InvalidInput(why))?;
        let output = measure(&samples, mixed.channels, mixed.sample_rate);
        Ok(json!({
            "sampleRate": mixed.sample_rate,
            "channels": mixed.channels,
            "frames": samples.len() / (mixed.channels.max(1) as usize),
            "durationSeconds": samples.len() as f64 / (mixed.sample_rate as f64 * mixed.channels.max(1) as f64),
            "chain": Self::chain_report(chain),
            "input": input,
            "output": output,
        }))
    }

    /// Normalize the mix to a loudness target: measure, derive the gain from the
    /// R128 offset, then limit to a true-peak ceiling. Two passes over the same
    /// mixed audio, which is what makes it deterministic.
    pub fn normalize(
        &self,
        timeline: &Timeline,
        track_volumes: &HashMap<TrackId, f32>,
        master_volume: f32,
        chain: &Chain,
        target_lufs: f64,
        true_peak_dbtp: f64,
    ) -> AppResult<Value> {
        let done = self.normalize_mix(
            timeline,
            track_volumes,
            master_volume,
            chain,
            target_lufs,
            true_peak_dbtp,
        )?;
        Ok(json!({
            "sampleRate": done.sample_rate,
            "channels": done.channels,
            "frames": done.frames,
            "durationSeconds": done.duration_seconds,
            "targetLufs": target_lufs,
            "truePeakDbtp": true_peak_dbtp,
            "appliedGainDb": done.gain_db,
            "chain": Self::chain_report(chain),
            "stages": Self::chain_report(&done.stages),
            "beforeNormalization": done.before,
            "afterNormalization": done.after,
        }))
    }

    /// Normalize and write a 16-bit WAV.
    pub fn render_normalized(
        &self,
        timeline: &Timeline,
        track_volumes: &HashMap<TrackId, f32>,
        master_volume: f32,
        chain: &Chain,
        target_lufs: f64,
        true_peak_dbtp: f64,
        path: &Path,
    ) -> AppResult<Value> {
        let done = self.normalize_mix(
            timeline,
            track_volumes,
            master_volume,
            chain,
            target_lufs,
            true_peak_dbtp,
        )?;
        write_wav(path, done.sample_rate, done.channels, &done.samples)?;
        Ok(json!({
            "path": path.display().to_string(),
            "bytes": std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            "sampleRate": done.sample_rate,
            "channels": done.channels,
            "frames": done.frames,
            "durationSeconds": done.duration_seconds,
            "targetLufs": target_lufs,
            "truePeakDbtp": true_peak_dbtp,
            "appliedGainDb": done.gain_db,
            "chain": Self::chain_report(chain),
            "stages": Self::chain_report(&done.stages),
            "beforeNormalization": done.before,
            "afterNormalization": done.after,
        }))
    }

    /// Mix, normalize, and measure both sides. One implementation so the measured
    /// path and the rendered path can never disagree.
    fn normalize_mix(
        &self,
        timeline: &Timeline,
        track_volumes: &HashMap<TrackId, f32>,
        master_volume: f32,
        chain: &Chain,
        target_lufs: f64,
        true_peak_dbtp: f64,
    ) -> AppResult<Normalized> {
        let mixed = self.mix(timeline, track_volumes, master_volume)?;
        if mixed.samples.is_empty() {
            return Err(AppError::InvalidInput(
                "the timeline has no audio to normalize".into(),
            ));
        }
        let mut chained = mixed.samples.clone();
        chain
            .apply(&mut chained, mixed.channels, mixed.sample_rate)
            .map_err(|why| AppError::InvalidInput(why))?;
        let before = measure(&chained, mixed.channels, mixed.sample_rate);
        let integrated = before
            .get("integratedLufs")
            .and_then(Value::as_f64)
            .ok_or_else(|| {
                AppError::InvalidInput("the mix is silent; nothing to normalize".into())
            })?;

        let gain = snapshort_audio_dsp::loudness::normalize_gain_db(integrated, target_lufs)
            .clamp(-96.0, 24.0) as f32;
        let ceiling = true_peak_dbtp.clamp(-24.0, 0.0) as f32;
        let mut stages = chain.clone();
        stages
            .effects
            .push(EffectInstance::new("amplify").param("gain", gain));
        stages
            .effects
            .push(EffectInstance::new("limiter").param("ceiling", ceiling));

        let mut samples = mixed.samples;
        stages
            .apply(&mut samples, mixed.channels, mixed.sample_rate)
            .map_err(|why| AppError::InvalidInput(why))?;
        let after = measure(&samples, mixed.channels, mixed.sample_rate);

        let channels = mixed.channels.max(1);
        let frames = samples.len() / channels as usize;
        Ok(Normalized {
            sample_rate: mixed.sample_rate,
            channels: mixed.channels,
            frames,
            duration_seconds: frames as f64 / mixed.sample_rate as f64,
            gain_db: gain,
            before,
            after,
            stages,
            samples,
        })
    }

    /// Mix, apply the chain and write a 16-bit WAV; returns the path and the
    /// measurement of what was written.
    pub fn render_wav(
        &self,
        timeline: &Timeline,
        track_volumes: &HashMap<TrackId, f32>,
        master_volume: f32,
        chain: &Chain,
        path: &Path,
    ) -> AppResult<Value> {
        let mixed = self.mix(timeline, track_volumes, master_volume)?;
        if mixed.samples.is_empty() {
            return Err(AppError::InvalidInput(
                "the timeline has no audio to render".into(),
            ));
        }
        let mut samples = mixed.samples;
        chain
            .apply(&mut samples, mixed.channels, mixed.sample_rate)
            .map_err(|why| AppError::InvalidInput(why))?;

        write_wav(path, mixed.sample_rate, mixed.channels, &samples)?;

        let output = measure(&samples, mixed.channels, mixed.sample_rate);
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Ok(json!({
            "path": path.display().to_string(),
            "bytes": bytes,
            "sampleRate": mixed.sample_rate,
            "channels": mixed.channels,
            "frames": samples.len() / (mixed.channels.max(1) as usize),
            "durationSeconds": samples.len() as f64 / (mixed.sample_rate as f64 * mixed.channels.max(1) as f64),
            "chain": Self::chain_report(chain),
            "output": output,
        }))
    }
}

struct Normalized {
    sample_rate: u32,
    channels: u16,
    frames: usize,
    duration_seconds: f64,
    gain_db: f32,
    before: Value,
    after: Value,
    stages: Chain,
    samples: Vec<f32>,
}

fn write_wav(path: &Path, sample_rate: u32, channels: u16, samples: &[f32]) -> AppResult<()> {
    let parent = path.parent().filter(|dir| !dir.as_os_str().is_empty());
    if let Some(dir) = parent {
        std::fs::create_dir_all(dir)
            .map_err(|why| AppError::InvalidInput(format!("create {}: {why}", dir.display())))?;
    }
    miniter_audio::wav::write_pcm16_wav(path, sample_rate, channels, samples)
        .map_err(|why| AppError::InvalidInput(why.to_string()))
}

/// Speech runs of the mix, as word-like spans: each run of frames above
/// `threshold_db` (frame RMS, all channels) separated by less than
/// `min_silence_ms`, and at least `min_speech_ms` long. Dead-space removal needs
/// only the gaps between runs, so no recogniser is required.
pub fn speech_segments(
    samples: &[f32],
    channels: u16,
    sample_rate: u32,
    threshold_db: f64,
    min_speech_ms: f64,
    min_silence_ms: f64,
) -> Vec<snapshort_edit::Word> {
    let channels = channels.max(1) as usize;
    let frames = samples.len() / channels;
    if frames == 0 || sample_rate == 0 {
        return Vec::new();
    }
    let per_ms = sample_rate as f64 / 1000.0;
    let frame_len = (per_ms * 10.0).round().max(1.0) as usize;
    let min_silence_frames = (min_silence_ms / 10.0).round().max(1.0) as usize;
    let min_speech_frames = (min_speech_ms / 10.0).round().max(1.0) as usize;

    let mut loud: Vec<bool> = Vec::with_capacity(frames / frame_len + 1);
    let mut at = 0;
    while at < frames {
        let end = (at + frame_len).min(frames);
        let mut sum = 0.0f64;
        for sample in &samples[at * channels..end * channels] {
            sum += (*sample as f64) * (*sample as f64);
        }
        let count = ((end - at) * channels).max(1) as f64;
        let rms = (sum / count).sqrt();
        loud.push(20.0 * rms.max(1e-12).log10() >= threshold_db);
        at = end;
    }

    let mut out: Vec<snapshort_edit::Word> = Vec::new();
    let mut i = 0;
    while i < loud.len() {
        if !loud[i] {
            i += 1;
            continue;
        }
        let mut end = i + 1;
        let mut silence = 0;
        let mut probe = end;
        while probe < loud.len() {
            if loud[probe] {
                end = probe + 1;
                silence = 0;
            } else {
                silence += 1;
                if silence >= min_silence_frames {
                    break;
                }
            }
            probe += 1;
        }
        if end - i >= min_speech_frames {
            out.push(snapshort_edit::Word::new(
                "",
                (i * frame_len) as i64 * 1_000_000 / sample_rate as i64,
                (end * frame_len).min(frames) as i64 * 1_000_000 / sample_rate as i64,
            ));
        }
        i = end;
    }
    out
}

/// The catalogue of effects, as a Vec (see [`AudioDspService::catalog`]).
fn category_name(category: effects::Category) -> &'static str {
    match category {
        effects::Category::Filter => "filter",
        effects::Category::Dynamics => "dynamics",
        effects::Category::Time => "time",
        effects::Category::Channel => "channel",
        effects::Category::Restoration => "restoration",
        effects::Category::Pitch => "pitch",
        effects::Category::Modulation => "modulation",
        effects::Category::Reverb => "reverb",
        effects::Category::Special => "special",
    }
}

fn unit_name(unit: snapshort_audio_dsp::Unit) -> &'static str {
    use snapshort_audio_dsp::Unit;
    match unit {
        Unit::None => "",
        Unit::Decibels => "dB",
        Unit::Hertz => "Hz",
        Unit::Milliseconds => "ms",
        Unit::Seconds => "s",
        Unit::Percent => "%",
        Unit::Q => "Q",
        Unit::Ratio => ":1",
        Unit::Semitones => "st",
        Unit::Bpm => "bpm",
        Unit::Pan => "pan",
        Unit::Toggle => "toggle",
        Unit::Choice => "choice",
    }
}

fn param_specs(specs: &[snapshort_audio_dsp::ParamSpec]) -> Vec<Value> {
    specs
        .iter()
        .map(|spec| {
            json!({
                "id": spec.id,
                "name": spec.name,
                "min": spec.min,
                "max": spec.max,
                "default": spec.default,
                "unit": unit_name(spec.unit),
            })
        })
        .collect()
}

fn resolved_params(effect: &str, params: &BTreeMap<String, f32>) -> BTreeMap<String, f32> {
    let mut resolved: BTreeMap<String, f32> = effects::effect_info(effect)
        .map(|info| {
            info.params
                .iter()
                .map(|spec| (spec.id.to_string(), spec.default))
                .collect()
        })
        .unwrap_or_default();
    for (key, value) in params {
        resolved.insert(key.clone(), *value);
    }
    resolved
}

/// BS.1770 / R128 numbers for interleaved audio.
fn measure(samples: &[f32], channels: u16, sample_rate: u32) -> Value {
    let mut meter = LoudnessMeter::new(sample_rate as f64, channels.max(1) as usize);
    meter.process_interleaved(samples);
    let s = meter.summary();
    json!({
        "integratedLufs": finite(s.integrated_lufs),
        "loudnessRangeLu": finite(s.loudness_range_lu),
        "truePeakDbtp": finite(s.true_peak_dbtp),
        "samplePeakDbfs": finite(s.sample_peak_dbfs),
        "maxMomentaryLufs": finite(s.max_momentary_lufs),
        "maxShortTermLufs": finite(s.max_short_term_lufs),
    })
}

fn finite(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}
