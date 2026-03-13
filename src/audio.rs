// ==========================================================================
// audio.rs — Signal Processing Engine
// Ported from ChloeVibes Spectral Haptics Engine (JavaScript/Web Audio)
// into native Rust for use with system audio capture.
//
// This module contains:
//   - SpectralAnalyzer: FFT-based frequency analysis with band energies
//   - EnvelopeProcessor: Full ADSR envelope with configurable curves
//   - Gate: Threshold gate with hysteresis and auto-gate
//   - BeatDetector: Onset detection via spectral flux
//   - SharedSpectralData: Thread-safe wrapper for cross-thread sharing
// ==========================================================================

use std::f32::consts::{PI, TAU};
use std::sync::{Arc, Mutex};

use rustfft::{num_complex::Complex, FftPlanner};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// FFT window size. 2048 at 48kHz = ~42ms windows, ~23Hz bin resolution.
/// Good balance between frequency resolution and time resolution.
pub const FFT_SIZE: usize = 2048;

/// Number of perceptual frequency bands we split the spectrum into.
pub const NUM_BANDS: usize = 8;

/// Frequency edges for our 8 bands (Hz).
/// Sub-bass | Bass | Low-mid | Mid | Upper-mid | Presence | Brilliance | Air
const BAND_EDGES: [f32; 9] = [
    20.0, 60.0, 250.0, 500.0, 2000.0, 4000.0, 6000.0, 12000.0, 20000.0,
];

/// Labels for the bands (for UI display)
pub const BAND_NAMES: [&str; NUM_BANDS] = [
    "Sub", "Bass", "Lo-Mid", "Mid", "Hi-Mid", "Pres", "Brill", "Air",
];

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// How the trigger magnitude is calculated from audio energy.
/// Mirrors ChloeVibes' trigger mode system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TriggerMode {
    /// Intensity scales continuously with audio energy above threshold.
    /// The "normal" mode — louder = stronger vibration.
    Dynamic,
    /// Fixed output level when energy exceeds threshold, zero otherwise.
    /// Great for rhythmic on/off pulsing.
    Binary,
    /// Blend between dynamic and binary. Adjustable via hybrid_blend.
    Hybrid,
}

impl Default for TriggerMode {
    fn default() -> Self {
        Self::Dynamic
    }
}

/// Which part of the frequency spectrum to analyze.
/// Lets you isolate bass hits, vocal range, etc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FrequencyMode {
    /// Analyze the full audible spectrum (weighted toward lower freqs).
    Full,
    /// Only frequencies below target_frequency.
    LowPass,
    /// Only frequencies above target_frequency.
    HighPass,
    /// Narrow band around target_frequency with adjustable Q.
    BandPass,
}

impl Default for FrequencyMode {
    fn default() -> Self {
        Self::Full
    }
}

/// High-level modulation pattern for the climax engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ClimaxPattern {
    /// Smooth continuous rise toward the end of the cycle.
    Wave,
    /// Step-like intensity increases (plateaus then jumps).
    Stairs,
    /// Aggressive exponential ramp in the final third.
    Surge,
}

impl Default for ClimaxPattern {
    fn default() -> Self {
        Self::Wave
    }
}

/// ADSR envelope state machine states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeState {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

// ---------------------------------------------------------------------------
// SpectralData — the analysis output shared between threads
// ---------------------------------------------------------------------------

/// Results of spectral analysis, produced by the capture thread
/// and consumed by the GUI/processing thread.
#[derive(Clone, Debug)]
pub struct SpectralData {
    /// Energy in each of our 8 frequency bands (0.0 - 1.0 ish)
    pub band_energies: [f32; NUM_BANDS],
    /// Overall RMS power of the audio signal
    #[allow(dead_code)]
    pub rms_power: f32,
    /// Spectral centroid in Hz — higher = brighter sound
    #[allow(dead_code)]
    pub spectral_centroid: f32,
    /// Spectral flux — how much the spectrum changed since last frame.
    /// Spikes on transients (drum hits, note onsets).
    pub spectral_flux: f32,
    /// Frequency of the loudest bin
    #[allow(dead_code)]
    pub dominant_frequency: f32,
}

impl Default for SpectralData {
    fn default() -> Self {
        Self {
            band_energies: [0.0; NUM_BANDS],
            rms_power: 0.0,
            spectral_centroid: 0.0,
            spectral_flux: 0.0,
            dominant_frequency: 0.0,
        }
    }
}

/// Thread-safe wrapper for SpectralData. The capture thread writes,
/// the GUI thread reads. Mutex contention is negligible at these rates.
#[derive(Clone)]
pub struct SharedSpectralData(Arc<Mutex<SpectralData>>);

