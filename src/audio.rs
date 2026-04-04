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

use rustfft::{num_complex::Complex, Fft, FftPlanner};

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
#[derive(Default)]
pub enum TriggerMode {
    /// Intensity scales continuously with audio energy above threshold.
    /// The "normal" mode — louder = stronger vibration.
    #[default]
    Dynamic,
    /// Fixed output level when energy exceeds threshold, zero otherwise.
    /// Great for rhythmic on/off pulsing.
    Binary,
    /// Blend between dynamic and binary. Adjustable via hybrid_blend.
    Hybrid,
}


/// Which part of the frequency spectrum to analyze.
/// Lets you isolate bass hits, vocal range, etc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[derive(Default)]
pub enum FrequencyMode {
    /// Analyze the full audible spectrum (weighted toward lower freqs).
    #[default]
    Full,
    /// Only frequencies below target_frequency.
    LowPass,
    /// Only frequencies above target_frequency.
    HighPass,
    /// Narrow band around target_frequency with adjustable Q.
    BandPass,
}


/// High-level modulation pattern for the climax engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[derive(Default)]
pub enum ClimaxPattern {
    /// Smooth continuous rise toward the end of the cycle.
    #[default]
    Wave,
    /// Step-like intensity increases (plateaus then jumps).
    Stairs,
    /// Aggressive exponential ramp in the final third.
    Surge,
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
    /// Overall RMS power of the audio signal (reserved, currently unused)
    pub rms_power: f32,
    /// Spectral centroid in Hz — higher = brighter sound
    pub spectral_centroid: f32,
    /// Spectral flux — how much the spectrum changed since last frame.
    /// Spikes on transients (drum hits, note onsets).
    pub spectral_flux: f32,
    /// Frequency of the loudest bin (reserved, currently unused)
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
    fft_plan: std::sync::Arc<dyn Fft<f32>>,
    /// Hann window function — reduces spectral leakage
    window: Vec<f32>,
    /// Previous frame's magnitude spectrum (for flux calculation)
    prev_magnitude: Vec<f32>,
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
        let fft_plan = planner.plan_fft_forward(FFT_SIZE);
        let scratch_len = fft_plan.get_inplace_scratch_len();

        Self {
            fft_plan,
            window,
            prev_magnitude: vec![0.0; FFT_SIZE / 2],
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
        let start = mono_len.saturating_sub(FFT_SIZE);
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
        self.fft_plan
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

        // Save magnitude spectrum for next frame's flux calculation
        self.prev_magnitude.copy_from_slice(magnitudes);

        SpectralData {
            band_energies,
            rms_power: 0.0,
            spectral_centroid,
            spectral_flux,
            dominant_frequency: 0.0,
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

        // Proportional hysteresis: close threshold is slightly below open
        // threshold to prevent chattering. Gap scales with the threshold itself
        // so low thresholds get a tight band, high thresholds a wider one.
        // Matches Android Gate.kt behavior.
        let hysteresis = (effective_threshold * 0.25).clamp(0.005, 0.08);
        let open_threshold = effective_threshold;
        let close_threshold = (effective_threshold - hysteresis).max(0.0);
        let is_above = if !self.was_open {
            energy > open_threshold
        } else {
            energy > close_threshold
        };

        // Asymmetric smoothing: instant open, smooth close.
        // Opening speed is critical for transient response — every ms
        // of gate delay eats the attack phase. Closing smoothness
        // prevents chatter without impacting onset timing.
        let gate_signal = if is_above { 1.0 } else { 0.0 };
        if smoothing > 0.0 {
            let alpha = if gate_signal > self.smoothed {
                1.0 // Instant open — don't filter the rising edge
            } else {
                1.0 - smoothing.clamp(0.0, 0.98) // Smooth close
            };
            self.smoothed = self.smoothed * (1.0 - alpha) + gate_signal * alpha;
        } else {
            self.smoothed = gate_signal;
        }

        let open = self.smoothed > 0.5;
        self.was_open = open;
        open
    }

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
    /// Attack target — normally 1.0, up to 1.2 with velocity overshoot
    attack_target: f32,
    /// Timestamp when current phase started (ms)
    start_time_ms: f32,
    /// Was the gate open last frame?
    last_gate_open: bool,
    /// Minimum time between retriggers (ms). 20ms matches motor spin-up.
    min_retrigger_ms: f32,
    /// Time of last trigger (ms)
    last_trigger_time_ms: f32,
    /// Stochastic micro-pause: next pause timestamp (ms); 0 = not initialized
    next_micro_pause_ms: f32,
    /// Remaining micro-pause frames (0 = not pausing)
    micro_pause_frames: i32,
}

impl EnvelopeProcessor {
    pub fn new() -> Self {
        Self {
            state: EnvelopeState::Idle,
            value: 0.0,
            phase_start_value: 0.0,
            magnitude: 0.0,
            attack_target: 1.0,
            start_time_ms: 0.0,
            last_gate_open: false,
            min_retrigger_ms: 20.0,
            last_trigger_time_ms: 0.0,
            next_micro_pause_ms: 0.0,
            micro_pause_frames: 0,
        }
    }

