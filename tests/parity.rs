// ==========================================================================
// parity.rs -- Golden-file parity test harness
//
// Runs the complete Rust signal chain over a deterministic synthetic PCM
// buffer and emits per-frame envelope + climax outputs to a CSV.  The
// Kotlin side (android/app/src/test/.../ParityTest.kt) generates the same
// PCM with the same formula and runs the same signal chain, then asserts
// that the numbers it produces match this CSV within a small epsilon.
//
// If the two sides drift (someone edits one and forgets the other),
// the CI parity test will fail and surface the regression immediately.
//
// Synthetic signal formula (must match Kotlin side byte-for-byte):
//
//   for sample i in [0, N):
//     t = i / SAMPLE_RATE
//     sig = 0.05                                       // DC offset
//         + 0.50 * sin(2π * 100  * t)                   // bass sine
//         + 0.30 * sin(2π * 1000 * t)                   // mid sine
//         + drum_hit(t)                                 // transient
//         + noise(i)                                    // LCG noise
//     window = hann_envelope(t, DURATION)               // 50ms fades
//     pcm[i] = sig * window
//
//   drum_hit(t):
//     let T = floor(t / 0.5) * 0.5    // trigger at t = 0, 0.5, 1.0, ...
//     let u = t - T                    // time since last trigger
//     if u < 0.05: 0.6 * exp(-u / 0.010)  // 50ms window, 10ms tau
//     else:        0
//
//   noise(i):
//     LCG state_{i+1} = (state_i * 1664525 + 1013904223) mod 2^32
//     return ((state_i >> 8) / 2^24 - 0.5) * 0.04       // ±0.02 RMS-ish
//     seed = 0x51ED_5EED
//
// Preset parameters are committed in this file (see TestPreset) so the
// output is fully deterministic given the input.
// ==========================================================================

use chloe_vibes::audio::{
    BeatDetector, ClimaxEngine, ClimaxPattern, EnvelopeProcessor, FrequencyMode, Gate,
    SpectralAnalyzer, TriggerMode,
};
use std::f32::consts::TAU;
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Synthetic PCM generation
// ---------------------------------------------------------------------------

const SAMPLE_RATE: f32 = 48_000.0;
const DURATION_SEC: f32 = 10.0;
const TOTAL_SAMPLES: usize = (SAMPLE_RATE as usize) * (DURATION_SEC as usize);
const FRAME_SIZE: usize = 1024;
const LCG_SEED: u32 = 0x51ED_5EED;

/// 32-bit linear congruential generator (Numerical Recipes constants).
/// Must match Kotlin side exactly: returns (state, value).
/// state update is unsigned 32-bit wrap; value is centered at 0 with
/// amplitude ~±0.02.
fn lcg_step(state: u32) -> (u32, f32) {
    let next = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let unit = ((next >> 8) as f32) / ((1u32 << 24) as f32);
    let noise = (unit - 0.5) * 0.04;
    (next, noise)
}

/// Drum-hit shape: 50ms exponential decay at 0.6 peak, retriggering at 2 Hz.
fn drum_hit(t: f32) -> f32 {
    let period = 0.5_f32;
    let trigger = (t / period).floor() * period;
    let u = t - trigger;
    if u < 0.05 {
        0.6 * (-u / 0.010).exp()
    } else {
        0.0
    }
}

/// Raised-cosine fade-in/fade-out envelope (50ms each end).
/// Avoids a click at sample 0 / the last sample.
fn hann_envelope(t: f32, duration: f32) -> f32 {
    let fade = 0.050_f32;
    if t < fade {
        0.5 * (1.0 - (std::f32::consts::PI * t / fade).cos())
    } else if t > duration - fade {
        let u = (duration - t) / fade;
        0.5 * (1.0 - (std::f32::consts::PI * u).cos())
    } else {
        1.0
    }
}

/// Generate the whole 10-second mono PCM buffer.
fn generate_pcm() -> Vec<f32> {
    let mut pcm = vec![0.0f32; TOTAL_SAMPLES];
    let mut state: u32 = LCG_SEED;
    for i in 0..TOTAL_SAMPLES {
        let (next, noise) = lcg_step(state);
        state = next;
        let t = i as f32 / SAMPLE_RATE;
        let sig = 0.05
            + 0.50 * (TAU * 100.0 * t).sin()
            + 0.30 * (TAU * 1000.0 * t).sin()
            + drum_hit(t)
            + noise;
        pcm[i] = sig * hann_envelope(t, DURATION_SEC);
    }
    pcm
}

// ---------------------------------------------------------------------------
// Test preset parameters.  Changing these invalidates the golden file.
// Matches the values committed in the Kotlin ParityTest.
// ---------------------------------------------------------------------------

