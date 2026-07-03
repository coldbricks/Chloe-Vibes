// ==========================================================================
// auto_lock.rs -- AUTO-LOCK: supervising estimator-controller (Phase 1)
//
// One button that fits the signal chain to the playing material: the most
// rhythmic drive band, the punchiest trigger shape, and an envelope whose
// decay fits the tempo. Reads the signals the engine already publishes each
// frame and writes a WHITELISTED subset of existing Settings fields through
// a short glide. The engine itself is untouched, so Rust/Kotlin parity is
// preserved by construction, and the write struct simply lacks every field
// Auto-Lock must never touch: volume, output gain, output floor/ceiling,
// gate, climax, trim.
//
// Full design + skeptic-verdict rationale: docs/AUTO_LOCK_DESIGN.md
// ==========================================================================

use std::collections::VecDeque;

use crate::{
    audio::{FrequencyMode, SpectralData, TriggerMode},
    settings::Settings,
};

const N_BANDS: usize = 8;
/// Band edges in Hz — the parity-locked spec shared with the Kotlin engine.
const BAND_EDGES: [f32; 9] = [
    20.0, 60.0, 250.0, 500.0, 2000.0, 4000.0, 6000.0, 12000.0, 20000.0,
];

const RING_WINDOW_MS: f32 = 8_000.0;
const LISTEN_BUDGET_MS: f32 = 15_000.0;
const LISTEN_MIN_VALID_MS: f32 = 4_000.0;
const GLIDE_MS: f32 = 1_500.0;
/// Best band must beat the runner-up by this factor, else stay Full.
const SALIENCE_MARGIN: f32 = 1.3;
const MIN_LOCK_SCORE: f32 = 0.45;
/// Onsets closer than this are folded into one (the predictive pre-fire can
/// emit an onset up to ~76ms before the detected one).
const ONSET_MERGE_MS: f32 = 120.0;
/// A frame counts as onset-aligned if a merged onset happened within this
/// window before it (covers pre-fire lead + transient bloom).
const ONSET_ALIGN_MS: f32 = 100.0;
/// A frame is "valid audio" above this pre-volume energy.
const VALID_ENERGY: f32 = 0.002;

// ---------------------------------------------------------------------------
// The write whitelist
// ---------------------------------------------------------------------------

/// Every Settings field Auto-Lock is allowed to write — and ONLY those.
/// main_volume, output_gain, min_vibe, max_vibe, gate_*, climax_*, trim_ms
/// are deliberately absent: the supervisor cannot raise delivered intensity
/// above what the user configured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LockParams {
    frequency_mode: FrequencyMode,
    target_frequency: f32,
    trigger_mode: TriggerMode,
    binary_level: f32,
    hybrid_blend: f32,
    threshold_knee: f32,
    dynamic_curve: f32,
    attack_ms: f32,
    decay_ms: f32,
    sustain_level: f32,
    release_ms: f32,
    attack_curve: f32,
    decay_curve: f32,
    release_curve: f32,
    input_rise_ms: f32,
    input_fall_ms: f32,
    output_slew_ms: f32,
}

impl LockParams {
    fn capture(s: &Settings) -> Self {
        Self {
            frequency_mode: s.frequency_mode,
            target_frequency: s.target_frequency,
            trigger_mode: s.trigger_mode,
            binary_level: s.binary_level,
            hybrid_blend: s.hybrid_blend,
            threshold_knee: s.threshold_knee,
            dynamic_curve: s.dynamic_curve,
            attack_ms: s.attack_ms,
            decay_ms: s.decay_ms,
            sustain_level: s.sustain_level,
            release_ms: s.release_ms,
            attack_curve: s.attack_curve,
            decay_curve: s.decay_curve,
            release_curve: s.release_curve,
            input_rise_ms: s.input_rise_ms,
            input_fall_ms: s.input_fall_ms,
            output_slew_ms: s.output_slew_ms,
        }
    }