    /// Trigger the envelope (gate just opened or strong onset detected).
    pub fn trigger(&mut self, magnitude: f32, current_time_ms: f32, velocity: f32, attack_ms: f32) {
        // Enforce minimum retrigger interval
        if current_time_ms - self.last_trigger_time_ms < self.min_retrigger_ms {
            return;
        }

        let scaled_magnitude = magnitude * (0.5 + 0.5 * velocity);
        self.magnitude = scaled_magnitude.clamp(0.0, 1.5);

        // Velocity overshoot: strong onsets briefly exceed normal peak.
        // A hard drum hit should momentarily push past the normal ceiling,
        // creating a visceral "punch" sensation before decaying to sustain.
        self.attack_target = if velocity > 1.0 {
            (1.0 + 0.15 * (velocity - 1.0)).min(1.2)
        } else {
            1.0
        };

        // For short attacks (< 50ms), skip directly to Decay at peak.
        // Motor spin-up (~20ms) provides the physical ramp — sending peak
        // immediately ensures the BLE command carries the full transient.
        if attack_ms < 50.0 {
            self.state = EnvelopeState::Decay;
            self.value = self.attack_target;
            self.phase_start_value = self.attack_target;
            self.start_time_ms = current_time_ms;
        } else {
            self.state = EnvelopeState::Attack;
            self.start_time_ms = current_time_ms;
            self.phase_start_value = self.value.max(0.4);
        }

        // Reset micro-pause on retrigger
        self.micro_pause_frames = 0;
        self.next_micro_pause_ms = 0.0;
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
                    // Instant attack — jump to peak (with velocity overshoot if any)
                    self.value = self.attack_target;
                    self.state = EnvelopeState::Decay;
                    self.start_time_ms = current_time_ms;
                    self.phase_start_value = self.attack_target;
                } else {
                    let progress = (elapsed / attack_ms).clamp(0.0, 1.0);
                    let curved = apply_curve(progress, attack_curve);
                    self.value = self.phase_start_value + (self.attack_target - self.phase_start_value) * curved;

                    if progress >= 1.0 {
                        self.value = self.attack_target;
                        self.state = EnvelopeState::Decay;
                        self.start_time_ms = current_time_ms;
                        self.phase_start_value = self.attack_target;
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
                // Stochastic micro-pauses: drops to true zero for 3-6 frames (48-96ms).
                // Long enough for the motor to actually stop, creating a real nerve
                // reset. Intermittent stimulation maintains sensitivity far longer
                // than continuous vibration. True zero (not 5%) ensures the motor
                // fully decelerates — partial intensity keeps nerves in the adapted state.
                if self.micro_pause_frames > 0 {
                    self.micro_pause_frames -= 1;
                    self.value = 0.0; // True zero — motor must stop
                } else if self.next_micro_pause_ms > 0.0 && current_time_ms >= self.next_micro_pause_ms {
                    // 3-6 frames at 60Hz = 48-96ms (motor needs ~20ms to stop)
                    self.micro_pause_frames = 3 + ((current_time_ms * 7.13) as i32 & 0x3);
                    // Next pause in 2-8 seconds (deterministic pseudo-random)
                    let pseudo_rand = ((current_time_ms * 13.37) as u32 & 0xFFFF) as f32 / 65535.0;
                    self.next_micro_pause_ms = current_time_ms + 2000.0 + pseudo_rand * 6000.0;
                    self.value = 0.0;
                } else {
                    // Initialize micro-pause timer on first sustain frame
                    if self.next_micro_pause_ms <= 0.0 {
                        let pseudo_rand = ((current_time_ms * 13.37) as u32 & 0xFFFF) as f32 / 65535.0;
                        self.next_micro_pause_ms = current_time_ms + 2000.0 + pseudo_rand * 6000.0;
                    }

                    // 5-layer modulation to prevent neural adaptation.
                    // Irrational-ratio frequencies ensure the combined waveform never
                    // exactly repeats, keeping nerve endings from filtering the stimulus.
                    let primary    = 0.22 * (current_time_ms * 0.0075).sin();    // ~1.2Hz
                    let secondary  = 0.14 * (current_time_ms * 0.0019).sin();    // ~0.3Hz
                    let tertiary   = 0.10 * (current_time_ms * 0.01696).sin();   // ~2.7Hz
                    let cross_freq = 0.08 * (current_time_ms * 0.001068).sin();  // ~0.17Hz
                    let noise      = 0.10 * (
                        (current_time_ms * 0.00317).sin() * 0.30
                        + (current_time_ms * 0.00713).sin() * 0.25
                        + (current_time_ms * 0.01137).sin() * 0.20
                        + (current_time_ms * 0.02173).sin() * 0.15
                        + (current_time_ms * 0.00491).sin() * 0.10
                    );
                    let modulation = 1.0 + primary + secondary + tertiary + cross_freq + noise;
                    self.value = (sustain_level * modulation).clamp(0.0, 1.0);
                }
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
    #[allow(clippy::too_many_arguments)]
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
        spectral_centroid: f32,
    ) -> f32 {
        // Frequency-dependent envelope shaping: bass = deep sustained pressure,
        // treble = sharp surface tingling. Spectral centroid tells us whether the
        // current sound is bass-heavy or bright.
        let centroid_norm = ((spectral_centroid - 100.0) / 4000.0).clamp(0.0, 1.0);
        // Bass: hold longer (continuous pressure). Treble: release faster (tap).
        let adj_sustain_level = sustain_level * (1.0 - 0.25 * centroid_norm);
        let adj_release_ms = release_ms * (1.0 + 0.4 * (1.0 - centroid_norm));

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
            self.trigger(magnitude.max(0.03), current_time_ms, velocity, attack_ms);
        } else if gate_open && self.state == EnvelopeState::Idle {
            // Gate open but envelope idle — retrigger
            self.trigger(magnitude.max(0.03), current_time_ms, 1.0, attack_ms);
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

        // Process the envelope state machine (using frequency-adjusted sustain/release)
        self.process(
            current_time_ms,
            attack_ms,
            decay_ms,
            adj_sustain_level,
            adj_release_ms,
            attack_curve,
            decay_curve,
            release_curve,
        )
    }

    pub fn reset(&mut self) {
        self.state = EnvelopeState::Idle;
        self.value = 0.0;
        self.magnitude = 0.0;
        self.attack_target = 1.0;
        self.micro_pause_frames = 0;
        self.next_micro_pause_ms = 0.0;
    }
}

// ---------------------------------------------------------------------------
// BeatDetector — Onset detection via spectral flux
// ---------------------------------------------------------------------------

/// Onset/beat detector using adaptive thresholding on spectral flux,
/// with tempo tracking and predictive onset for latency compensation.
/// Ported from Android BeatDetector.kt.
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
    /// Recent onset strength for velocity tracking
    recent_onset_strength: f32,
    // Tempo tracking for predictive onset
    onset_timestamps: [f32; 16],
    onset_ts_index: usize,
    onset_ts_count: usize,
    /// Estimated inter-onset interval in ms (0 = no estimate).
    pub tempo_interval_ms: f32,
    /// Confidence in tempo estimate (0.0 = none, 1.0 = locked).
    pub tempo_confidence: f32,
    /// Predicted time of next onset in ms (0 = no prediction).
    pub predicted_next_onset_ms: f32,
}

impl BeatDetector {
    pub fn new() -> Self {
        Self {
            flux_history: vec![0.0; 43], // ~1 second at 43Hz
            history_index: 0,
            adaptive_threshold: 0.55,
            last_onset_time_ms: 0.0,
            cooldown_ms: 55.0, // 55ms ≈ 18 onsets/sec max (≈270 BPM 16th notes)
            recent_onset_strength: 0.0,
            onset_timestamps: [0.0; 16],
            onset_ts_index: 0,
            onset_ts_count: 0,
            tempo_interval_ms: 0.0,
            tempo_confidence: 0.0,
            predicted_next_onset_ms: 0.0,
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

            // Track onset velocity
            let raw_strength = if threshold > 0.0 {
                spectral_flux / threshold
            } else {
                0.0
            };
            self.recent_onset_strength =
                self.recent_onset_strength * 0.3 + raw_strength * 0.7;

            // Record timestamp for tempo tracking
            self.onset_timestamps[self.onset_ts_index] = current_time_ms;
            self.onset_ts_index = (self.onset_ts_index + 1) % self.onset_timestamps.len();
            if self.onset_ts_count < self.onset_timestamps.len() {
                self.onset_ts_count += 1;
            }

            // Update tempo prediction after accumulating enough onsets
            if self.onset_ts_count >= 4 {
                self.update_tempo_prediction(current_time_ms);
            }
        } else {
            // Faster decay back to baseline — recover sensitivity between beats.
            self.adaptive_threshold = (self.adaptive_threshold * 0.985).max(0.12);
            self.recent_onset_strength *= 0.98;
        }

        let strength = if threshold > 0.0 {
            spectral_flux / threshold
        } else {
            0.0
        };

        (is_onset, strength)
    }