struct TestPreset;

impl TestPreset {
    // Gate
    const GATE_THRESHOLD: f32 = 0.02;
    const GATE_AUTO_AMOUNT: f32 = 0.0; // auto-gate OFF for determinism
    const GATE_SMOOTHING: f32 = 0.0;

    // Envelope (drive)
    const TRIGGER_MODE: TriggerMode = TriggerMode::Dynamic;
    const THRESHOLD: f32 = 0.02;
    const THRESHOLD_KNEE: f32 = 0.0;
    const DYNAMIC_CURVE: f32 = 1.0;
    const BINARY_LEVEL: f32 = 1.0;
    const HYBRID_BLEND: f32 = 0.5;
    const ATTACK_MS: f32 = 20.0; // <50 → fast-path (Attack skipped)
    const DECAY_MS: f32 = 120.0;
    const SUSTAIN_LEVEL: f32 = 0.5;
    const RELEASE_MS: f32 = 180.0;
    const ATTACK_CURVE: f32 = 1.0;
    const DECAY_CURVE: f32 = 1.0;
    const RELEASE_CURVE: f32 = 1.0;

    // Climax
    const CLIMAX_ENABLED: bool = true;
    const CLIMAX_INTENSITY: f32 = 0.5;
    const CLIMAX_BUILD_UP_MS: f32 = 8_000.0; // minimum cycle length
    const CLIMAX_TEASE_RATIO: f32 = 0.25;
    const CLIMAX_TEASE_DROP: f32 = 0.4;
    const CLIMAX_SURGE_BOOST: f32 = 0.6;
    const CLIMAX_PULSE_DEPTH: f32 = 0.2;
    const CLIMAX_PATTERN: ClimaxPattern = ClimaxPattern::Wave;

    // Frequency extraction
    const FREQ_MODE: FrequencyMode = FrequencyMode::Full;
    const FREQ_TARGET: f32 = 1000.0;
}

// ---------------------------------------------------------------------------
// Run the signal chain; return per-frame (envelope_out, climax_out) pairs.
// ---------------------------------------------------------------------------

fn run_chain(pcm: &[f32]) -> Vec<(f32, f32)> {
    let mut analyzer = SpectralAnalyzer::new(SAMPLE_RATE);
    let mut gate = Gate::new();
    let mut beat = BeatDetector::new();
    let mut env = EnvelopeProcessor::new();
    let mut climax = ClimaxEngine::new();

    let num_frames = pcm.len() / FRAME_SIZE;
    let mut out = Vec::with_capacity(num_frames);

    for frame_idx in 0..num_frames {
        let start = frame_idx * FRAME_SIZE;
        let end = start + FRAME_SIZE;
        let chunk = &pcm[start..end];

        // currentTimeMs advances by FRAME_SIZE samples per frame.
        let current_time_ms = (frame_idx as f32) * (FRAME_SIZE as f32) * 1000.0 / SAMPLE_RATE;

        // 1) Spectral analysis (mono, 1 channel)
        let spectral = analyzer.analyze(chunk, 1);

        // 2) Extract energy (Full mode)
        let energy = SpectralAnalyzer::extract_energy(
            &spectral,
            TestPreset::FREQ_MODE,
            TestPreset::FREQ_TARGET,
        );

        // 3) Gate
        let gate_open = gate.process(
            energy,
            TestPreset::GATE_THRESHOLD,
            TestPreset::GATE_AUTO_AMOUNT,
            TestPreset::GATE_SMOOTHING,
        );

        // 4) Beat detector
        let (is_onset, onset_strength) = beat.process(spectral.spectral_flux, current_time_ms);

        // 5) Envelope (drive does threshold + ADSR internally)
        let env_out = env.drive(
            gate_open,
            energy,
            is_onset,
            onset_strength,
            current_time_ms,
            TestPreset::TRIGGER_MODE,
            TestPreset::THRESHOLD,
            TestPreset::THRESHOLD_KNEE,
            TestPreset::DYNAMIC_CURVE,
            TestPreset::BINARY_LEVEL,
            TestPreset::HYBRID_BLEND,
            TestPreset::ATTACK_MS,
            TestPreset::DECAY_MS,
            TestPreset::SUSTAIN_LEVEL,
            TestPreset::RELEASE_MS,
            TestPreset::ATTACK_CURVE,
            TestPreset::DECAY_CURVE,
            TestPreset::RELEASE_CURVE,
            spectral.spectral_centroid,
        );

        // 6) Climax
        let climax_out = climax.process(
            env_out,
            energy,
            gate_open,
            is_onset,
            onset_strength,
            current_time_ms,
            TestPreset::CLIMAX_ENABLED,
            TestPreset::CLIMAX_INTENSITY,
            TestPreset::CLIMAX_BUILD_UP_MS,
            TestPreset::CLIMAX_TEASE_RATIO,
            TestPreset::CLIMAX_TEASE_DROP,
            TestPreset::CLIMAX_SURGE_BOOST,
            TestPreset::CLIMAX_PULSE_DEPTH,
            TestPreset::CLIMAX_PATTERN,
        );

        out.push((env_out, climax_out));
    }

    out
}

