// ==========================================================================
// auto_lock.rs -- FIND BOOM (AUTO-LOCK): max-dynamic bass-drum tuner
//
// One button that listens to the playing material and locks the sweet spot:
//   kick-punch band, gate that opens on hits / closes in the trough,
//   Hybrid trigger at near-ceiling punch, and a boom envelope whose decay
//   spans ~78% of the felt beat into a near-zero floor.
//
// Goal: maximum *dynamic* output (contrast), not maximum continuous level.
// After lock, the user owns the knobs — any manual tweak cancels the lock
// and keeps the tuned values as the new starting point (or Revert).
//
// Whitelist: band/trigger/envelope/slew/gate. NEVER volume, output gain,
// min/max vibe, climax intensity, or trim (user consent ceiling).
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

/// Every Settings field FIND BOOM is allowed to write — and ONLY those.
/// main_volume, output_gain, min_vibe, max_vibe, climax intensity/cycle,
/// and trim_ms are deliberately absent: the supervisor cannot raise the
/// user's consent ceiling, only reshape the boom inside it.
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
    output_slew_ms: f32,
    /// Kick-open / trough-close threshold (pre-volume energy domain).
    gate_threshold: f32,
    gate_smoothing: f32,
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
            output_slew_ms: s.output_slew_ms,
            gate_threshold: s.gate_threshold,
            gate_smoothing: s.gate_smoothing,
        }
    }

    /// Write these values into Settings. Forces climax off (boom path).
    /// Does not touch current_preset_name.
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
        // input_rise/fall deliberately not written — dead in the motor path.
        s.output_slew_ms = self.output_slew_ms;
        s.gate_threshold = self.gate_threshold;
        s.gate_smoothing = self.gate_smoothing;
        s.auto_gate_amount = 0.0; // manual threshold we just fitted
        s.climax_mode_enabled = false;
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
            || (self.output_slew_ms - s.output_slew_ms).abs() > EPS
            || (self.gate_threshold - s.gate_threshold).abs() > EPS
            || (self.gate_smoothing - s.gate_smoothing).abs() > EPS
            || s.climax_mode_enabled
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
            output_slew_ms: l(a.output_slew_ms, b.output_slew_ms),
            gate_threshold: l(a.gate_threshold, b.gate_threshold),
            gate_smoothing: l(a.gate_smoothing, b.gate_smoothing),
        }
    }
}

