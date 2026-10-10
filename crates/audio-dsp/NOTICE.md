The `src/effects`, `src/biquad`, `src/channels`, `src/design`, `src/ducking`, `src/fft`,
`src/loudness`, `src/oversample`, `src/remix`, `src/resample`, `src/smooth`, `src/sync` and
`src/util` modules are ported from ArtCraft's FilmCraft:

    filmcraft-audio-dsp — https://github.com/storytold/filmcraft (crates/audio-dsp)
    Copyright (c) the FilmCraft authors

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
The original crate is intentionally dependency-free; that property, the `AudioEffect`
trait, the effect registry and the BS.1770 / EBU R128 metering are unchanged, as are the
Premiere-parity tests in `src/effects/premiere_tests.rs` and the ffmpeg ebur128 oracle in
`tests/loudness_oracle.rs`.

Added in snapshort: `src/chain.rs` (ordered chains by effect id with parameter
overrides, look-ahead compensation) and a 16-bit float WAV writer in
`tests/common/mod.rs`.

Reistribution of the combined work keeps this notice. The snapshort workspace as a
whole is GPL-3.0-only; this crate keeps its original dual licence.