// ---------------------------------------------------------------------------
// CSV I/O
// ---------------------------------------------------------------------------

fn format_csv(frames: &[(f32, f32)]) -> String {
    let mut s = String::with_capacity(frames.len() * 40);
    s.push_str("frame,envelope_out,climax_out\n");
    for (i, (env_out, climax_out)) in frames.iter().enumerate() {
        // Fixed 7-decimal precision keeps files stable and readable.
        s.push_str(&format!("{},{:.7},{:.7}\n", i, env_out, climax_out));
    }
    s
}

fn parse_csv(raw: &str) -> Vec<(u32, f32, f32)> {
    let mut rows = Vec::new();
    for (li, line) in raw.lines().enumerate() {
        if li == 0 {
            continue; // header
        }
        if line.trim().is_empty() {
            continue;
        }
        let mut it = line.split(',');
        let frame: u32 = it.next().unwrap().trim().parse().unwrap();
        let env: f32 = it.next().unwrap().trim().parse().unwrap();
        let climax: f32 = it.next().unwrap().trim().parse().unwrap();
        rows.push((frame, env, climax));
    }
    rows
}

fn golden_path() -> PathBuf {
    // CARGO_MANIFEST_DIR is the crate root when running `cargo test`.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("parity_golden.csv");
    p
}

// ---------------------------------------------------------------------------
// The test
// ---------------------------------------------------------------------------

#[test]
fn parity_rust_golden() {
    let pcm = generate_pcm();
    assert_eq!(pcm.len(), TOTAL_SAMPLES, "PCM length mismatch");

    let frames = run_chain(&pcm);
    let csv = format_csv(&frames);

    let path = golden_path();
    let epsilon: f32 = 1e-4;

    match fs::read_to_string(&path) {
        Ok(existing) => {
            let golden = parse_csv(&existing);
            assert_eq!(
                golden.len(),
                frames.len(),
                "frame count differs: golden={} new={}",
                golden.len(),
                frames.len()
            );

            let mut max_env_diff = 0.0_f32;
            let mut max_climax_diff = 0.0_f32;
            let mut worst_frame = 0usize;
            let mut mismatches: Vec<String> = Vec::new();

            for (i, ((env_new, climax_new), (fidx, env_gold, climax_gold))) in
                frames.iter().zip(golden.iter()).enumerate()
            {
                assert_eq!(*fidx as usize, i, "frame index mismatch at row {}", i);
                let de = (env_new - env_gold).abs();
                let dc = (climax_new - climax_gold).abs();
                if de > max_env_diff || dc > max_climax_diff {
                    if de > max_env_diff {
                        max_env_diff = de;
                    }
                    if dc > max_climax_diff {
                        max_climax_diff = dc;
                    }
                    if de > epsilon || dc > epsilon {
                        worst_frame = i;
                    }
                }
                if de > epsilon || dc > epsilon {
                    if mismatches.len() < 8 {
                        mismatches.push(format!(
                            "frame {}: env {:.7} vs {:.7} (Δ={:.2e}), climax {:.7} vs {:.7} (Δ={:.2e})",
                            i, env_new, env_gold, de, climax_new, climax_gold, dc
                        ));
                    }
                }
            }

            if !mismatches.is_empty() {
                // Write the new file next to the golden so a human can diff.
                let mut new_path = path.clone();
                new_path.set_extension("csv.new");
                fs::write(&new_path, &csv).expect("write .csv.new");
                panic!(
                    "Rust parity regression vs committed golden (epsilon={}):\n  worst frame: {}\n  max env diff: {:.2e}\n  max climax diff: {:.2e}\n  first mismatches:\n    {}\n  new output written to: {}\n  (If this change is intentional, overwrite tests/parity_golden.csv and verify the Kotlin ParityTest still passes with epsilon 1e-3.)",
                    epsilon,
                    worst_frame,
                    max_env_diff,
                    max_climax_diff,
                    mismatches.join("\n    "),
                    new_path.display(),
                );
            }
        }
        Err(_) => {
            // No golden yet: write it and fail so the user can review+commit.
            fs::write(&path, &csv).expect("write golden CSV");
            panic!(
                "No golden file existed at {} — wrote a fresh one. Review it, commit it, then re-run `cargo test --test parity`.",
                path.display()
            );
        }
    }
}