/// Human-readable readout of the last successful lock (for the UI).
#[derive(Clone, Debug)]
pub struct LockReport {
    pub bpm: f32,
    pub band_label: &'static str,
    pub decay_ms: f32,
    pub gate_threshold: f32,
    pub binary_level: f32,
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
    /// Only audio captured AFTER this timestamp counts toward the current
    /// listen. Without it, a retry after NO LOCK instantly re-analyzed the
    /// same stale ring and bounced straight back to NO LOCK — the button
    /// appeared dead.
    listen_from_ms: f32,
    /// Last successful lock details for the UI readout.
    pub last_report: Option<LockReport>,
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
            listen_from_ms: 0.0,
            last_report: None,
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
        self.last_report = None;
        self.cancel();
    }

    /// Keep the locked values as the new normal (explicit consent — this is
    /// the only way a lock outlives the session without the snapshot guard).
    pub fn keep(&mut self, _settings: &mut Settings) {
        self.cancel();
    }

    /// Main button behavior per state.
    pub fn on_button(&mut self, now_ms: f32) {
        match self.state {
            AutoLockState::Idle | AutoLockState::NoLock { .. } => {
                // Fresh listen: only audio arriving from now on counts.
                self.listen_from_ms = now_ms;
                self.last_report = None;
                self.state = AutoLockState::Listening { started_ms: now_ms };
            }
            AutoLockState::Listening { .. } => {
                // Cancel the listen; nothing was written yet.
                self.state = AutoLockState::Idle;
            }
            AutoLockState::Locked { .. } => {
                // Re-lock on fresh material. The original pre-lock snapshot
                // is kept, so Revert still returns to the user's settings.
                self.listen_from_ms = now_ms;
                self.state = AutoLockState::Listening { started_ms: now_ms };
                self.glide = None;
            }
        }
    }

    pub fn button_label(&self, now_ms: f32) -> String {
        match self.state {
            AutoLockState::Idle => "FIND BOOM".to_string(),
            AutoLockState::Listening { started_ms } => {
                format!("TUNING {:.0}s…", ((now_ms - started_ms) / 1000.0).max(0.0))
            }
            AutoLockState::Locked { score } => {
                format!("BOOM {:.0}%", (score * 100.0).clamp(0.0, 99.0))
            }
            AutoLockState::NoLock { reason } => format!("NO LOCK — {reason}"),
        }
    }

    pub fn is_locked(&self) -> bool {
        matches!(self.state, AutoLockState::Locked { .. })
    }

    /// One-line summary after a successful lock, e.g.
    /// "124 BPM · Bass · decay 375ms · gate 0.12".
    pub fn report_line(&self) -> Option<String> {
        let r = self.last_report.as_ref()?;
        Some(format!(
            "{:.0} BPM · {} · decay {:.0}ms · gate {:.2} · punch {:.0}%",
            r.bpm,
            r.band_label,
            r.decay_ms,
            r.gate_threshold,
            (r.binary_level * 100.0).clamp(0.0, 100.0)
        ))
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
        _envelope_output: f32,
        using_rms_fallback: bool,
        engine_tempo_confidence: f32,
        settings: &mut Settings,
    ) {
        self.record_frame(
            now_ms,
            spectral,
            pre_volume_energy,
            onset_ok,
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
                // Early commit: once we have the minimum valid window and a
                // rock-solid engine tempo lock, don't make the user wait out
                // the full 8s ring — the boom shape is already knowable.
                let early_ok = valid_ms >= LISTEN_MIN_VALID_MS
                    && elapsed >= LISTEN_MIN_VALID_MS
                    && engine_tempo_confidence >= 0.70;
                if ring_full || elapsed >= LISTEN_BUDGET_MS || early_ok {
                    if valid_ms >= LISTEN_MIN_VALID_MS {
                        self.try_commit(now_ms, engine_tempo_confidence, settings);
                    } else if elapsed >= LISTEN_BUDGET_MS {
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
            if pair[1].t_ms < self.listen_from_ms {
                continue;
            }
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
        let features = match self.estimate(self.listen_from_ms) {
            Ok(f) => f,
            Err(reason) => {
                // Early-commit probes can fail before enough onsets land;
                // stay listening unless the budget is exhausted.
                if let AutoLockState::Listening { started_ms } = self.state {
                    if now_ms - started_ms < LISTEN_BUDGET_MS {
                        return;
                    }
                }
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
        // Crest rewards material with real hit/trough contrast (the boom fuel).
        let crest_norm = ((features.crest - 1.0) / 5.0).clamp(0.0, 1.0);
        let score = 0.45 * conf
            + 0.25 * margin_norm
            + 0.15 * (1.0 - features.silence_ratio)
            + 0.15 * crest_norm;

        if conf < 0.35 || score < MIN_LOCK_SCORE {
            if let AutoLockState::Listening { started_ms } = self.state {
                if now_ms - started_ms < LISTEN_BUDGET_MS {
                    return; // keep listening; early probe wasn't ready
                }
            }
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
        settings.climax_mode_enabled = false;
        settings.current_preset_name = String::from("Auto Boom");
        settings.sanitize();
        self.expected = Some(LockParams::capture(settings));
        self.glide = Some(Glide {
            started_ms: now_ms,
            from,
            to: target,
        });

        let beat_ms = Self::fold_to_perceptual_beat(features.ioi_median);
        let bpm = if beat_ms > 1.0 {
            60_000.0 / beat_ms
        } else {
            0.0
        };
        self.last_report = Some(LockReport {
            bpm,
            band_label: band_label(features.best_band),
            decay_ms: target.decay_ms,
            gate_threshold: target.gate_threshold,
            binary_level: target.binary_level,
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

    /// Estimate features from ring data captured at or after `from_ms`
    /// (0.0 = the whole ring). A fresh listen must not re-judge stale audio.
    fn estimate(&self, from_ms: f32) -> Result<Features, &'static str> {
        let frames: Vec<&FrameSample> = self.frames.iter().filter(|f| f.t_ms >= from_ms).collect();
        if frames.len() < 40 {
            return Err("not enough audio");
        }

        // Silence ratio over the window
        let silent = frames.iter().filter(|f| !f.valid).count();
        let silence_ratio = silent as f32 / frames.len() as f32;

        // Inter-onset intervals, filtered to a plausible beat range
        let onsets: Vec<f32> = self
            .onsets
            .iter()
            .copied()
            .filter(|&o| o >= from_ms)
            .collect();
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

        // Per-band PUNCH: the objective is the biggest per-hit impact, not
        // merely the most rhythmic band. For each band:
        //   hit height  = median over onsets of the max energy jump inside
        //                 the onset-aligned window (how hard each hit lands)
        //   floor       = median energy jump on non-aligned frames (the mush
        //                 between hits that dilutes the contrast)
        //   reliability = fraction of onsets where the band actually moved
        // punch = hit_height * reliability / (floor + regularizer). A kick
        // with huge jumps out of a quiet floor dominates; a loud-but-ticky
        // hi-hat or a band smeared by a bassline loses.
        let n = frames.len();
        let mut delta_rows: Vec<[f32; N_BANDS]> = Vec::with_capacity(n.saturating_sub(1));
        let mut aligned_rows: Vec<bool> = Vec::with_capacity(n.saturating_sub(1));
        for pair in frames.windows(2) {
            let (prev, cur) = (pair[0], pair[1]);
            let mut row = [0.0f32; N_BANDS];
            for (b, slot) in row.iter_mut().enumerate() {
                *slot = (cur.band_energies[b] - prev.band_energies[b]).max(0.0);
            }
            delta_rows.push(row);
            aligned_rows.push(
                onsets
                    .iter()
                    .any(|&o| cur.t_ms >= o && cur.t_ms - o <= ONSET_ALIGN_MS),
            );
        }

        let mut punch = [0.0f32; N_BANDS];
        for b in 0..N_BANDS {
            // Per-onset peak jump in this band
            let mut hit_peaks: Vec<f32> = Vec::with_capacity(onsets.len());
            for &o in &onsets {
                let mut peak = 0.0f32;
                let mut in_window = false;
                for (row, pair) in delta_rows.iter().zip(frames.windows(2)) {
                    let t = pair[1].t_ms;
                    if t >= o && t - o <= ONSET_ALIGN_MS {
                        in_window = true;
                        peak = peak.max(row[b]);
                    } else if t > o + ONSET_ALIGN_MS {
                        break;
                    }
                }
                if in_window {
                    hit_peaks.push(peak);
                }
            }
            if hit_peaks.is_empty() {
                continue;
            }
            hit_peaks.sort_by(|a, c| a.partial_cmp(c).unwrap());
            let hit_height = percentile_sorted(&hit_peaks, 0.5);
            let reliability = hit_peaks.iter().filter(|&&p| p > 1e-4).count() as f32
                / hit_peaks.len().max(1) as f32;

            // Between-hit activity: MEAN of non-aligned deltas (median is
            // almost always exactly 0 for rectified deltas, which made every
            // band look perfectly clean).
            let (mut floor_sum, mut floor_n) = (0.0f32, 0u32);
            for (row, &a) in delta_rows.iter().zip(aligned_rows.iter()) {
                if !a {
                    floor_sum += row[b];
                    floor_n += 1;
                }
            }
            let floor = floor_sum / floor_n.max(1) as f32;

            // Punch = absolute per-hit impact, scaled by consistency and by a
            // BOUNDED contrast bonus (1.0 for a silent floor, 0.25 when the
            // between-hit mush equals the hit itself). Absolute hit height
            // stays in charge, so the kick's big jump beats a clean-but-tiny
            // treble tick.
            let contrast = hit_height / (hit_height + 3.0 * floor + 1e-6);
            punch[b] = hit_height * reliability * contrast;
        }

        let mut order: Vec<usize> = (0..N_BANDS).collect();
        order.sort_by(|&a, &b| punch[b].partial_cmp(&punch[a]).unwrap());
        let best_band = order[0];
        let salience_margin = if punch[order[1]] > 1e-6 {
            punch[best_band] / punch[order[1]]
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

        // Gate fit: pre-volume energy on hits vs between hits. Threshold sits
        // just above the trough so kicks open the gate and silence closes it
        // — the single biggest lever for dynamic contrast.
        let mut hit_e: Vec<f32> = Vec::new();
        let mut floor_e: Vec<f32> = Vec::new();
        for f in &frames {
            if !f.valid {
                continue;
            }
            let on_hit = onsets
                .iter()
                .any(|&o| f.t_ms >= o && f.t_ms - o <= ONSET_ALIGN_MS);
            if on_hit {
                hit_e.push(f.pre_volume_energy);
            } else {
                floor_e.push(f.pre_volume_energy);
            }
        }
        hit_e.sort_by(|a, b| a.partial_cmp(b).unwrap());
        floor_e.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let energy_hit = if hit_e.is_empty() {
            percentile_sorted(&energies, 0.90)
        } else {
            percentile_sorted(&hit_e, 0.50)
        };
        let energy_floor = if floor_e.is_empty() {
            percentile_sorted(&energies, 0.20)
        } else {
            percentile_sorted(&floor_e, 0.50)
        };

        Ok(Features {
            ioi_median,
            ioi_iqr,
            best_band,
            salience_margin,
            crest,
            centroid_median,
            silence_ratio,
            energy_hit,
            energy_floor,
        })
    }

    /// Fold a raw inter-onset interval into the perceptual beat octave
    /// (70-180 BPM => 333-857ms). Onset detectors track the subdivision grid
    /// (verified on real material: a 125 BPM track detects as a rock-solid
    /// 250 BPM eighth-note grid), but the envelope must breathe with the
    /// FELT beat, not the subdivision.
    fn fold_to_perceptual_beat(ioi_ms: f32) -> f32 {
        let mut t = ioi_ms;
        if t <= 0.0 {
            return t;
        }
        while t < 333.0 {
            t *= 2.0;
        }
        while t > 857.0 {
            t /= 2.0;
        }
        t
    }

    /// Feature -> parameter mapping. Always produces the bass-drum boom
    /// shape; crest / band / gate decide *how hard* and *where*, never
    /// whether we boom. Intensity fields stay under the user's ceiling.
    fn map_features(f: &Features, settings: &Settings) -> LockParams {
        let t = Self::fold_to_perceptual_beat(f.ioi_median);

        // Frequency focus. Clear winner → tight band. Ambiguous but punchy /
        // low-band → still kick-lock (Full smears hats into the boom).
        let (frequency_mode, target_frequency) = if f.salience_margin >= SALIENCE_MARGIN {
            match f.best_band {
                0 => (FrequencyMode::LowPass, BAND_EDGES[1]), // sub only
                1 => (FrequencyMode::LowPass, BAND_EDGES[2]), // sub+bass
                2 | 3 => (
                    FrequencyMode::BandPass,
                    (BAND_EDGES[f.best_band] * BAND_EDGES[f.best_band + 1]).sqrt(),
                ),
                b => (FrequencyMode::HighPass, BAND_EDGES[b]),
            }
        } else if f.crest >= 2.0 || f.best_band <= 1 {
            (FrequencyMode::LowPass, BAND_EDGES[2])
        } else {
            (FrequencyMode::Full, settings.target_frequency)
        };

        // Trigger: max dynamic delivery. Kick-band or punchy crest → Hybrid
        // with a strong binary thump. Flat material stays Dynamic on the same
        // boom envelope.
        let kick_band = f.best_band <= 1;
        let (trigger_mode, hybrid_blend, dynamic_curve) = if kick_band || f.crest >= 2.0 {
            let blend = if f.crest >= 4.0 {
                0.72
            } else if f.crest >= 2.5 {
                0.62
            } else {
                0.55
            };
            (TriggerMode::Hybrid, blend, 1.15)
        } else if f.crest >= 1.5 {
            (TriggerMode::Hybrid, 0.45, 1.35)
        } else {
            (TriggerMode::Dynamic, settings.hybrid_blend, 1.7)
        };

        // Punch from MATERIAL crest + kick boost.
        // Cap 0.88 leaves overshoot headroom; floor 0.70 always thumps.
        let crest_n = ((f.crest - 1.0) / 5.0).clamp(0.0, 1.0);
        let band_boost = if kick_band { 0.06 } else { 0.0 };
        let binary_level = (0.72 + 0.12 * crest_n + band_boost).clamp(0.70, 0.88);

        // Bass-drum body: instant peak → ~78% of beat exp decay → near-zero floor.
        let decay_ms = (0.78 * t).clamp(80.0, 1600.0);
        let sustain_target = 0.08;
        let release_target = (0.50 * t).clamp(80.0, 1200.0);

        // Engine-exact centroid pre-compensation (audio.rs linear cn formula).
        let cn = ((f.centroid_median - 100.0) / 4000.0).clamp(0.0, 1.0);
        let sustain_level = (sustain_target / (1.0 - 0.25 * cn)).clamp(0.0, 1.0);
        let release_ms = (release_target / (1.0 + 0.4 * (1.0 - cn))).clamp(0.5, 5000.0);

        // Gate just above the between-hit floor so kicks open and troughs close.
        let span = (f.energy_hit - f.energy_floor).max(0.0);
        let mut gate_threshold = f.energy_floor + span * 0.22;
        let cap = (f.energy_hit * 0.55).max(0.02);
        gate_threshold = gate_threshold.clamp(0.02, cap).clamp(0.02, 0.45);
        let gate_smoothing = if f.crest >= 2.5 { 0.04 } else { 0.08 };

        let output_slew_ms = (0.10 * t).clamp(30.0, 55.0);

        LockParams {
            frequency_mode,
            target_frequency,
            trigger_mode,
            binary_level,
            hybrid_blend,
            threshold_knee: 0.15,
            dynamic_curve,
            attack_ms: 20.0,
            decay_ms,
            sustain_level,
            release_ms,
            attack_curve: 0.7,
            decay_curve: 1.8,
            release_curve: 1.3,
            output_slew_ms,
            gate_threshold,
            gate_smoothing,
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
    silence_ratio: f32,
    energy_hit: f32,
    energy_floor: f32,
}

fn band_label(band: usize) -> &'static str {
    match band {
        0 => "Sub",
        1 => "Bass",
        2 => "Lo-Mid",
        3 => "Mid",
        4 => "Hi-Mid",
        5 => "Pres",
        6 => "Brill",
        _ => "Air",
    }
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

    // ---- Offline beat-detection harness on real material ----
    //
    // Run: cargo test --release analyze_real_wav -- --ignored --nocapture
    //
    // Feeds a real WAV through the real SpectralAnalyzer -> Gate ->
    // BeatDetector -> AutoLock estimator under three cadence models:
    //   designed-47Hz    one fresh 2048 window per 1024-sample hop (the
    //                    cadence the detector's frame-based statistics assume)
    //   capture-100Hz    the actual capture-thread behavior: ~10ms poll of a
    //                    rolling buffer, so consecutive FFT windows overlap
    //                    ~80% and flux shrinks accordingly
    //   live-uiDup       capture-100Hz plus the UI loop re-processing each
    //                    stored spectral frame 2-3x (measured ~240fps)
    // This isolates whether beat-detection failures are algorithmic or
    // cadence-induced.
    #[test]
    #[ignore]
    fn analyze_real_wav() {
        let path = std::env::var("CHLOE_WAV")
            .unwrap_or_else(|_| r"C:\Users\coldb\Downloads\STA.wav".to_string());
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("SKIP: {path} not found");
            return;
        };
        let (mono, rate) = parse_wav_mono(&bytes);
        eprintln!(
            "loaded {path}: {} samples @ {rate} Hz ({:.1}s)",
            mono.len(),
            mono.len() as f32 / rate as f32
        );
        run_cadence_model("designed-47Hz", &mono, rate as f32, 1024, 1);
        run_cadence_model("capture-100Hz", &mono, rate as f32, 480, 1);
        run_cadence_model("live-uiDup", &mono, rate as f32, 480, 0);
    }

    /// Minimal RIFF/WAVE PCM16 parser -> mono f32.
    fn parse_wav_mono(bytes: &[u8]) -> (Vec<f32>, u32) {
        let mut pos = 12usize;
        let mut channels = 2usize;
        let mut rate = 48_000u32;
        let mut data: &[u8] = &[];
        while pos + 8 <= bytes.len() {
            let id = &bytes[pos..pos + 4];
            let sz = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
            let end = (pos + 8 + sz).min(bytes.len());
            let body = &bytes[pos + 8..end];
            if id == b"fmt " && body.len() >= 8 {
                channels = u16::from_le_bytes(body[2..4].try_into().unwrap()) as usize;
                rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            }
            if id == b"data" {
                data = body;
                break;
            }
            pos += 8 + sz + (sz & 1);
        }
        let ch = channels.max(1);
        let mut mono = Vec::with_capacity(data.len() / (2 * ch));
        for frame in data.chunks_exact(2 * ch) {
            let mut acc = 0f32;
            for c in 0..ch {
                acc += i16::from_le_bytes([frame[2 * c], frame[2 * c + 1]]) as f32 / 32768.0;
            }
            mono.push(acc / ch as f32);
        }
        (mono, rate)
    }

    /// ui_ticks: 1 = one pipeline pass per capture frame; 0 = simulate the
    /// live UI loop (2-3 passes per capture frame, ~240fps vs ~100Hz capture).
    fn run_cadence_model(mode: &str, mono: &[f32], rate: f32, hop: usize, ui_ticks: usize) {
        use crate::audio::{BeatDetector, FrequencyMode, Gate, SpectralAnalyzer};

        let mut analyzer = SpectralAnalyzer::new(rate);
        let mut gate = Gate::new();
        let mut beat = BeatDetector::new();
        let mut al = AutoLock::new();
        let mut settings = Settings {
            gate_threshold: 0.17, // field-reported user setting
            ..Default::default()
        };
        al.on_button(0.0);

        let main_volume = 1.15f32;
        let normalize = |v: f32| ((v * 6.0).clamp(0.0, 1.0)).powf(0.65).clamp(0.0, 1.0);

        let hop_ms = hop as f32 / rate * 1000.0;
        let mut onset_times: Vec<f32> = Vec::new();
        let mut last_state = format!("{:?}", al.state);
        let mut pos = 2048usize;
        let mut t_ms = 0.0f32;
        let mut frame_idx = 0u64;

        while pos + hop <= mono.len() {
            pos += hop;
            t_ms += hop_ms;
            frame_idx += 1;
            let sd = analyzer.analyze(&mono[pos - 2048..pos], 1);

            let ticks = if ui_ticks == 0 {
                // ~240fps UI over ~100Hz capture: alternate 2 and 3 passes
                if frame_idx % 5 < 2 {
                    3
                } else {
                    2
                }
            } else {
                ui_ticks
            };

            for k in 0..ticks {
                let tick_ms = t_ms + k as f32 * (hop_ms / ticks as f32);
                let energy_raw = SpectralAnalyzer::extract_energy(&sd, FrequencyMode::Full, 200.0);
                let normalized = normalize(energy_raw);
                let energy = (normalized * main_volume).clamp(0.0, 1.0);
                let _gate_open = gate.process(normalized, settings.gate_threshold, 0.0, 0.22);
                let (onset, strength) = beat.process(sd.spectral_flux, tick_ms);
                let onset_ok = onset && strength > 1.02 && energy > settings.gate_threshold * 0.40;
                if onset_ok {
                    onset_times.push(tick_ms);
                }
                al.tick(
                    tick_ms,
                    &sd,
                    normalized,
                    onset_ok,
                    energy,
                    false,
                    beat.tempo_confidence,
                    &mut settings,
                );
            }

            let state_now = format!("{:?}", al.state);
            if state_now != last_state {
                eprintln!("[{mode}] t={:.1}s state -> {state_now}", t_ms / 1000.0);
                last_state = state_now;
            }
        }

        let mut iois: Vec<f32> = onset_times
            .windows(2)
            .map(|w| w[1] - w[0])
            .filter(|d| (150.0..=2000.0).contains(d))
            .collect();
        iois.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let (med, iqr) = if iois.len() >= 2 {
            (
                percentile_sorted(&iois, 0.5),
                percentile_sorted(&iois, 0.75) - percentile_sorted(&iois, 0.25),
            )
        } else {
            (0.0, 0.0)
        };
        let dur_s = t_ms / 1000.0;
        eprintln!(
            "[{mode}] onsets={} ({:.2}/s) ioi_med={:.0}ms (={:.1} bpm) iqr={:.0}ms conf_end={:.2} tempo_int={:.0}ms",
            onset_times.len(),
            onset_times.len() as f32 / dur_s.max(0.1),
            med,
            if med > 0.0 { 60_000.0 / med } else { 0.0 },
            iqr,
            beat.tempo_confidence,
            beat.tempo_interval_ms,
        );
        match al.estimate(0.0) {
            Ok(f) => {
                eprintln!(
                    "[{mode}] estimate: ioi_med={:.0} iqr={:.0} band={} margin={:.2} crest={:.2} silence={:.2}",
                    f.ioi_median,
                    f.ioi_iqr,
                    f.best_band,
                    f.salience_margin,
                    f.crest,
                    f.silence_ratio,
                );
                let folded = AutoLock::fold_to_perceptual_beat(f.ioi_median);
                let p = AutoLock::map_features(&f, &settings);
                eprintln!(
                    "[{mode}] fitted: beat={:.0}ms (folded from {:.0}) mode={:?} target={:.0}Hz decay={:.0}ms sustain={:.2} release={:.0}ms slew={:.0}ms trig={:?}",
                    folded,
                    f.ioi_median,
                    p.frequency_mode,
                    p.target_frequency,
                    p.decay_ms,
                    p.sustain_level,
                    p.release_ms,
                    p.output_slew_ms,
                    p.trigger_mode
                );
            }
            Err(reason) => eprintln!("[{mode}] estimate: ERR {reason}"),
        }
        eprintln!("[{mode}] final autolock state: {:?}\n", al.state);
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
        // Bass-drum waveform: the decay spans most of the beat and LANDS
        // (envelope in Sustain, the only retrigger state) with jitter margin
        // before the next hit — never a flat gap, never a truncated cut.
        assert!(
            settings.decay_ms >= 0.70 * 500.0 && settings.decay_ms <= 0.85 * 500.0,
            "decay {} must span ~78% of the 500ms beat",
            settings.decay_ms
        );
        assert!(settings.attack_ms < 50.0, "attack must take the fast path");
    }

    #[test]
    fn never_touches_ceiling_fields() {
        let mut probe = Settings {
            main_volume: 1.234,
            output_gain: 0.777,
            min_vibe: 0.111,
            max_vibe: 0.888,
            climax_intensity: 0.321,
            climax_mode_enabled: true,
            ..Default::default()
        };

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
        // Consent ceiling: volume / gain / range / climax intensity untouched.
        assert_eq!(probe.main_volume, 1.234);
        assert_eq!(probe.output_gain, 0.777);
        assert_eq!(probe.min_vibe, 0.111);
        assert_eq!(probe.max_vibe, 0.888);
        assert_eq!(probe.climax_intensity, 0.321);
        // Boom path forces climax off; gate is fitted (not frozen).
        assert!(!probe.climax_mode_enabled);
        assert!(probe.gate_threshold > 0.0);
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
    fn retry_after_no_lock_listens_fresh_and_can_lock() {
        let mut al = AutoLock::new();
        let mut settings = Settings::default();
        al.on_button(0.0);

        // 16s of silence -> NO LOCK
        let mut now = 0.0f32;
        while now < 16_000.0 {
            now += 20.0;
            let bands = [(now % 13.0) * 1e-6; N_BANDS];
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
        assert!(matches!(al.state, AutoLockState::NoLock { .. }));

        // Retry: must actually LISTEN to fresh audio, not instantly re-judge
        // the stale ring and bounce back to NoLock (the dead-button bug).
        al.on_button(now);
        now += 20.0;
        al.tick(
            now,
            &spectral_with_bands([2e-5; N_BANDS]),
            0.0001,
            false,
            0.0,
            false,
            0.0,
            &mut settings,
        );
        assert!(
            matches!(al.state, AutoLockState::Listening { .. }),
            "retry must re-listen, got {:?}",
            al.state
        );

        // Feed real beats after the retry -> must reach Locked
        let mut next_beat = now + 100.0;
        let end = now + 16_000.0;
        while now < end {
            now += 20.0;
            let mut bands = [0.02f32; N_BANDS];
            bands[7] = 0.02 + (now / 900.0).sin().abs() * 0.001;
            let mut onset = false;
            if now >= next_beat {
                bands[1] = 0.8;
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
        assert!(
            al.is_locked(),
            "retry on good material must lock, got {:?}",
            al.state
        );
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
            silence_ratio: 0.0,
            energy_hit: 0.5,
            energy_floor: 0.05,
        };
        let s = Settings::default();
        let dark = AutoLock::map_features(&f, &s);
        f.centroid_median = 4_100.0; // cn = 1 (bright)
        let bright = AutoLock::map_features(&f, &s);

        let target = (0.50f32 * 500.0).clamp(80.0, 1200.0);
        // Dark: engine multiplies by 1.4 -> written must be target/1.4
        assert!((dark.release_ms * 1.4 - target).abs() < 0.5);
        // Bright: engine multiplies by 1.0 -> written == target
        assert!((bright.release_ms - target).abs() < 0.5);
        // Sustain compensation direction: dark writes as-is, bright writes higher
        assert!(bright.sustain_level > dark.sustain_level);
    }

    #[test]
    fn binary_level_punch_policy() {
        // Punch comes from crest + kick-band. Floor 0.70,
        // cap 0.88 (overshoot headroom). User max/gain/multiplier still bind.
        let mut f = Features {
            ioi_median: 400.0,
            ioi_iqr: 15.0,
            best_band: 1, // kick band
            salience_margin: 2.0,
            crest: 1.5, // mild
            centroid_median: 1000.0,
            silence_ratio: 0.0,
            energy_hit: 0.4,
            energy_floor: 0.05,
        };
        let s = Settings::default();
        let mild = AutoLock::map_features(&f, &s);
        assert_eq!(mild.trigger_mode, TriggerMode::Hybrid);
        assert!(
            mild.binary_level >= 0.70 && mild.binary_level <= 0.88,
            "binary {}",
            mild.binary_level
        );

        f.crest = 6.0; // very punchy → near cap
        let loud = AutoLock::map_features(&f, &s);
        assert!(
            (loud.binary_level - 0.88).abs() < 1e-3,
            "binary {}",
            loud.binary_level
        );
        // Gate sits between floor and hit
        assert!(loud.gate_threshold > f.energy_floor);
        assert!(loud.gate_threshold < f.energy_hit);
        // Always boom body
        assert!(loud.sustain_level < 0.2);
        assert!(loud.attack_ms < 50.0);
        assert!((loud.decay_curve - 1.8).abs() < 1e-3);
    }

    #[test]
    fn weak_salience_with_kick_still_boom_locks() {
        // Bass + presence pulse together → no clear winner, but kick energy
        // + crest still LowPass (Full would smear hats into the boom).
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
        assert_eq!(settings.frequency_mode, FrequencyMode::LowPass);
        // Boom body always
        assert!(settings.sustain_level < 0.2);
        assert!(settings.decay_ms > 200.0);
    }
}