    fn update_tempo_prediction(&mut self, current_time_ms: f32) {
        // Collect inter-onset intervals from recent timestamps
        let mut intervals = [0.0f32; 15];
        let mut count = 0usize;
        for i in 1..self.onset_ts_count {
            let len = self.onset_timestamps.len();
            let curr =
                self.onset_timestamps[(self.onset_ts_index + len - i) % len];
            let prev =
                self.onset_timestamps[(self.onset_ts_index + len - i - 1) % len];
            let interval = curr - prev;
            if (150.0..=2000.0).contains(&interval) {
                // 30-400 BPM range
                intervals[count] = interval;
                count += 1;
                if count >= intervals.len() {
                    break;
                }
            }
        }

        if count < 3 {
            self.tempo_confidence = 0.0;
            self.predicted_next_onset_ms = 0.0;
            return;
        }

        let mean: f32 = intervals[..count].iter().sum::<f32>() / count as f32;
        let variance: f32 = intervals[..count]
            .iter()
            .map(|&v| (v - mean).powi(2))
            .sum::<f32>()
            / count as f32;
        let std_dev = variance.sqrt();

        // Confidence: low coefficient of variation = high confidence
        let cv = if mean > 0.0 { std_dev / mean } else { 1.0 };
        self.tempo_confidence = (1.0 - cv * 4.0).clamp(0.0, 1.0);
        self.tempo_interval_ms = mean;

        if self.tempo_confidence > 0.5 {
            let len = self.onset_timestamps.len();
            let last_onset =
                self.onset_timestamps[(self.onset_ts_index + len - 1) % len];
            let elapsed = current_time_ms - last_onset;
            let intervals_elapsed = (elapsed / mean) as u32;
            self.predicted_next_onset_ms =
                last_onset + (intervals_elapsed + 1) as f32 * mean;
        } else {
            self.predicted_next_onset_ms = 0.0;
        }
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
    // 5-oscillator detuned micro-pulse (prevents single-freq adaptation)
    micro_phase: f32,
    micro_phase2: f32,
    micro_phase3: f32,
    micro_phase4: f32,
    micro_phase5: f32,
    onset_boost: f32,
    // Arousal momentum — tracks cumulative stimulation across cycles
    arousal_momentum: f32,
    cycle_count: u32,
    // Sub-harmonic resonance: low-freq flutter coupled to device mechanics
    sub_harmonic_phase: f32,
    // Chaos oscillator state (Lorenz attractor)
    chaos_x: f32,
    chaos_y: f32,
    chaos_z: f32,
    // Dual-motor phasing: independent signal for second motor
    motor2_phase: f32,
    /// Secondary motor output (0.0 - 1.0) for dual-motor devices.
    pub motor2_output: f32,
    // Edge tracking — forces intensity dips to prevent plateau adaptation
    high_output_ms: f32,
    deny_active: bool,
    deny_start_ms: f32,
    deny_duration_ms: f32,
    // Breathing-rate modulation: couples to involuntary arousal breathing
    breathing_phase: f32,
}

impl ClimaxEngine {
    pub fn new() -> Self {
        Self {
            cycle_anchor_ms: 0.0,
            last_time_ms: 0.0,
            micro_phase: 0.0,
            micro_phase2: 0.0,
            micro_phase3: 0.0,
            micro_phase4: 0.0,
            micro_phase5: 0.0,
            onset_boost: 0.0,
            arousal_momentum: 0.0,
            cycle_count: 0,
            sub_harmonic_phase: 0.0,
            chaos_x: 0.1,
            chaos_y: 0.0,
            chaos_z: 0.0,
            motor2_phase: 0.0,
            motor2_output: 0.0,
            high_output_ms: 0.0,
            deny_active: false,
            deny_start_ms: 0.0,
            deny_duration_ms: 0.0,
            breathing_phase: 0.0,
        }
    }