impl SharedSpectralData {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(SpectralData::default())))
    }

    pub fn store(&self, data: SpectralData) {
        if let Ok(mut guard) = self.0.lock() {
            *guard = data;
        }
    }

    pub fn load(&self) -> SpectralData {
        self.0.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// SpectralAnalyzer — FFT-based frequency analysis
// ---------------------------------------------------------------------------

/// Performs FFT on audio samples and extracts perceptual features.
/// This is the core upgrade over Chloe Vibes' simple RMS calculation.
///
/// Runs in the capture thread. Produces SpectralData each frame.
pub struct SpectralAnalyzer {
    planner: FftPlanner<f32>,
    /// Hann window function — reduces spectral leakage
    window: Vec<f32>,
    /// Previous frame's magnitude spectrum (for flux calculation)
    prev_magnitude: Vec<f32>,
    #[allow(dead_code)]
    sample_rate: f32,
    /// Hz per FFT bin
    bin_resolution: f32,
    /// Pre-calculated bin index ranges for each frequency band
    band_bin_ranges: [(usize, usize); NUM_BANDS],
    /// Reusable FFT input/output buffer
    fft_buffer: Vec<Complex<f32>>,
    /// Reusable FFT scratch space (avoids internal allocation)
    scratch_buffer: Vec<Complex<f32>>,
    /// Mono sample accumulation buffer
    mono_buffer: Vec<f32>,
    /// Pre-allocated magnitude buffer (avoids per-frame Vec allocation)
    magnitudes_buffer: Vec<f32>,
}

impl SpectralAnalyzer {
    pub fn new(sample_rate: f32) -> Self {
        // Generate Hann window function.
        // This tapers the edges of each analysis window to zero,
        // preventing spectral leakage artifacts in the FFT.
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (FFT_SIZE - 1) as f32).cos()))
            .collect();

        let bin_resolution = sample_rate / FFT_SIZE as f32;

        // Pre-calculate which FFT bins correspond to each frequency band.
        // This avoids recalculating every frame.
        let mut band_bin_ranges = [(0usize, 0usize); NUM_BANDS];
        for i in 0..NUM_BANDS {
            let start = (BAND_EDGES[i] / bin_resolution).round() as usize;
            let end = (BAND_EDGES[i + 1] / bin_resolution).round() as usize;
            band_bin_ranges[i] = (start.min(FFT_SIZE / 2), end.min(FFT_SIZE / 2));
        }

        let mut planner = FftPlanner::new();
        let scratch_len = planner.plan_fft_forward(FFT_SIZE).get_inplace_scratch_len();

        Self {
            planner,
            window,
            prev_magnitude: vec![0.0; FFT_SIZE / 2],
            sample_rate,
            bin_resolution,
            band_bin_ranges,
            fft_buffer: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            scratch_buffer: vec![Complex::new(0.0, 0.0); scratch_len],
            mono_buffer: Vec::with_capacity(FFT_SIZE),
            magnitudes_buffer: vec![0.0; FFT_SIZE / 2],
        }
    }

    /// Analyze a buffer of interleaved audio samples.
    /// Returns a SpectralData struct with all the extracted features.
    pub fn analyze(&mut self, samples: &[f32], channels: usize) -> SpectralData {
        // Step 1: Mix to mono by averaging channels
        self.mono_buffer.clear();
        for frame in samples.chunks(channels.max(1)) {
            let sum: f32 = frame.iter().sum();
            self.mono_buffer.push(sum / frame.len().max(1) as f32);
        }

        // Step 2: Take the last FFT_SIZE samples (or zero-pad if not enough)
        let mono_len = self.mono_buffer.len();
        let start = if mono_len > FFT_SIZE {
            mono_len - FFT_SIZE
        } else {
            0
        };
        let available = &self.mono_buffer[start..];

        // Step 3: Apply Hann window and fill FFT buffer
        for i in 0..FFT_SIZE {
            let sample = if i < available.len() {
                available[i]
            } else {
                0.0
            };
            self.fft_buffer[i] = Complex::new(sample * self.window[i], 0.0);
        }

        // Step 4: Run FFT (in-place with pre-allocated scratch)
        self.planner
            .plan_fft_forward(FFT_SIZE)
            .process_with_scratch(&mut self.fft_buffer, &mut self.scratch_buffer);

        // Step 5: Compute magnitude spectrum into pre-allocated buffer
        let half = FFT_SIZE / 2;
        // Factor 2/N for standard FFT magnitude normalization
        let scale = 2.0 / FFT_SIZE as f32;
        self.magnitudes_buffer.clear();
        self.magnitudes_buffer.extend(
            self.fft_buffer[..half]
                .iter()
                .map(|c| (c.re * c.re + c.im * c.im).sqrt() * scale),
        );
        let magnitudes = &self.magnitudes_buffer;

        // Step 6: Calculate band energies
        let mut band_energies = [0.0f32; NUM_BANDS];
        for (i, &(bin_start, bin_end)) in self.band_bin_ranges.iter().enumerate() {
            if bin_end > bin_start && bin_end <= magnitudes.len() {
                let band_slice = &magnitudes[bin_start..bin_end];
                let energy: f32 = band_slice.iter().map(|m| m * m).sum();
                band_energies[i] = (energy / band_slice.len() as f32).sqrt();
            }
        }

        // Step 7: RMS power (time domain — independent of FFT)
        let rms_power = if !available.is_empty() {
            let sum: f32 = available.iter().map(|s| s * s).sum();
            (sum / available.len() as f32).sqrt()
        } else {
            0.0
        };

        // Step 8: Spectral centroid (brightness)
        // Weighted average frequency, where weights are magnitudes.
        let (mut weighted_sum, mut total_mag) = (0.0f32, 0.0f32);
        for (i, &mag) in magnitudes.iter().enumerate() {
            let freq = i as f32 * self.bin_resolution;
            weighted_sum += freq * mag;
            total_mag += mag;
        }
        let spectral_centroid = if total_mag > 1e-10 {
            weighted_sum / total_mag
        } else {
            0.0
        };

        // Step 9: Spectral flux (half-wave rectified)
        // Only counts increases in magnitude — sensitive to onsets/transients.
        let spectral_flux: f32 = magnitudes
            .iter()
            .zip(self.prev_magnitude.iter())
            .map(|(&curr, &prev)| (curr - prev).max(0.0))
            .sum();

        // Step 10: Dominant frequency (loudest bin)
        let dominant_bin = magnitudes
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let dominant_frequency = dominant_bin as f32 * self.bin_resolution;

        // Save magnitude spectrum for next frame's flux calculation
        self.prev_magnitude.copy_from_slice(magnitudes);

        SpectralData {
            band_energies,
            rms_power,
            spectral_centroid,
            spectral_flux,
            dominant_frequency,
        }
    }

    /// Extract energy from specific frequency range based on mode.
    /// This is called by the GUI thread using stored SpectralData,
    /// not by the capture thread directly.
    pub fn extract_energy(data: &SpectralData, mode: FrequencyMode, target_freq: f32) -> f32 {
        match mode {
            FrequencyMode::Full => {
                // Weighted sum of all bands, emphasizing lower frequencies
                // (where most musical energy lives)
                let weights = [0.25, 0.25, 0.15, 0.12, 0.08, 0.06, 0.05, 0.04];
                let mut energy = 0.0f32;
                for (e, w) in data.band_energies.iter().zip(weights.iter()) {
                    energy += e * w;
                }
                energy / weights.iter().sum::<f32>()
            }
            FrequencyMode::LowPass => {
                // Sum energy from bands whose upper edge is below target
                let mut energy = 0.0f32;
                let mut count = 0.0f32;
                for (i, &e) in data.band_energies.iter().enumerate() {
                    if BAND_EDGES[i + 1] <= target_freq {
                        energy += e;
                        count += 1.0;
                    } else if BAND_EDGES[i] < target_freq {
                        // Partial contribution for the straddling band
                        let frac =
                            (target_freq - BAND_EDGES[i]) / (BAND_EDGES[i + 1] - BAND_EDGES[i]);
                        energy += e * frac;
                        count += frac;
                    }
                }
                if count > 0.0 {
                    energy / count
                } else {
                    0.0
                }
            }
            FrequencyMode::HighPass => {
                let mut energy = 0.0f32;
                let mut count = 0.0f32;
                for (i, &e) in data.band_energies.iter().enumerate() {
                    if BAND_EDGES[i] >= target_freq {
                        energy += e;
                        count += 1.0;
                    } else if BAND_EDGES[i + 1] > target_freq {
                        let frac =
                            (BAND_EDGES[i + 1] - target_freq) / (BAND_EDGES[i + 1] - BAND_EDGES[i]);
                        energy += e * frac;
                        count += frac;
                    }
                }
                if count > 0.0 {
                    energy / count
                } else {
                    0.0
                }
            }
            FrequencyMode::BandPass => {
                // Focus on the band(s) containing target_freq.
                // Wider Q = more bands included (not implemented here;
                // using fixed single-band for simplicity).
                let mut best_energy = 0.0f32;
                for (i, &e) in data.band_energies.iter().enumerate() {
                    if target_freq >= BAND_EDGES[i] && target_freq < BAND_EDGES[i + 1] {
                        best_energy = e;
                        break;
                    }
                }
                best_energy
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Gate — Threshold with hysteresis and auto-gate
// ---------------------------------------------------------------------------

/// Noise gate with hysteresis to prevent chattering, plus an auto-gate
/// mode that adapts the threshold based on a rolling energy histogram.
///
/// Ported from ChloeVibes' gate implementation.
pub struct Gate {
    /// Was the gate open last frame?
    was_open: bool,
    /// Smoothed gate signal (0.0 = closed, 1.0 = open)
    pub smoothed: f32,
    /// Rolling histogram of energy levels (100 bins, 0-100%)
    histogram: [f32; 100],
    /// Total samples in histogram
    histogram_samples: f32,
    /// Auto-calculated optimal threshold
    optimal_threshold: f32,
    /// Frame counter for periodic histogram recalculation
    frame_count: u32,
}

impl Gate {
    pub fn new() -> Self {
        Self {
            was_open: false,
            smoothed: 0.0,
            histogram: [0.0; 100],
            histogram_samples: 0.0,
            optimal_threshold: 0.2,
            frame_count: 0,
        }
    }

    /// Process one frame. Returns whether the gate is open.
    ///
    /// - `energy`: current audio energy level (0.0 - 1.0)
    /// - `manual_threshold`: user-set threshold (0.0 - 1.0)
    /// - `auto_gate_amount`: blend between manual and auto (0.0 = manual, 1.0 = auto)
    /// - `smoothing`: gate smoothing amount (0.0 = instant, 1.0 = very smooth)
    /// - `threshold_knee`: width of soft threshold region (0.0 = hard edge)
    pub fn process(
        &mut self,
        energy: f32,
        manual_threshold: f32,
        auto_gate_amount: f32,
        smoothing: f32,
        threshold_knee: f32,
    ) -> bool {
        // Auto-gate: maintain energy histogram and calculate optimal threshold
        if auto_gate_amount > 0.0 {
            let bin = (energy * 99.0).round() as usize;
            let bin = bin.min(99);
            self.histogram[bin] += 1.0;
            self.histogram_samples += 1.0;
            self.frame_count += 1;

            // Recalculate every ~86 frames (~2 seconds at 43Hz update rate)
            if self.frame_count >= 86 {
                self.frame_count = 0;

                // Find threshold that keeps gate open ~25% of the time.
                // Count from the top down to find where 25% of samples lie.
                let target_open_time = 0.25;
                let mut cumulative = 0.0f32;
                let mut optimal_bin = 99usize;

                for i in (0..100).rev() {
                    cumulative += self.histogram[i];
                    let percent_open = cumulative / self.histogram_samples.max(1.0);
                    if percent_open >= target_open_time {
                        optimal_bin = i;
                        break;
                    }
                }

                let calculated = optimal_bin as f32 / 100.0;
                // Smooth threshold changes to avoid jumps
                self.optimal_threshold = self.optimal_threshold * 0.7 + calculated * 0.3;

                // Decay histogram for rolling window effect
                for val in self.histogram.iter_mut() {
                    *val *= 0.5;
                }
                self.histogram_samples *= 0.5;
            }
        } else {
            // Reset histogram when auto-gate is off
            self.histogram = [0.0; 100];
            self.histogram_samples = 0.0;
            self.optimal_threshold = 0.2;
        }

        // Blend manual and auto thresholds
        let effective_threshold = lerp(manual_threshold, self.optimal_threshold, auto_gate_amount);

        // Soft-knee gate: open near the threshold, close lower to avoid chatter.
        // Larger knee widens the usable zone so threshold isn't "all-or-nothing".
        let knee = threshold_knee.clamp(0.0, 0.45);
        let open_threshold = (effective_threshold - 0.2 * knee).clamp(0.0, 1.0);
        let close_threshold = (effective_threshold - knee - 0.08 * effective_threshold).max(0.0);
        let is_above = if !self.was_open {
            energy > open_threshold
        } else {
            energy > close_threshold
        };

        // Smoothing: 0 = instant, 1 = very gradual.
        // Use exponential moving average where higher smoothing = slower response.
        let gate_signal = if is_above { 1.0 } else { 0.0 };
        if smoothing > 0.0 {
            // alpha near 1 = instant (low smoothing), alpha near 0 = sluggish (high smoothing)
            let alpha = 1.0 - smoothing.clamp(0.0, 0.98);
            self.smoothed = self.smoothed * (1.0 - alpha) + gate_signal * alpha;
        } else {
            self.smoothed = gate_signal;
        }

        let open = self.smoothed > 0.5;
        self.was_open = open;
        open
    }

    #[allow(dead_code)]
    pub fn was_open(&self) -> bool {
        self.was_open
    }

    pub fn effective_threshold(&self, manual: f32, auto_amount: f32) -> f32 {
        lerp(manual, self.optimal_threshold, auto_amount)
    }
}

// ---------------------------------------------------------------------------
// EnvelopeProcessor — ADSR with configurable curves
// ---------------------------------------------------------------------------

/// Full Attack-Decay-Sustain-Release envelope processor.
/// Transforms raw gate/trigger events into smooth, shaped output curves.
///
/// This is the biggest single upgrade over Chloe Vibes' linear decay.
/// Each stage has a configurable curve exponent:
///   - 1.0 = linear
///   - < 1.0 = fast start, slow finish (logarithmic feel)
///   - > 1.0 = slow start, fast finish (exponential feel)
pub struct EnvelopeProcessor {
    pub state: EnvelopeState,
    /// Current envelope value (0.0 - 1.0)
    pub value: f32,
    /// Value when current phase started
    phase_start_value: f32,
    /// Trigger magnitude (how hard the trigger was)
    magnitude: f32,
    /// Timestamp when current phase started (ms)
    start_time_ms: f32,
    /// Was the gate open last frame?
    last_gate_open: bool,
    /// Minimum time between retriggers (ms)
    min_retrigger_ms: f32,
    /// Time of last trigger (ms)
    last_trigger_time_ms: f32,
}

impl EnvelopeProcessor {
    pub fn new() -> Self {
        Self {
            state: EnvelopeState::Idle,
            value: 0.0,
            phase_start_value: 0.0,
            magnitude: 0.0,
            start_time_ms: 0.0,
            last_gate_open: false,
            min_retrigger_ms: 35.0,
            last_trigger_time_ms: 0.0,
        }
    }

    /// Trigger the envelope (gate just opened or strong onset detected).
    pub fn trigger(&mut self, magnitude: f32, current_time_ms: f32, velocity: f32) {
        // Enforce minimum retrigger interval
        if current_time_ms - self.last_trigger_time_ms < self.min_retrigger_ms {
            return;
        }

        let scaled_magnitude = magnitude * (0.5 + 0.5 * velocity);
        self.magnitude = scaled_magnitude.clamp(0.0, 1.5);
        self.state = EnvelopeState::Attack;
        self.start_time_ms = current_time_ms;
        self.phase_start_value = self.value; // Start from current value (retrigger)
        self.last_trigger_time_ms = current_time_ms;
    }

    /// Release the envelope (gate just closed).
    pub fn release(&mut self, current_time_ms: f32) {
        if self.state != EnvelopeState::Idle && self.state != EnvelopeState::Release {
            self.state = EnvelopeState::Release;
            self.start_time_ms = current_time_ms;
            self.phase_start_value = self.value;
        }
    }

    /// Update the sustain magnitude (for dynamic modes where energy
    /// changes while gate is held open).
    pub fn update_magnitude(&mut self, new_magnitude: f32) {
        if self.state == EnvelopeState::Sustain {
            // Asymmetric smoothing: fast rise (feel the hit), slower fall (natural decay).
            // 30% rise = punchy response to louder moments.
            // 15% fall = smooth enough to avoid jitter but responsive enough to feel dynamics.
            let alpha = if new_magnitude > self.magnitude {
                0.30
            } else {
                0.15
            };
            self.magnitude = self.magnitude * (1.0 - alpha) + new_magnitude * alpha;
        }
    }

    /// Process one frame of the envelope. Returns output value (0.0 - 1.0).
    ///
    /// Parameters are in milliseconds (attack, decay, release) and
    /// 0.0-1.0 (sustain level, curve exponents).
    pub fn process(
        &mut self,
        current_time_ms: f32,
        attack_ms: f32,
        decay_ms: f32,
        sustain_level: f32,
        release_ms: f32,
        attack_curve: f32,
        decay_curve: f32,
        release_curve: f32,
    ) -> f32 {
        let elapsed = current_time_ms - self.start_time_ms;

        match self.state {
            EnvelopeState::Attack => {
                if attack_ms <= 0.5 {
                    // Instant attack
                    self.value = 1.0;
                    self.state = EnvelopeState::Decay;
                    self.start_time_ms = current_time_ms;
                    self.phase_start_value = 1.0;
                } else {
                    let progress = (elapsed / attack_ms).clamp(0.0, 1.0);
                    let curved = apply_curve(progress, attack_curve);
                    self.value = self.phase_start_value + (1.0 - self.phase_start_value) * curved;

                    if progress >= 1.0 {
                        self.value = 1.0;
                        self.state = EnvelopeState::Decay;
                        self.start_time_ms = current_time_ms;
                        self.phase_start_value = 1.0;
                    }
                }
            }
            EnvelopeState::Decay => {
                if decay_ms <= 0.5 {
                    self.value = sustain_level;
                    self.state = EnvelopeState::Sustain;
                } else {
                    let progress = (elapsed / decay_ms).clamp(0.0, 1.0);
                    let decay_factor = apply_curve(1.0 - progress, decay_curve);
                    self.value =
                        sustain_level + (self.phase_start_value - sustain_level) * decay_factor;

                    if progress >= 1.0 {
                        self.value = sustain_level;
                        self.state = EnvelopeState::Sustain;
                    }
                }
            }
            EnvelopeState::Sustain => {
                // Multi-layer modulation to prevent neural adaptation.
                // Total variation ±25-35% keeps nerve endings sensitized.
                //   Primary: ~1.2Hz, ±20% (slow, deep oscillation)
                //   Secondary: ~0.3Hz, ±12% (breathing rhythm)
                //   Perlin-style noise: ±8% (irrational-ratio sines prevent pattern lock)
                let primary = 0.20 * (current_time_ms * 0.0075).sin();   // ~1.2Hz, ±20%
                let secondary = 0.12 * (current_time_ms * 0.0019).sin(); // ~0.3Hz, ±12%
                let noise = 0.08 * (
                    (current_time_ms * 0.00317).sin() * 0.5 +
                    (current_time_ms * 0.00713).sin() * 0.3 +
                    (current_time_ms * 0.01137).sin() * 0.2
                );
                let modulation = 1.0 + primary + secondary + noise;
                self.value = sustain_level * modulation;
            }
            EnvelopeState::Release => {
                if release_ms <= 0.5 {
                    self.value = 0.0;
                    self.state = EnvelopeState::Idle;
                    self.magnitude = 0.0;
                } else {
                    let progress = (elapsed / release_ms).clamp(0.0, 1.0);
                    let release_factor = apply_curve(1.0 - progress, release_curve);
                    self.value = self.phase_start_value * release_factor;

                    if self.value <= 0.001 || progress >= 1.0 {
                        self.value = 0.0;
                        self.state = EnvelopeState::Idle;
                        self.magnitude = 0.0;
                    }
                }
            }
            EnvelopeState::Idle => {
                self.value = (self.value * 0.95).max(0.0); // Gentle fade
                if self.value < 0.001 {
                    self.value = 0.0;
                }
                self.magnitude = 0.0;
            }
        }

        // Apply magnitude scaling
        (self.value * self.magnitude).clamp(0.0, 1.0)
    }

    /// Drive the envelope from gate state and onset detection.
    /// This is the main entry point called each frame from the GUI update.
    ///
    /// Returns the envelope output (0.0 - 1.0).
    pub fn drive(
        &mut self,
        gate_open: bool,
        energy: f32,
        is_onset: bool,
        onset_strength: f32,
        current_time_ms: f32,
        trigger_mode: TriggerMode,
        threshold: f32,
        threshold_knee: f32,
        dynamic_curve: f32,
        binary_level: f32,
        hybrid_blend: f32,
        attack_ms: f32,
        decay_ms: f32,
        sustain_level: f32,
        release_ms: f32,
        attack_curve: f32,
        decay_curve: f32,
        release_curve: f32,
    ) -> f32 {
        let dynamic_component = {
            let knee = threshold_knee.clamp(0.0, 0.45);
            let start = (threshold - knee).clamp(0.0, 1.0);
            let span = (1.0 - start).max(0.01);
            let normalized = ((energy - start) / span).clamp(0.0, 1.0);
            normalized.powf(dynamic_curve.clamp(0.35, 2.5))
        };

        // Calculate trigger magnitude based on mode
        let magnitude = match trigger_mode {
            TriggerMode::Dynamic => dynamic_component,
            TriggerMode::Binary => {
                if gate_open {
                    binary_level
                } else {
                    0.0
                }
            }
            TriggerMode::Hybrid => {
                dynamic_component * (1.0 - hybrid_blend)
                    + if gate_open {
                        binary_level * hybrid_blend
                    } else {
                        0.0
                    }
            }
        };

        // Gate edge detection
        let gate_just_opened = gate_open && !self.last_gate_open;
        let gate_just_closed = !gate_open && self.last_gate_open;

        // Onset retrigger: retrigger on onsets above a moderate threshold.
        // 1.05x catches most real beats; the retrigger cooldown (35ms) prevents flutter.
        let is_onset_trigger =
            is_onset && onset_strength > 1.05 && gate_open && self.state == EnvelopeState::Sustain;

        // Trigger logic
        if gate_just_opened || is_onset_trigger {
            let velocity = if is_onset_trigger {
                onset_strength.min(1.35)
            } else {
                1.0
            };
            self.trigger(magnitude.max(0.03), current_time_ms, velocity);
        } else if gate_open && self.state == EnvelopeState::Idle {
            // Gate open but envelope idle — retrigger
            self.trigger(magnitude.max(0.03), current_time_ms, 1.0);
        } else if gate_just_closed {
            self.release(current_time_ms);
        }

        // Update magnitude during sustain for dynamic/hybrid modes
        if gate_open
            && self.state == EnvelopeState::Sustain
            && matches!(trigger_mode, TriggerMode::Dynamic | TriggerMode::Hybrid)
        {
            self.update_magnitude(magnitude);
        }

        self.last_gate_open = gate_open;

        // Process the envelope state machine
        self.process(
            current_time_ms,
            attack_ms,
            decay_ms,
            sustain_level,
            release_ms,
            attack_curve,
            decay_curve,
            release_curve,
        )
    }

    pub fn reset(&mut self) {
        self.state = EnvelopeState::Idle;
        self.value = 0.0;
        self.magnitude = 0.0;
    }
}

// ---------------------------------------------------------------------------
// BeatDetector — Onset detection via spectral flux
// ---------------------------------------------------------------------------

/// Simple onset/beat detector using adaptive thresholding on spectral flux.
/// Not as sophisticated as ChloeVibes' tempo estimation, but catches
/// transients reliably for retrigger purposes.
pub struct BeatDetector {
    /// Rolling history of spectral flux values
    flux_history: Vec<f32>,
    history_index: usize,
    /// Adaptive threshold multiplier
    adaptive_threshold: f32,
    /// Cooldown timestamp
    last_onset_time_ms: f32,
    /// Minimum time between detected onsets (ms)
    cooldown_ms: f32,
}

impl BeatDetector {
    pub fn new() -> Self {
        Self {
            flux_history: vec![0.0; 43], // ~1 second at 43Hz
            history_index: 0,
            adaptive_threshold: 0.55,
            last_onset_time_ms: 0.0,
            cooldown_ms: 55.0, // 55ms ≈ 18 onsets/sec max (≈270 BPM 16th notes)
        }
    }

    /// Process spectral flux and detect onsets.
    /// Returns (is_onset, onset_strength).
    pub fn process(&mut self, spectral_flux: f32, current_time_ms: f32) -> (bool, f32) {
        // Update history
        self.flux_history[self.history_index] = spectral_flux;
        self.history_index = (self.history_index + 1) % self.flux_history.len();

        // Calculate local statistics
        let mean: f32 = self.flux_history.iter().sum::<f32>() / self.flux_history.len() as f32;
        let variance: f32 = self
            .flux_history
            .iter()
            .map(|&v| (v - mean).powi(2))
            .sum::<f32>()
            / self.flux_history.len() as f32;
        let std_dev = variance.sqrt();

        // Adaptive threshold — lower baseline means catching more real beats.
        let threshold = mean + self.adaptive_threshold * std_dev;

        // Detect onset
        let is_onset = spectral_flux > threshold
            && (current_time_ms - self.last_onset_time_ms) > self.cooldown_ms;

        if is_onset {
            self.last_onset_time_ms = current_time_ms;
            // Moderate growth after onset (prevents rapid double-triggering)
            self.adaptive_threshold = (self.adaptive_threshold * 1.06).min(1.8);
        } else {
            // Faster decay back to baseline — recover sensitivity between beats.
            self.adaptive_threshold = (self.adaptive_threshold * 0.985).max(0.12);
        }

        let strength = if threshold > 0.0 {
            spectral_flux / threshold
        } else {
            0.0
        };

        (is_onset, strength)
    }
}

// ---------------------------------------------------------------------------
// ClimaxEngine - Time-domain escalation with tease/surge cycle
// ---------------------------------------------------------------------------

/// Adds a slow time-based "build -> tease -> surge" layer on top of the
/// audio-reactive envelope output.
///
/// The goal is to keep stimulation dynamic over longer sessions by:
/// - slowly raising effective intensity over a cycle,
/// - introducing a controlled dip near the end (tease),
/// - then surging back up with faster micro-pulses.
pub struct ClimaxEngine {
    cycle_anchor_ms: f32,
    last_time_ms: f32,
    micro_phase: f32,
    micro_phase2: f32,
    micro_phase3: f32,
    onset_boost: f32,
    // Edge tracking — forces intensity dips to prevent plateau adaptation
    high_output_ms: f32,
    deny_active: bool,
    deny_start_ms: f32,
    deny_duration_ms: f32,
}

impl ClimaxEngine {
    pub fn new() -> Self {
        Self {
            cycle_anchor_ms: 0.0,
            last_time_ms: 0.0,
            micro_phase: 0.0,
            micro_phase2: 0.0,
            micro_phase3: 0.0,
            onset_boost: 0.0,
            high_output_ms: 0.0,
            deny_active: false,
            deny_start_ms: 0.0,
            deny_duration_ms: 0.0,
        }
    }

    pub fn reset(&mut self, current_time_ms: f32) {
        self.cycle_anchor_ms = current_time_ms;
        self.last_time_ms = current_time_ms;
        self.micro_phase = 0.0;
        self.micro_phase2 = 0.0;
        self.micro_phase3 = 0.0;
        self.onset_boost = 0.0;
        self.high_output_ms = 0.0;
        self.deny_active = false;
        self.deny_start_ms = 0.0;
        self.deny_duration_ms = 0.0;
    }

    /// Returns current cycle progress in [0, 1).
    pub fn phase_progress(&self, current_time_ms: f32, build_up_ms: f32) -> f32 {
        let cycle_len = build_up_ms.clamp(8_000.0, 240_000.0);
        if cycle_len <= 0.0 {
            return 0.0;
        }
        ((current_time_ms - self.cycle_anchor_ms) / cycle_len)
            .fract()
            .max(0.0)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        input: f32,
        energy: f32,
        gate_open: bool,
        is_onset: bool,
        onset_strength: f32,
        current_time_ms: f32,
        enabled: bool,
        intensity: f32,
        build_up_ms: f32,
        tease_ratio: f32,
        tease_drop: f32,
        surge_boost: f32,
        pulse_depth: f32,
        pattern: ClimaxPattern,
    ) -> f32 {
        let dry = input.clamp(0.0, 1.0);
        if !enabled {
            self.reset(current_time_ms);
            return dry;
        }

        if self.last_time_ms <= 0.0 {
            self.reset(current_time_ms);
        }

        let cycle_len = build_up_ms.clamp(8_000.0, 240_000.0);
        let dt = ((current_time_ms - self.last_time_ms) * 0.001).clamp(0.0, 0.2);
        self.last_time_ms = current_time_ms;

        if current_time_ms - self.cycle_anchor_ms >= cycle_len {
            let cycles = ((current_time_ms - self.cycle_anchor_ms) / cycle_len)
                .floor()
                .max(1.0);
            self.cycle_anchor_ms += cycles * cycle_len;
        }

        let progress = ((current_time_ms - self.cycle_anchor_ms) / cycle_len).clamp(0.0, 1.0);
        let intensity = intensity.clamp(0.0, 1.0);

        let ramp = match pattern {
            ClimaxPattern::Wave => smooth_step(progress),
            ClimaxPattern::Stairs => {
                let steps = 6.0;
                ((progress * steps).floor() / steps).clamp(0.0, 1.0)
            }
            ClimaxPattern::Surge => progress.powf(0.6),
        };

        let tease_start = 1.0 - tease_ratio.clamp(0.05, 0.5);
        let tease_factor = if progress >= tease_start {
            let t = ((progress - tease_start) / (1.0 - tease_start)).clamp(0.0, 1.0);
            let envelope = 1.0 - (2.0 * t - 1.0).abs();
            1.0 - tease_drop.clamp(0.0, 0.9) * envelope
        } else {
            1.0
        };

        let surge_factor = if progress >= 0.84 {
            let t = ((progress - 0.84) / 0.16).clamp(0.0, 1.0);
            // Steeper power curve (0.3 vs 0.5) — hits harder at the end
            1.0 + surge_boost.clamp(0.0, 1.2) * t.powf(0.3)
        } else {
            1.0
        };

        if is_onset && gate_open {
            self.onset_boost = (self.onset_boost + 0.08 * onset_strength.clamp(0.0, 2.0)).min(0.30);
        }
        self.onset_boost = (self.onset_boost - dt * 0.9).max(0.0);

        let pulse_depth = pulse_depth.clamp(0.0, 0.45);
        // During surge phase, allow up to 8Hz micro-pulse; otherwise cap at 6Hz.
        let max_pulse_hz = if progress >= 0.84 { 8.0 } else { 6.0 };
        let pulse_rate_hz = (2.0 + intensity * 2.0 + energy * 1.5 + ramp * 0.5).min(max_pulse_hz);
        // Triple-oscillator detuned micro-pulse — prevents single-frequency adaptation
        let detune = 0.07; // ±7% frequency spread
        self.micro_phase = (self.micro_phase + dt * pulse_rate_hz * TAU).rem_euclid(TAU);
        self.micro_phase2 = (self.micro_phase2 + dt * pulse_rate_hz * (1.0 + detune) * TAU).rem_euclid(TAU);
        self.micro_phase3 = (self.micro_phase3 + dt * pulse_rate_hz * (1.0 - detune) * TAU).rem_euclid(TAU);
        let pulse_raw = 0.5 * self.micro_phase.sin()
            + 0.3 * self.micro_phase2.sin()
            + 0.2 * self.micro_phase3.sin();
        let pulse = 1.0 - pulse_depth + pulse_depth * (0.5 + 0.5 * pulse_raw);

        // Arousal gain: never attenuate below dry signal. Build UP from the audio-reactive base.
        // At ramp=0 (cycle start): gain = 1.0 (passthrough).
        // At ramp=1 (cycle peak):  gain = up to 1.85 (amplified).
        let arousal_gain = (1.0 + 0.85 * ramp) * (1.0 + intensity * 0.20);
        let gated_boost = if gate_open { self.onset_boost } else { 0.0 };

        let raw_output = (dry * arousal_gain * tease_factor * surge_factor * pulse + gated_boost).clamp(0.0, 1.0);

        // Edge-and-deny: when output has been >0.8 for >3 seconds, force a dip
        // to 60% for 2-4 seconds, then surge back. Prevents plateau adaptation.
        if raw_output > 0.8 {
            self.high_output_ms += dt * 1000.0;
        } else {
            self.high_output_ms = (self.high_output_ms - dt * 500.0).max(0.0);
        }

        if !self.deny_active && self.high_output_ms > 3000.0 {
            self.deny_active = true;
            self.deny_start_ms = current_time_ms;
            // Randomized deny duration: 2000-4000ms using cheap pseudo-random
            self.deny_duration_ms = 2000.0 + 2000.0 * (0.5 + 0.5 * (current_time_ms * 0.00137).sin());
            self.high_output_ms = 0.0;
        }

        if self.deny_active {
            let deny_elapsed = current_time_ms - self.deny_start_ms;
            if deny_elapsed >= self.deny_duration_ms {
                self.deny_active = false;
            } else {
                // Smooth envelope: fade down then back up
                let deny_t = deny_elapsed / self.deny_duration_ms;
                // Parabolic dip: peaks at center of deny window
                let deny_depth = 0.40; // 40% reduction at deepest point
                let deny_envelope = deny_depth * (1.0 - (2.0 * deny_t - 1.0).powi(2));
                return (raw_output * (1.0 - deny_envelope)).clamp(0.0, 1.0);
            }
        }

        raw_output
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Linear interpolation between two values.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Apply a power curve to a value in [0, 1].
/// exponent 1.0 = linear, < 1.0 = logarithmic, > 1.0 = exponential.
fn apply_curve(value: f32, exponent: f32) -> f32 {
    value.clamp(0.0, 1.0).powf(exponent)
}

fn smooth_step(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