    /// Write these values into Settings. Does not touch current_preset_name.
    pub fn apply(&self, s: &mut Settings) {
        s.frequency_mode = self.frequency_mode;
        s.target_frequency = self.target_frequency;
        s.trigger_mode = self.trigger_mode;
        s.binary_level = self.binary_level;
        s.hybrid_blend = self.hybrid_blend;
        s.threshold_knee = self.threshold_knee;
        s.dynamic_curve = self.dynamic_curve;
        s.attack_ms = self.attack_ms;
        s.decay_ms = self.decay_ms;
        s.sustain_level = self.sustain_level;
        s.release_ms = self.release_ms;
        s.attack_curve = self.attack_curve;
        s.decay_curve = self.decay_curve;
        s.release_curve = self.release_curve;
        s.input_rise_ms = self.input_rise_ms;
        s.input_fall_ms = self.input_fall_ms;
        s.output_slew_ms = self.output_slew_ms;
    }

    fn diverged_from(&self, s: &Settings) -> bool {
        const EPS: f32 = 1e-3;
        self.frequency_mode != s.frequency_mode
            || self.trigger_mode != s.trigger_mode
            || (self.target_frequency - s.target_frequency).abs() > EPS
            || (self.binary_level - s.binary_level).abs() > EPS
            || (self.hybrid_blend - s.hybrid_blend).abs() > EPS
            || (self.threshold_knee - s.threshold_knee).abs() > EPS
            || (self.dynamic_curve - s.dynamic_curve).abs() > EPS
            || (self.attack_ms - s.attack_ms).abs() > EPS
            || (self.decay_ms - s.decay_ms).abs() > EPS
            || (self.sustain_level - s.sustain_level).abs() > EPS
            || (self.release_ms - s.release_ms).abs() > EPS
            || (self.attack_curve - s.attack_curve).abs() > EPS
            || (self.decay_curve - s.decay_curve).abs() > EPS
            || (self.release_curve - s.release_curve).abs() > EPS
            || (self.input_rise_ms - s.input_rise_ms).abs() > EPS
            || (self.input_fall_ms - s.input_fall_ms).abs() > EPS
            || (self.output_slew_ms - s.output_slew_ms).abs() > EPS
    }

    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let l = |x: f32, y: f32| x + (y - x) * t;
        Self {
            // Enums commit at glide start (no onset-boundary scheduling in P1)
            frequency_mode: b.frequency_mode,
            trigger_mode: b.trigger_mode,
            target_frequency: l(a.target_frequency, b.target_frequency),
            binary_level: l(a.binary_level, b.binary_level),
            hybrid_blend: l(a.hybrid_blend, b.hybrid_blend),
            threshold_knee: l(a.threshold_knee, b.threshold_knee),
            dynamic_curve: l(a.dynamic_curve, b.dynamic_curve),
            attack_ms: l(a.attack_ms, b.attack_ms),
            decay_ms: l(a.decay_ms, b.decay_ms),
            sustain_level: l(a.sustain_level, b.sustain_level),
            release_ms: l(a.release_ms, b.release_ms),
            attack_curve: l(a.attack_curve, b.attack_curve),
            decay_curve: l(a.decay_curve, b.decay_curve),
            release_curve: l(a.release_curve, b.release_curve),
            input_rise_ms: l(a.input_rise_ms, b.input_rise_ms),
            input_fall_ms: l(a.input_fall_ms, b.input_fall_ms),
            output_slew_ms: l(a.output_slew_ms, b.output_slew_ms),
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AutoLockState {
    Idle,
    Listening { started_ms: f32 },
    Locked { score: f32 },
    NoLock { reason: &'static str },
}

struct FrameSample {
    t_ms: f32,
    band_energies: [f32; N_BANDS],
    pre_volume_energy: f32,
    centroid: f32,
    envelope_output: f32,
    valid: bool,
}

struct Glide {
    started_ms: f32,
    from: LockParams,
    to: LockParams,
}

pub struct AutoLock {
    pub state: AutoLockState,
    frames: VecDeque<FrameSample>,
    onsets: VecDeque<f32>,
    last_band_bits: [u32; N_BANDS],
    /// Pre-lock user settings, restored by revert and used by the
    /// persistence guard so eframe auto-save can never persist a lock.
    snapshot: Option<LockParams>,
    glide: Option<Glide>,
    /// What we last wrote — any mismatch means the user (or a preset click)
    /// took over, which cancels the lock without fighting them.
    expected: Option<LockParams>,
}

impl AutoLock {
    pub fn new() -> Self {
        Self {
            state: AutoLockState::Idle,
            frames: VecDeque::with_capacity(512),
            onsets: VecDeque::with_capacity(64),
            last_band_bits: [0; N_BANDS],
            snapshot: None,
            glide: None,
            expected: None,
        }
    }

    /// Pre-lock snapshot for the persistence guard, if a lock is active.
    pub fn pre_lock_snapshot(&self) -> Option<LockParams> {
        self.snapshot
    }

    pub fn live_params(&self, settings: &Settings) -> LockParams {
        LockParams::capture(settings)
    }

    /// Cancel any lock/listen in progress WITHOUT reverting settings
    /// (the user's own change wins).
    pub fn cancel(&mut self) {
        self.state = AutoLockState::Idle;
        self.snapshot = None;
        self.glide = None;
        self.expected = None;
    }

    /// Restore the pre-lock settings and go idle.
    pub fn revert(&mut self, settings: &mut Settings) {
        if let Some(snap) = self.snapshot.take() {
            snap.apply(settings);
            settings.sanitize();
        }
        self.cancel();
    }

    /// Keep the locked values as the new normal (explicit consent — this is
    /// the only way a lock outlives the session).
    pub fn keep(&mut self, _settings: &mut Settings) {
        self.cancel();
    }

    /// Main button behavior per state.
    pub fn on_button(&mut self, now_ms: f32) {
        match self.state {
            AutoLockState::Idle | AutoLockState::NoLock { .. } => {
                self.state = AutoLockState::Listening { started_ms: now_ms };
            }
            AutoLockState::Listening { .. } => {
                // Cancel the listen; nothing was written yet.
                self.state = AutoLockState::Idle;
            }
            AutoLockState::Locked { .. } => {
                // Re-lock on fresh material. The original pre-lock snapshot
                // is kept, so Revert still returns to the user's settings.
                self.state = AutoLockState::Listening { started_ms: now_ms };
                self.glide = None;
            }
        }
    }

    pub fn button_label(&self, now_ms: f32) -> String {
        match self.state {
            AutoLockState::Idle => "AUTO-LOCK".to_string(),
            AutoLockState::Listening { started_ms } => {
                format!(
                    "LISTENING {:.0}s",
                    ((now_ms - started_ms) / 1000.0).max(0.0)
                )
            }
            AutoLockState::Locked { score } => {
                format!("LOCKED {:.0}%", (score * 100.0).clamp(0.0, 99.0))
            }
            AutoLockState::NoLock { reason } => format!("NO LOCK — {reason}"),
        }
    }

    pub fn is_locked(&self) -> bool {
        matches!(self.state, AutoLockState::Locked { .. })
    }

    // -----------------------------------------------------------------------
    // Per-frame tick (advanced pipeline only)
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        now_ms: f32,
        spectral: &SpectralData,
        pre_volume_energy: f32,
        onset_ok: bool,
        envelope_output: f32,
        using_rms_fallback: bool,
        engine_tempo_confidence: f32,
        settings: &mut Settings,
    ) {
        self.record_frame(
            now_ms,
            spectral,
            pre_volume_energy,
            onset_ok,
            envelope_output,
            using_rms_fallback,
        );

        // User-override / preset-race guard: if any whitelisted field no
        // longer matches what we last wrote, the user (or apply_preset)
        // intervened — cancel before writing anything this frame.
        if let Some(expected) = &self.expected {
            if expected.diverged_from(settings) {
                self.cancel();
                return;
            }
        }

        match self.state {
            AutoLockState::Listening { started_ms } => {
                let valid_ms = self.valid_window_ms();
                let elapsed = now_ms - started_ms;
                let ring_full = valid_ms >= RING_WINDOW_MS - 500.0;
                if ring_full || elapsed >= LISTEN_BUDGET_MS {
                    if valid_ms >= LISTEN_MIN_VALID_MS {
                        self.try_commit(now_ms, engine_tempo_confidence, settings);
                    } else {
                        self.state = AutoLockState::NoLock {
                            reason: "not enough audio",
                        };
                    }
                }
            }
            AutoLockState::Locked { .. } => self.tick_glide(now_ms, settings),
            _ => {}
        }
    }

    fn record_frame(
        &mut self,
        now_ms: f32,
        spectral: &SpectralData,
        pre_volume_energy: f32,
        onset_ok: bool,
        envelope_output: f32,
        using_rms_fallback: bool,
    ) {
        // Merged-onset record (predictive pre-fire + detected onset arrive as
        // a pair up to ~76ms apart; treat them as one beat).
        if onset_ok {
            let is_new = self
                .onsets
                .back()
                .map(|last| now_ms - last > ONSET_MERGE_MS)
                .unwrap_or(true);
            if is_new {
                self.onsets.push_back(now_ms);
                if self.onsets.len() > 64 {
                    self.onsets.pop_front();
                }
            }
        }

        // Bitwise dedupe: update() repaints faster than the capture thread
        // produces frames, so identical snapshots must not be re-counted.
        let mut bits = [0u32; N_BANDS];
        for (i, b) in bits.iter_mut().enumerate() {
            *b = spectral.band_energies[i].to_bits();
        }
        if bits == self.last_band_bits && !self.frames.is_empty() {
            return;
        }
        self.last_band_bits = bits;

        let mut band_energies = [0.0f32; N_BANDS];
        band_energies.copy_from_slice(&spectral.band_energies[..N_BANDS]);
        self.frames.push_back(FrameSample {
            t_ms: now_ms,
            band_energies,
            pre_volume_energy,
            centroid: spectral.spectral_centroid,
            envelope_output,
            valid: !using_rms_fallback && pre_volume_energy > VALID_ENERGY,
        });

        while let Some(front) = self.frames.front() {
            if now_ms - front.t_ms > RING_WINDOW_MS {
                self.frames.pop_front();
            } else {
                break;
            }
        }
        while let Some(front) = self.onsets.front() {
            if now_ms - front > RING_WINDOW_MS {
                self.onsets.pop_front();
            } else {
                break;
            }
        }
    }

    fn valid_window_ms(&self) -> f32 {
        let mut total = 0.0;
        for pair in self.frames.iter().collect::<Vec<_>>().windows(2) {
            if pair[1].valid {
                total += pair[1].t_ms - pair[0].t_ms;
            }
        }
        total
    }

    // -----------------------------------------------------------------------
    // Estimation + commit
    // -----------------------------------------------------------------------

    fn try_commit(&mut self, now_ms: f32, engine_conf: f32, settings: &mut Settings) {
        let features = match self.estimate() {
            Ok(f) => f,
            Err(reason) => {
                self.state = AutoLockState::NoLock { reason };
                return;
            }
        };

        // Tempo trust: the engine's live confidence or our own IOI stability,
        // whichever is stronger right now.
        let own_conf = if features.ioi_median > 0.0 {
            (1.0 - 4.0 * (features.ioi_iqr / features.ioi_median)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let conf = engine_conf.max(own_conf);
        let margin_norm = ((features.salience_margin - 1.0) / 1.5).clamp(0.0, 1.0);
        let score = 0.55 * conf + 0.30 * margin_norm + 0.15 * (1.0 - features.silence_ratio);

        if conf < 0.35 || score < MIN_LOCK_SCORE {
            self.state = AutoLockState::NoLock {
                reason: "no steady rhythm",
            };
            return;
        }

        let target = Self::map_features(&features, settings);

        // First lock this session: snapshot the user's settings for revert
        // and the persistence guard. A re-lock keeps the original snapshot.
        if self.snapshot.is_none() {
            self.snapshot = Some(LockParams::capture(settings));
        }

        let from = LockParams::capture(settings);
        // Enums switch immediately; floats glide from current values.
        settings.frequency_mode = target.frequency_mode;
        settings.trigger_mode = target.trigger_mode;
        settings.current_preset_name = String::new();
        settings.sanitize();
        self.expected = Some(LockParams::capture(settings));
        self.glide = Some(Glide {
            started_ms: now_ms,
            from,
            to: target,
        });
        self.state = AutoLockState::Locked { score };
    }

    fn tick_glide(&mut self, now_ms: f32, settings: &mut Settings) {
        let Some(glide) = &self.glide else { return };
        let p = ((now_ms - glide.started_ms) / GLIDE_MS).clamp(0.0, 1.0);
        let eased = p * p * (3.0 - 2.0 * p);
        let current = LockParams::lerp(&glide.from, &glide.to, eased);
        current.apply(settings);
        settings.sanitize();
        self.expected = Some(LockParams::capture(settings));
        if p >= 1.0 {
            self.glide = None;
        }
    }

    fn estimate(&self) -> Result<Features, &'static str> {
        let frames: Vec<&FrameSample> = self.frames.iter().collect();
        if frames.len() < 40 {
            return Err("not enough audio");
        }

        // Silence ratio over the window
        let silent = frames.iter().filter(|f| !f.valid).count();
        let silence_ratio = silent as f32 / frames.len() as f32;

        // Inter-onset intervals, filtered to a plausible beat range
        let onsets: Vec<f32> = self.onsets.iter().copied().collect();
        if onsets.len() < 8 {
            return Err("no steady rhythm");
        }
        let mut iois: Vec<f32> = onsets
            .windows(2)
            .map(|w| w[1] - w[0])
            .filter(|d| (150.0..=2000.0).contains(d))
            .collect();
        if iois.len() < 5 {
            return Err("no steady rhythm");
        }
        iois.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let ioi_median = percentile_sorted(&iois, 0.5);
        let ioi_iqr = percentile_sorted(&iois, 0.75) - percentile_sorted(&iois, 0.25);

        // Per-band rhythmic salience: the fraction of each band's total
        // half-wave-rectified energy delta that lands at onset times. High =
        // the band moves WITH the beat, independent of its absolute loudness.
        let mut at_onset = [0.0f32; N_BANDS];
        let mut total = [1e-6f32; N_BANDS];
        for pair in frames.windows(2) {
            let (prev, cur) = (pair[0], pair[1]);
            let aligned = onsets
                .iter()
                .any(|&o| cur.t_ms >= o && cur.t_ms - o <= ONSET_ALIGN_MS);
            for b in 0..N_BANDS {
                let delta = (cur.band_energies[b] - prev.band_energies[b]).max(0.0);
                total[b] += delta;
                if aligned {
                    at_onset[b] += delta;
                }
            }
        }
        let mut salience = [0.0f32; N_BANDS];
        for b in 0..N_BANDS {
            // Weight concentration by absolute activity so a near-silent band
            // with two lucky spikes can't win.
            let activity = (total[b] / frames.len() as f32).sqrt();
            salience[b] = (at_onset[b] / total[b]) * activity;
        }
        let mut order: Vec<usize> = (0..N_BANDS).collect();
        order.sort_by(|&a, &b| salience[b].partial_cmp(&salience[a]).unwrap());
        let best_band = order[0];
        let salience_margin = if salience[order[1]] > 1e-6 {
            salience[best_band] / salience[order[1]]
        } else {
            f32::INFINITY
        };

        // Crest factor of the pre-volume energy (what the gate sees)
        let mut energies: Vec<f32> = frames
            .iter()
            .filter(|f| f.valid)
            .map(|f| f.pre_volume_energy)
            .collect();
        if energies.is_empty() {
            return Err("not enough audio");
        }
        energies.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let crest =
            percentile_sorted(&energies, 0.95) / percentile_sorted(&energies, 0.50).max(1e-4);

        // Median centroid (valid frames), for engine-exact pre-compensation
        let mut centroids: Vec<f32> = frames
            .iter()
            .filter(|f| f.valid)
            .map(|f| f.centroid)
            .collect();
        centroids.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let centroid_median = percentile_sorted(&centroids, 0.5);

        // Observed dynamic envelope output cap for binary_level
        let mut envs: Vec<f32> = frames.iter().map(|f| f.envelope_output).collect();
        envs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let envelope_p90 = percentile_sorted(&envs, 0.90);

        Ok(Features {
            ioi_median,
            ioi_iqr,
            best_band,
            salience_margin,
            crest,
            centroid_median,
            envelope_p90,
            silence_ratio,
        })
    }

    /// Feature -> parameter mapping. All numbers land inside sanitize()
    /// ranges; intensity-bearing fields are capped by observed output.
    fn map_features(f: &Features, settings: &Settings) -> LockParams {
        let t = f.ioi_median;

        // Frequency focus, only with a clear winner
        let (frequency_mode, target_frequency) = if f.salience_margin >= SALIENCE_MARGIN {
            match f.best_band {
                0 | 1 => (FrequencyMode::LowPass, BAND_EDGES[2]), // sub+bass together
                2 | 3 => (
                    FrequencyMode::BandPass,
                    (BAND_EDGES[f.best_band] * BAND_EDGES[f.best_band + 1]).sqrt(),
                ),
                b => (FrequencyMode::HighPass, BAND_EDGES[b]),
            }
        } else {
            (FrequencyMode::Full, settings.target_frequency)
        };

        // Trigger shape from material punchiness
        let (trigger_mode, hybrid_blend, dynamic_curve) = if f.crest >= 4.0 {
            (TriggerMode::Hybrid, 0.7, 1.0)
        } else if f.crest >= 2.2 {
            (TriggerMode::Hybrid, 0.45, 1.2)
        } else {
            // Compressed material: stay dynamic, expand contrast via curve
            (TriggerMode::Dynamic, settings.hybrid_blend, 1.6)
        };

        // Never deliver more than the dynamic path already did
        let binary_level = f.envelope_p90.clamp(0.0, 0.65);

        // Envelope fitted to the beat. Decay MUST fit inside the IOI: the
        // engine only retriggers from Sustain, so a decay longer than the
        // gap between hits silently eats beats.
        let decay_ms = (0.30 * t).clamp(50.0, 600.0).min(0.6 * t);
        let sustain_target = if t < 300.0 {
            0.18
        } else if t < 600.0 {
            0.28
        } else {
            0.38
        };
        let release_target = (0.55 * t).clamp(80.0, 1500.0);

        // Engine-exact centroid pre-compensation (audio.rs frequency shaping:
        // sustain *= 1 - 0.25*cn; release *= 1 + 0.4*(1-cn), with the LINEAR
        // norm cn = clamp((centroid-100)/4000, 0, 1) — not a log scale).
        let cn = ((f.centroid_median - 100.0) / 4000.0).clamp(0.0, 1.0);
        let sustain_level = (sustain_target / (1.0 - 0.25 * cn)).clamp(0.0, 1.0);
        let release_ms = (release_target / (1.0 + 0.4 * (1.0 - cn))).clamp(0.5, 5000.0);

        // Input smoothing + output slew scaled to the material
        let input_rise_ms = if f.crest >= 2.2 { 8.0 } else { 25.0 };
        let input_fall_ms = (0.35 * t).clamp(80.0, 300.0);
        let output_slew_ms = (0.18 * t).clamp(35.0, 120.0);

        LockParams {
            frequency_mode,
            target_frequency,
            trigger_mode,
            binary_level,
            hybrid_blend,
            threshold_knee: 0.15,
            dynamic_curve,
            // Anything < 50ms takes the engine's instant-peak fast path;
            // 20ms is honest — finer "tuning" here would be a placebo.
            attack_ms: 20.0,
            decay_ms,
            sustain_level,
            release_ms,
            attack_curve: 0.7,
            decay_curve: 1.0,
            release_curve: 1.3,
            input_rise_ms,
            input_fall_ms,
            output_slew_ms,
        }
    }
}

struct Features {
    ioi_median: f32,
    ioi_iqr: f32,
    best_band: usize,
    salience_margin: f32,
    crest: f32,
    centroid_median: f32,
    envelope_p90: f32,
    silence_ratio: f32,
}

/// Percentile of an ascending-sorted, non-empty slice (nearest-rank).
fn percentile_sorted(sorted: &[f32], p: f32) -> f32 {
    let idx = ((sorted.len() as f32 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn spectral_with_bands(bands: [f32; N_BANDS]) -> SpectralData {
        SpectralData {
            band_energies: bands,
            ..SpectralData::default()
        }
    }

    /// Feed a synthetic session: band `beat_band` pulses every `ioi_ms`,
    /// others hold a noise floor. Returns (auto_lock, settings, now_ms).
    fn run_synthetic_session(
        beat_band: usize,
        ioi_ms: f32,
        pulse: f32,
    ) -> (AutoLock, Settings, f32) {
        let mut al = AutoLock::new();
        let mut settings = Settings::default();
        al.on_button(0.0);

        let frame_ms = 20.0;
        let mut now = 0.0f32;
        let mut next_beat = 100.0f32;
        // 16s of material so the ring fills regardless of budget
        while now < 16_000.0 {
            now += frame_ms;
            let mut bands = [0.02f32; N_BANDS];
            // Slight per-frame wiggle so bitwise dedupe doesn't drop frames
            bands[7] = 0.02 + (now / 1000.0).sin().abs() * 0.001;
            let mut onset = false;
            if now >= next_beat {
                bands[beat_band] = pulse;
                onset = true;
                next_beat += ioi_ms;
            }
            let energy = bands.iter().sum::<f32>() / N_BANDS as f32;
            al.tick(
                now,
                &spectral_with_bands(bands),
                energy,
                onset,
                if onset { 0.5 } else { 0.1 },
                false,
                0.9,
                &mut settings,
            );
        }
        (al, settings, now)
    }

    #[test]
    fn locks_on_rhythmic_bass_and_picks_low_band() {
        let (al, settings, _) = run_synthetic_session(1, 500.0, 0.8);
        assert!(al.is_locked(), "state = {:?}", al.state);
        assert_eq!(settings.frequency_mode, FrequencyMode::LowPass);
        assert!((settings.target_frequency - BAND_EDGES[2]).abs() < 1.0);
    }

    #[test]
    fn decay_fits_inside_the_beat_interval() {
        let (mut al, mut settings, now) = run_synthetic_session(1, 500.0, 0.8);
        assert!(al.is_locked());
        // Let the glide finish
        for i in 1..200 {
            let t = now + i as f32 * 20.0;
            al.tick(
                t,
                &spectral_with_bands([0.02 + (t % 7.0) * 1e-4; N_BANDS]),
                0.02,
                false,
                0.1,
                false,
                0.9,
                &mut settings,
            );
        }
        assert!(
            settings.decay_ms <= 0.6 * 500.0 + 1.0,
            "decay {} must fit inside the 500ms IOI",
            settings.decay_ms
        );
        assert!(settings.attack_ms < 50.0, "attack must take the fast path");
    }

    #[test]
    fn never_touches_ceiling_fields() {
        let mut probe = Settings::default();
        probe.main_volume = 1.234;
        probe.output_gain = 0.777;
        probe.min_vibe = 0.111;
        probe.max_vibe = 0.888;
        probe.gate_threshold = 0.099;
        probe.climax_intensity = 0.321;

        let mut al = AutoLock::new();
        al.on_button(0.0);
        let frame_ms = 20.0;
        let mut now = 0.0f32;
        let mut next_beat = 100.0f32;
        while now < 20_000.0 {
            now += frame_ms;
            let mut bands = [0.02f32; N_BANDS];
            bands[7] = 0.02 + (now / 900.0).sin().abs() * 0.001;
            let mut onset = false;
            if now >= next_beat {
                bands[1] = 0.8;
                onset = true;
                next_beat += 400.0;
            }
            let energy = bands.iter().sum::<f32>() / N_BANDS as f32;
            al.tick(
                now,
                &spectral_with_bands(bands),
                energy,
                onset,
                0.4,
                false,
                0.9,
                &mut probe,
            );
        }
        assert!(al.is_locked());
        assert_eq!(probe.main_volume, 1.234);
        assert_eq!(probe.output_gain, 0.777);
        assert_eq!(probe.min_vibe, 0.111);
        assert_eq!(probe.max_vibe, 0.888);
        assert_eq!(probe.gate_threshold, 0.099);
        assert_eq!(probe.climax_intensity, 0.321);
    }

    #[test]
    fn no_lock_on_silence() {
        let mut al = AutoLock::new();
        let mut settings = Settings::default();
        al.on_button(0.0);
        let mut now = 0.0f32;
        while now < 16_000.0 {
            now += 20.0;
            let bands = [(now % 13.0) * 1e-6; N_BANDS]; // ~zero, non-constant
            al.tick(
                now,
                &spectral_with_bands(bands),
                0.0001,
                false,
                0.0,
                false,
                0.0,
                &mut settings,
            );
        }
        assert!(
            matches!(al.state, AutoLockState::NoLock { .. }),
            "state = {:?}",
            al.state
        );
        // And nothing was written
        assert_eq!(settings.decay_ms, Settings::default().decay_ms);
    }

    #[test]
    fn manual_change_cancels_the_lock() {
        let (mut al, mut settings, now) = run_synthetic_session(1, 500.0, 0.8);
        assert!(al.is_locked());
        // User grabs a slider
        settings.decay_ms += 100.0;
        al.tick(
            now + 20.0,
            &spectral_with_bands([0.02; N_BANDS]),
            0.02,
            false,
            0.1,
            false,
            0.9,
            &mut settings,
        );
        assert_eq!(al.state, AutoLockState::Idle);
        assert!(al.pre_lock_snapshot().is_none());
    }

    #[test]
    fn revert_restores_pre_lock_settings() {
        let (mut al, mut settings, _) = run_synthetic_session(1, 500.0, 0.8);
        assert!(al.is_locked());
        let defaults = Settings::default();
        assert_ne!(settings.decay_ms, defaults.decay_ms); // lock changed it
        al.revert(&mut settings);
        assert_eq!(settings.decay_ms, defaults.decay_ms);
        assert_eq!(settings.frequency_mode, defaults.frequency_mode);
        assert_eq!(al.state, AutoLockState::Idle);
    }

    #[test]
    fn centroid_compensation_is_engine_exact() {
        // For a bright centroid (cn near 1) the engine barely shapes, so the
        // written value ~= target; for a dark centroid the written release
        // must be SMALLER so the engine's 1.4x stretch lands on target.
        let mut f = Features {
            ioi_median: 500.0,
            ioi_iqr: 20.0,
            best_band: 1,
            salience_margin: 2.0,
            crest: 3.0,
            centroid_median: 100.0, // cn = 0 (dark)
            envelope_p90: 0.5,
            silence_ratio: 0.0,
        };
        let s = Settings::default();
        let dark = AutoLock::map_features(&f, &s);
        f.centroid_median = 4_100.0; // cn = 1 (bright)
        let bright = AutoLock::map_features(&f, &s);

        let target = (0.55f32 * 500.0).clamp(80.0, 1500.0);
        // Dark: engine multiplies by 1.4 -> written must be target/1.4
        assert!((dark.release_ms * 1.4 - target).abs() < 0.5);
        // Bright: engine multiplies by 1.0 -> written == target
        assert!((bright.release_ms - target).abs() < 0.5);
        // Sustain compensation direction: dark writes as-is, bright writes higher
        assert!(bright.sustain_level > dark.sustain_level);
    }

    #[test]
    fn binary_level_capped_by_observed_output() {
        let f = Features {
            ioi_median: 400.0,
            ioi_iqr: 15.0,
            best_band: 1,
            salience_margin: 2.0,
            crest: 5.0, // punchy -> Hybrid uses binary_level
            centroid_median: 1000.0,
            envelope_p90: 0.3, // quiet material
            silence_ratio: 0.0,
        };
        let s = Settings::default();
        let p = AutoLock::map_features(&f, &s);
        assert_eq!(p.trigger_mode, TriggerMode::Hybrid);
        assert!(p.binary_level <= 0.3 + 1e-6);
    }

    #[test]
    fn weak_salience_margin_stays_full_spectrum() {
        // Two bands pulse together -> no clear winner -> Full
        let mut al = AutoLock::new();
        let mut settings = Settings::default();
        al.on_button(0.0);
        let mut now = 0.0f32;
        let mut next_beat = 100.0f32;
        while now < 16_000.0 {
            now += 20.0;
            let mut bands = [0.02f32; N_BANDS];
            bands[7] = 0.02 + (now / 800.0).sin().abs() * 0.001;
            let mut onset = false;
            if now >= next_beat {
                bands[1] = 0.8;
                bands[5] = 0.8;
                onset = true;
                next_beat += 500.0;
            }
            let energy = bands.iter().sum::<f32>() / N_BANDS as f32;
            al.tick(
                now,
                &spectral_with_bands(bands),
                energy,
                onset,
                0.4,
                false,
                0.9,
                &mut settings,
            );
        }
        assert!(al.is_locked(), "state = {:?}", al.state);
        assert_eq!(settings.frequency_mode, FrequencyMode::Full);
    }
}
