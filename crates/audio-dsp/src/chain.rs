//! An ordered audio chain: effects by registry id with parameter overrides,
//! applied to interleaved f32 audio.

use std::collections::BTreeMap;

use crate::AudioEffect;
use crate::effects;

/// One effect in a chain: a registry id (`"denoise"`, `"limiter"`, …) and its
/// parameter overrides; unset parameters keep the effect's defaults.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectInstance {
    pub effect: String,
    pub params: BTreeMap<String, f32>,
}

impl EffectInstance {
    pub fn new(effect: impl Into<String>) -> Self {
        Self {
            effect: effect.into(),
            params: BTreeMap::new(),
        }
    }

    pub fn param(mut self, id: &str, value: f32) -> Self {
        self.params.insert(id.to_string(), value);
        self
    }
}

/// An ordered list of effects; audio passes through them in order.
#[derive(Clone, Debug, Default)]
pub struct Chain {
    pub effects: Vec<EffectInstance>,
}

impl Chain {
    pub fn new(effects: Vec<EffectInstance>) -> Self {
        Self { effects }
    }

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Every effect id must be in the registry, and every parameter id must
    /// belong to that effect, or the chain is refused before any audio is touched.
    pub fn validate(&self) -> Result<(), String> {
        for inst in &self.effects {
            let info = effects::effect_info(&inst.effect)
                .ok_or_else(|| format!("unknown audio effect `{}`", inst.effect))?;
            for key in inst.params.keys() {
                if !info.params.iter().any(|spec| spec.id == key.as_str()) {
                    return Err(format!(
                        "effect `{}` has no parameter `{}`",
                        inst.effect, key
                    ));
                }
            }
        }
        Ok(())
    }

    /// Instantiate the chain for `(sample_rate, channels)`, with parameters applied.
    pub fn build(
        &self,
        sample_rate: f32,
        channels: usize,
    ) -> Result<Vec<Box<dyn AudioEffect>>, String> {
        self.validate()?;
        let instances = channels.max(1);
        let mut out = Vec::with_capacity(self.effects.len());
        for inst in &self.effects {
            let mut effect = effects::create_effect(&inst.effect, sample_rate, instances)
                .ok_or_else(|| format!("unknown audio effect `{}`", inst.effect))?;
            for (key, value) in &inst.params {
                effect.set_param(key, *value);
            }
            out.push(effect);
        }
        Ok(out)
    }

    /// Look-ahead the chain reports, in samples, for delay compensation.
    pub fn latency(&self, sample_rate: f32, channels: usize) -> usize {
        self.build(sample_rate, channels)
            .map(|instances| instances.iter().map(|e| e.latency()).sum())
            .unwrap_or(0)
    }

    /// Apply the chain in place to interleaved f32 audio.
    pub fn apply(
        &self,
        interleaved: &mut [f32],
        channels: u16,
        sample_rate: u32,
    ) -> Result<(), String> {
        if self.is_empty() {
            return Ok(());
        }
        let channels = channels.max(1) as usize;
        if channels == 0 || interleaved.is_empty() {
            return Ok(());
        }
        let frames = interleaved.len() / channels;
        if frames == 0 {
            return Ok(());
        }

        let mut instances = self.build(sample_rate as f32, channels)?;
        let latency: usize = instances.iter().map(|e| e.latency()).sum();

        let head = latency.min(frames);
        let mut planar: Vec<Vec<f32>> = vec![vec![0.0f32; frames + head]; channels];
        for frame in 0..frames {
            for (channel, plane) in planar.iter_mut().enumerate() {
                plane[head + frame] = interleaved[frame * channels + channel];
            }
        }

        for effect in &mut instances {
            let mut slices: Vec<&mut [f32]> = planar.iter_mut().map(|p| p.as_mut_slice()).collect();
            effect.process(&mut slices);
        }

        // Read the delayed output: the head is the look-ahead the effects
        // consumed, so output[head + i] is input i, fully processed.
        for frame in 0..frames {
            for (channel, plane) in planar.iter().enumerate() {
                interleaved[frame * channels + channel] = plane[head + frame];
            }
        }
        Ok(())
    }
}