    pub fn reset(&mut self, current_time_ms: f32) {
        self.cycle_anchor_ms = current_time_ms;
        self.last_time_ms = current_time_ms;
        self.micro_phase = 0.0;
        self.micro_phase2 = 0.0;
        self.micro_phase3 = 0.0;
        self.micro_phase4 = 0.0;
        self.micro_phase5 = 0.0;
        self.onset_boost = 0.0;
        self.arousal_momentum = 0.0;
        self.cycle_count = 0;
        self.sub_harmonic_phase = 0.0;
        self.chaos_x = 0.1;
        self.chaos_y = 0.0;
        self.chaos_z = 0.0;
        self.motor2_phase = 0.0;
        self.motor2_output = 0.0;
        self.high_output_ms = 0.0;
        self.deny_active = false;
        self.deny_start_ms = 0.0;
        self.deny_duration_ms = 0.0;
        self.breathing_phase = 0.0;
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

        // Wrap cycle and track arousal momentum across completed cycles.
        // Momentum grows faster than it decays — sessions escalate over time,
        // compensating for neural desensitization from prolonged stimulation.
        if current_time_ms - self.cycle_anchor_ms >= cycle_len {
            let cycles = ((current_time_ms - self.cycle_anchor_ms) / cycle_len)
                .floor()
                .max(1.0);
            self.cycle_anchor_ms += cycles * cycle_len;
            self.cycle_count = self.cycle_count.saturating_add(1);
            self.arousal_momentum = (self.arousal_momentum + 0.12).min(0.75);
        }
        // Slow momentum decay during silence
        if !gate_open {
            self.arousal_momentum = (self.arousal_momentum - dt * 0.008).max(0.0);
        }

        let progress = ((current_time_ms - self.cycle_anchor_ms) / cycle_len).clamp(0.0, 1.0);
        let intensity = intensity.clamp(0.0, 1.0);
        let cycle_maturity = (self.cycle_count as f32 / 6.0).min(1.0); // 0→1 over first 6 cycles

        let ramp = match pattern {
            ClimaxPattern::Wave => smooth_step(progress),
            ClimaxPattern::Stairs => {
                let steps = 6.0;
                ((progress * steps).floor() / steps).clamp(0.0, 1.0)
            }
            ClimaxPattern::Surge => progress.powf(0.6),
        };

        // ---- Tease factor: asymmetric dip near end of cycle ----
        // Fast cliff down → hold at floor → slow sensual rebuild.
        // The sharp drop triggers a gasp reflex; the slow return builds
        // aching anticipation. Tease depth escalates across cycles —
        // first tease is gentle, later ones are devastating.
        let tease_start = 1.0 - tease_ratio.clamp(0.05, 0.5);
        let tease_factor = if progress >= tease_start {
            let t = ((progress - tease_start) / (1.0 - tease_start)).clamp(0.0, 1.0);
            let escalating_drop = tease_drop.clamp(0.0, 0.9) * (0.6 + 0.4 * cycle_maturity);
            let envelope = if t < 0.10 {
                // Sharp cliff down (first 10% of tease window)
                smooth_step(t / 0.10)
            } else if t < 0.55 {
                // Hold at floor — nerve endings reset, anticipation builds
                1.0
            } else {
                // Slow curved rebuild (last 45%) — agonizingly gradual
                let rebuild_t = (t - 0.55) / 0.45;
                1.0 - smooth_step(rebuild_t) * smooth_step(rebuild_t)
            };
            1.0 - escalating_drop * envelope
        } else {
            1.0
        };

        // ---- Surge factor: accelerating curve (slow build → explosive finish) ----
        // smooth_step² starts almost flat, then rockets upward in the final moments.
        // At t=0.25: 0.03 (barely perceptible). At t=0.75: 0.74 (building fast).
        // At t=0.95: 0.98 (slamming into peak). This is the opposite of the old
        // powf(0.2) which hit 87% immediately and then plateaued.
        let surge_start = 0.80;
        let surge_factor = if progress >= surge_start {
            let t = ((progress - surge_start) / (1.0 - surge_start)).clamp(0.0, 1.0);
            let ss = smooth_step(t);
            1.0 + surge_boost.clamp(0.0, 1.5) * ss * ss
        } else {
            1.0
        };

        // ---- Onset boost: scales with cycle progression ----
        // A drum hit during surge should feel like being pushed over the edge.
        // Early in cycle: modest bump. During surge: devastating impact.
        if is_onset && gate_open {
            let onset_scale = 0.14 + 0.22 * ramp; // 0.14 → 0.36 across cycle
            self.onset_boost =
                (self.onset_boost + onset_scale * onset_strength.clamp(0.0, 2.5)).min(0.60);
        }
        self.onset_boost = (self.onset_boost - dt * 0.7).max(0.0);

        // ---- 5-oscillator detuned micro-pulse ----
        let pulse_depth = pulse_depth.clamp(0.0, 0.55);
        let max_pulse_hz = if progress >= surge_start { 10.0 } else { 7.0 };
        let pulse_rate_hz =
            (2.0 + intensity * 3.0 + energy * 2.0 + ramp * 1.0).min(max_pulse_hz);
        let detune1 = 0.07;
        let detune2 = 0.13;
        self.micro_phase = (self.micro_phase + dt * pulse_rate_hz * TAU).rem_euclid(TAU);
        self.micro_phase2 =
            (self.micro_phase2 + dt * pulse_rate_hz * (1.0 + detune1) * TAU).rem_euclid(TAU);
        self.micro_phase3 =
            (self.micro_phase3 + dt * pulse_rate_hz * (1.0 - detune1) * TAU).rem_euclid(TAU);
        self.micro_phase4 =
            (self.micro_phase4 + dt * pulse_rate_hz * (1.0 + detune2) * TAU).rem_euclid(TAU);
        self.micro_phase5 =
            (self.micro_phase5 + dt * pulse_rate_hz * (1.0 - detune2) * TAU).rem_euclid(TAU);
        let pulse_raw = 0.35 * self.micro_phase.sin()
            + 0.22 * self.micro_phase2.sin()
            + 0.22 * self.micro_phase3.sin()
            + 0.11 * self.micro_phase4.sin()
            + 0.10 * self.micro_phase5.sin();
        let pulse = 1.0 - pulse_depth + pulse_depth * (0.5 + 0.5 * pulse_raw);

        // ---- Sub-harmonic resonance: scales with progression ----
        // Base 8% depth, building to 24% during surge. The device motor's
        // mechanical resonance (~150-200Hz) couples with these sub-harmonic
        // amplitude rates to create deep tissue "throbbing" that intensifies
        // as the cycle builds toward climax.
        let sub_freq_hz = 1.5 + ramp * 2.5 + energy * 0.5;
        self.sub_harmonic_phase =
            (self.sub_harmonic_phase + dt * sub_freq_hz * TAU).rem_euclid(TAU);
        let sub_depth = 0.08 + 0.16 * ramp; // 8% → 24%
        let sub_resonance = 1.0 + sub_depth * intensity * self.sub_harmonic_phase.sin();

        // ---- Chaos layer (Lorenz attractor): scales with progression ----
        // Aperiodic modulation prevents prediction and filtering.
        // Barely noticeable at cycle start (6%), unmistakable at surge (18%).
        let sigma = 10.0_f32;
        let rho = 28.0_f32;
        let beta = 8.0_f32 / 3.0;
        let chaos_step = dt * 0.8;
        let dx = sigma * (self.chaos_y - self.chaos_x) * chaos_step;
        let dy = (self.chaos_x * (rho - self.chaos_z) - self.chaos_y) * chaos_step;
        let dz = (self.chaos_x * self.chaos_y - beta * self.chaos_z) * chaos_step;
        self.chaos_x = (self.chaos_x + dx).clamp(-30.0, 30.0);
        self.chaos_y = (self.chaos_y + dy).clamp(-30.0, 30.0);
        self.chaos_z = (self.chaos_z + dz).clamp(0.0, 50.0);
        let chaos_depth = 0.06 + 0.12 * ramp; // 6% → 18%
        let chaos_mod = 1.0 + chaos_depth * intensity * (self.chaos_x / 30.0);

        // ---- Breathing-rate modulation ----
        // Human arousal breathing settles at ~0.15-0.25 Hz. This very slow
        // sine couples with the user's involuntary breathing pattern,
        // amplifying the physiological feedback loop between body and device.
        // Depth increases with progression — subtle at start, consuming at peak.
        let breathing_hz = 0.18;
        self.breathing_phase =
            (self.breathing_phase + dt * breathing_hz * TAU).rem_euclid(TAU);
        let breathing_depth = 0.06 + 0.10 * ramp; // 6% → 16%
        let breathing_mod = 1.0 + breathing_depth * self.breathing_phase.sin();

        // ---- Arousal gain: aggressive escalation ----
        // At ramp=0: gain = 1.0 (passthrough).
        // At ramp=1 with max momentum: up to 3.8x — overwhelming crescendo
        // that compensates for desensitization over long sessions.
        let momentum_bonus = self.arousal_momentum * 0.7;
        let arousal_gain =
            (1.0 + (1.2 + momentum_bonus) * ramp) * (1.0 + intensity * 0.40);
        let gated_boost = if gate_open { self.onset_boost } else { 0.0 };

        let raw_output = (dry * arousal_gain * tease_factor * surge_factor
            * pulse * sub_resonance * chaos_mod * breathing_mod
            + gated_boost)
            .clamp(0.0, 1.0);

        // ---- Dual-motor spatial contrast ----
        let phase_offset_hz = 0.3 + ramp * 1.7;
        self.motor2_phase =
            (self.motor2_phase + dt * phase_offset_hz * TAU).rem_euclid(TAU);
        let phase_mod = 0.5 + 0.5 * self.motor2_phase.sin();
        let anti_phase_depth = raw_output.clamp(0.0, 1.0) * 0.85;
        let motor2_factor = lerp(1.0, 0.15 + 0.85 * phase_mod, anti_phase_depth);
        self.motor2_output = (raw_output * motor2_factor).clamp(0.0, 1.0);

        // ---- Edge-and-deny: escalating across cycles ----
        // First deny is gentle and brief. Later denies are deeper and longer,
        // building frustration and making each return more devastating.
        // Time-to-deny also shortens — the system gets more aggressive.
        if raw_output > 0.75 {
            self.high_output_ms += dt * 1000.0;
        } else {
            self.high_output_ms = (self.high_output_ms - dt * 400.0).max(0.0);
        }

        // Deny trigger: 6s initially, dropping to 3s as cycles mature
        let deny_trigger_ms = 6000.0 - 3000.0 * cycle_maturity;
        if !self.deny_active && self.high_output_ms > deny_trigger_ms {
            self.deny_active = true;
            self.deny_start_ms = current_time_ms;
            // Duration escalates: 600ms initially → up to 3000ms in later cycles
            let base_duration = 600.0 + 1800.0 * cycle_maturity;
            let jitter = 0.5 + 0.5 * (current_time_ms * 0.00137).sin();
            self.deny_duration_ms = base_duration + 400.0 * jitter;
            self.high_output_ms = 0.0;
        }

        if self.deny_active {
            let deny_elapsed = current_time_ms - self.deny_start_ms;
            if deny_elapsed >= self.deny_duration_ms {
                self.deny_active = false;
                // Post-deny surge: overshoot harder after deeper denies.
                let post_deny_boost = 0.30 + 0.25 * cycle_maturity;
                self.onset_boost = (self.onset_boost + post_deny_boost).min(0.65);
            } else {
                let deny_t = deny_elapsed / self.deny_duration_ms;
                // Deny depth escalates: 60% initially → 90% at maturity
                let deny_depth = 0.60 + 0.30 * cycle_maturity;
                // Asymmetric envelope: cliff → hold → slow sensual return.
                let deny_envelope = if deny_t < 0.10 {
                    // Sharp cliff (50-100ms to floor)
                    deny_depth * smooth_step(deny_t / 0.10)
                } else if deny_t < 0.75 {
                    // Hold at floor — nerve endings reset, ache builds
                    deny_depth
                } else {
                    // Slow curved return (last 25%) — deliberately agonizing
                    let return_t = (deny_t - 0.75) / 0.25;
                    deny_depth * (1.0 - smooth_step(return_t))
                };
                let denied = (raw_output * (1.0 - deny_envelope)).clamp(0.0, 1.0);
                self.motor2_output =
                    (self.motor2_output * (1.0 - deny_envelope * 0.7)).clamp(0.0, 1.0);
                return denied;
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

// ==========================================================================
// Tests
// ==========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Helper functions ---

    #[test]
    fn test_lerp() {
        assert_eq!(lerp(0.0, 10.0, 0.0), 0.0);
        assert_eq!(lerp(0.0, 10.0, 1.0), 10.0);
        assert_eq!(lerp(0.0, 10.0, 0.5), 5.0);
        assert_eq!(lerp(2.0, 8.0, 0.25), 3.5);
    }

    #[test]
    fn test_apply_curve() {
        // Linear (exponent 1.0) should return input
        assert!((apply_curve(0.5, 1.0) - 0.5).abs() < 1e-6);
        // Quadratic (exponent 2.0) should square input
        assert!((apply_curve(0.5, 2.0) - 0.25).abs() < 1e-6);
        // Zero input should always be zero
        assert_eq!(apply_curve(0.0, 2.0), 0.0);
        // One input should always be one
        assert!((apply_curve(1.0, 3.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_smooth_step() {
        assert_eq!(smooth_step(0.0), 0.0);
        assert!((smooth_step(1.0) - 1.0).abs() < 1e-6);
        assert!((smooth_step(0.5) - 0.5).abs() < 1e-6);
        // Should clamp out-of-range values
        assert_eq!(smooth_step(-1.0), 0.0);
        assert!((smooth_step(2.0) - 1.0).abs() < 1e-6);
    }

    // --- SpectralAnalyzer ---

    #[test]
    fn smoke_spectral_analyzer() {
        let mut sa = SpectralAnalyzer::new(48000.0);
        let silence = vec![0.0f32; FFT_SIZE * 2]; // stereo silence
        let data = sa.analyze(&silence, 2);
        for &e in &data.band_energies {
            assert!(e >= 0.0, "band energy should be non-negative");
            assert!(e < 0.01, "silence should produce near-zero energy");
        }
        assert!(data.spectral_flux >= 0.0);
    }

    #[test]
    fn spectral_analyzer_detects_sine() {
        let mut sa = SpectralAnalyzer::new(48000.0);
        // Generate a 440 Hz sine wave (mono)
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (TAU * 440.0 * i as f32 / 48000.0).sin() * 0.5)
            .collect();
        let data = sa.analyze(&samples, 1);
        // 440 Hz falls in the "Lo-Mid" band (250-500 Hz) or "Mid" band
        // At least some bands should have significant energy
        let total: f32 = data.band_energies.iter().sum();
        assert!(total > 0.01, "sine wave should produce measurable energy");
    }

    #[test]
    fn spectral_flux_spikes_on_change() {
        let mut sa = SpectralAnalyzer::new(48000.0);
        // First frame: silence
        let silence = vec![0.0f32; FFT_SIZE];
        let _ = sa.analyze(&silence, 1);
        // Second frame: loud sine
        let loud: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (TAU * 1000.0 * i as f32 / 48000.0).sin())
            .collect();
        let data = sa.analyze(&loud, 1);
        assert!(data.spectral_flux > 0.0, "flux should spike on onset");
    }

    // --- Gate ---

    #[test]
    fn gate_closed_below_threshold() {
        let mut gate = Gate::new();
        let open = gate.process(0.1, 0.5, 0.0, 0.0);
        assert!(!open, "gate should be closed when energy < threshold");
    }

    #[test]
    fn gate_open_above_threshold() {
        let mut gate = Gate::new();
        let open = gate.process(0.8, 0.5, 0.0, 0.0);
        assert!(open, "gate should be open when energy > threshold");
    }

    #[test]
    fn gate_hysteresis_prevents_chatter() {
        let mut gate = Gate::new();
        // Open the gate
        gate.process(0.6, 0.5, 0.0, 0.0);
        // Energy drops slightly below threshold — should stay open due to hysteresis
        let open = gate.process(0.48, 0.5, 0.0, 0.0);
        assert!(open, "hysteresis should keep gate open just below threshold");
        // Energy drops well below — should close
        let open = gate.process(0.2, 0.5, 0.0, 0.0);
        assert!(!open, "gate should close when energy drops significantly");
    }

    // --- EnvelopeProcessor ---

    #[test]
    fn smoke_envelope_processor() {
        let mut env = EnvelopeProcessor::new();
        assert_eq!(env.state, EnvelopeState::Idle);
        // Trigger should transition to Attack (attack_ms >= 50 -> Attack state)
        env.trigger(1.0, 100.0, 0.0, 100.0);
        assert_eq!(env.state, EnvelopeState::Attack);
    }

    #[test]
    fn envelope_short_attack_skips_to_decay() {
        let mut env = EnvelopeProcessor::new();
        // Short attack (< 50ms) should skip directly to Decay
        env.trigger(1.0, 100.0, 0.0, 30.0);
        assert_eq!(env.state, EnvelopeState::Decay);
    }

    // --- BeatDetector ---

    #[test]
    fn smoke_beat_detector() {
        let mut bd = BeatDetector::new();
        let (is_onset, strength) = bd.process(0.0, 0.0);
        assert!(!is_onset);
        assert_eq!(strength, 0.0);
    }

    #[test]
    fn beat_detector_detects_spike() {
        let mut bd = BeatDetector::new();
        // Feed low flux for a while to establish baseline (with advancing time)
        for i in 0..200 {
            bd.process(0.01, i as f32 * 16.0); // ~16ms per frame
        }
        // Large spike well above adaptive threshold, with time past cooldown
        let time = 200.0 * 16.0 + 100.0; // well past cooldown
        let (is_onset, _) = bd.process(5.0, time);
        assert!(is_onset, "beat detector should detect a large flux spike");
    }

    // --- ClimaxEngine ---

    #[test]
    fn climax_engine_passthrough_when_disabled() {
        let mut ce = ClimaxEngine::new();
        // process(input, energy, gate_open, is_onset, onset_strength, current_time_ms,
        //         enabled, intensity, build_up_ms, tease_ratio, tease_drop, surge_boost,
        //         pulse_depth, pattern)
        let output = ce.process(
            0.75, 0.5, true, false, 0.0, 0.0,
            false, 0.5, 60000.0, 0.18, 0.3, 0.2, 0.15, ClimaxPattern::Wave,
        );
        assert!(
            (output - 0.75).abs() < 0.01,
            "disabled climax should pass through input, got {output}"
        );
    }

    #[test]
    fn climax_engine_bounded_output() {
        let mut ce = ClimaxEngine::new();
        for i in 0..1000 {
            let time = i as f32 * 100.0;
            let output = ce.process(
                0.5, 0.5, true, false, 0.0, time,
                true, 0.8, 60000.0, 0.18, 0.3, 0.2, 0.15, ClimaxPattern::Wave,
            );
            assert!(
                output >= 0.0 && output <= 1.5,
                "climax output should stay bounded, got {output}"
            );
        }
    }

    // --- SharedSpectralData ---

    #[test]
    fn shared_spectral_data_round_trip() {
        let shared = SharedSpectralData::new();
        let data = SpectralData {
            band_energies: [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8],
            rms_power: 0.0,
            spectral_centroid: 1500.0,
            spectral_flux: 0.42,
            dominant_frequency: 0.0,
        };
        shared.store(data.clone());
        let loaded = shared.load();
        assert_eq!(loaded.band_energies, data.band_energies);
        assert_eq!(loaded.spectral_centroid, 1500.0);
        assert_eq!(loaded.spectral_flux, 0.42);
    }

    // --- extract_energy modes ---

    #[test]
    fn extract_energy_full_mode() {
        let data = SpectralData {
            band_energies: [1.0; NUM_BANDS],
            rms_power: 0.0,
            spectral_centroid: 0.0,
            spectral_flux: 0.0,
            dominant_frequency: 0.0,
        };
        let energy = SpectralAnalyzer::extract_energy(&data, FrequencyMode::Full, 0.0);
        assert!(energy > 0.0, "full mode should produce positive energy");
        assert!((energy - 1.0).abs() < 0.01, "uniform bands should give ~1.0");
    }
}
