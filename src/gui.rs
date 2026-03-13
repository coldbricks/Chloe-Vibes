// ==========================================================================
// gui.rs — Main Application GUI and Processing Pipeline
//
// Major changes from original:
//   - Capture thread now produces SpectralData via FFT (not just RMS)
//   - New processing pipeline: Spectral → Gate → Trigger → ADSR Envelope
//   - Device tasks read processed output (not raw audio power)
//   - UI panels for all new ChloeVibes-derived controls
//   - Toggle to switch between legacy and advanced processing
// ==========================================================================

use std::{
    collections::{HashMap, VecDeque},
    iter::from_fn,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use audio_capture::win::capture::AudioCapture;
use buttplug::{
    client::{ButtplugClient, ButtplugClientDevice, ButtplugClientError, ScalarValueCommand},
    core::message::ActuatorType,
};
use clap::Parser;
use eframe::{
    egui::{
        self, Button, Color32, ComboBox, CornerRadius, ProgressBar, RichText, Slider, Stroke,
        StrokeKind, TextFormat, Ui, Visuals, Window,
    },
    epaint::{pos2, text::LayoutJob, vec2, FontId, Pos2, Rect, Shape},
    CreationContext, Storage,
};
use tokio::runtime::Runtime;

use crate::{
    audio::{
        self, BeatDetector, ClimaxEngine, ClimaxPattern, EnvelopeProcessor, EnvelopeState,
        FrequencyMode, Gate, SharedSpectralData, SpectralAnalyzer, SpectralData, TriggerMode,
        BAND_NAMES,
    },
    presets::{self, PresetCategory},
    settings::{defaults, DeviceSettings, OscillatorSettings, Settings, VibratorSettings},
    util::{self, MinCutoff, SharedF32},
};

// ---------------------------------------------------------------------------
// CLI Arguments
// ---------------------------------------------------------------------------

#[derive(Parser, Default)]
pub struct Gui {
    #[clap(short, long)]
    server_addr: Option<String>,
}

pub fn gui(args: Gui) {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Chloe Vibes",
        native_options,
        Box::new(|ctx| Ok(Box::new(GuiApp::new(args.server_addr, ctx)))),
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// Connection State
// ---------------------------------------------------------------------------

#[allow(dead_code)]
enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

// ---------------------------------------------------------------------------
// Device structs (unchanged from original)
// ---------------------------------------------------------------------------

struct Device {
    props: Arc<Mutex<DeviceProps>>,
    _task: tokio::task::JoinHandle<()>,
}

struct DeviceProps {
    is_enabled: bool,
    battery_state: BatteryState,
    multiplier: f32,
    min: f32,
    max: f32,
    vibrators: Vec<VibratorProps>,
    oscillators: Vec<OscillatorProps>,
}

#[allow(dead_code)]
struct BatteryState {
    shared_level: SharedF32,
    _task: tokio::task::JoinHandle<()>,
}

impl BatteryState {
    pub fn new(runtime: &Runtime, device: Arc<ButtplugClientDevice>) -> Self {
        let shared_level = SharedF32::new(0.0);
        let task = {
            let shared_level = shared_level.clone();
            runtime.spawn(battery_check_bg_task(device, shared_level))
        };
        Self {
            shared_level,
            _task: task,
        }
    }

    pub fn get_level(&self) -> Option<f32> {
        let value = self.shared_level.load();
        if value.is_nan() {
            None
        } else {
            Some(value)
        }
    }
}

async fn battery_check_bg_task(device: Arc<ButtplugClientDevice>, shared_level: SharedF32) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        match device.battery_level().await {
            Ok(level) => shared_level.store(level as f32),
            Err(_) => {
                shared_level.store(f32::NAN);
                break;
            }
        }
    }
}

impl DeviceProps {
    fn new(runtime: &Runtime, device: Arc<ButtplugClientDevice>, settings: &Settings) -> Self {
        let mut vibe_count = 0;
        let mut osci_count = 0;
        if let Some(messages) = device.message_attributes().scalar_cmd().as_ref() {
            for message in messages {
                match message.actuator_type() {
                    ActuatorType::Vibrate => vibe_count += 1,
                    ActuatorType::Oscillate => osci_count += 1,
                    _ => (),
                }
            }
        }

        let device_settings = if settings.save_device_settings {
            settings.device_settings.get(device.name().as_str())
        } else {
            None
        };

        let (_is_enabled, multiplier, min, max, vibrators, oscillators) =
            if let Some(ds) = device_settings {
                let mut vibrators = Vec::new();
                for vs in &ds.vibrators {
                    vibrators.push(VibratorProps {
                        is_enabled: vs.is_enabled,
                        multiplier: vs.multiplier,
                        min: vs.min,
                        max: vs.max,
                    });
                }
                while vibrators.len() < vibe_count {
                    vibrators.push(VibratorProps::default());
                }
                let mut oscillators = Vec::new();
                for osc in &ds.oscillators {
                    oscillators.push(OscillatorProps {
                        is_enabled: osc.is_enabled,
                        multiplier: osc.multiplier,
                        min: osc.min,
                        max: osc.max,
                    });
                }
                while oscillators.len() < osci_count {
                    oscillators.push(OscillatorProps::default());
                }
                (
                    ds.is_enabled,
                    ds.multiplier,
                    ds.min,
                    ds.max,
                    vibrators,
                    oscillators,
                )
            } else {
                let vibrators = from_fn(|| Some(VibratorProps::default()))
                    .take(vibe_count)
                    .collect();
                let oscillators = from_fn(|| Some(OscillatorProps::default()))
                    .take(osci_count)
                    .collect();
                (false, 1.0, 0.0, 1.0, vibrators, oscillators)
            };
        Self {
            is_enabled: false,
            battery_state: BatteryState::new(runtime, device),
            multiplier,
            min,
            max,
            vibrators,
            oscillators,
        }
    }
}

impl DeviceProps {
    fn calculate_visual_output(&self, input: f32) -> (f32, bool) {
        let power = (input * self.multiplier).clamp(0.0, self.max);
        (power, power < self.min)
    }

    fn calculate_output(&self, input: f32) -> f32 {
        (input * self.multiplier)
            .clamp(0.0, self.max)
            .min_cutoff(self.min)
    }
}

// ---------------------------------------------------------------------------
// Main Application State
// ---------------------------------------------------------------------------

struct GuiApp {
    runtime: tokio::runtime::Runtime,
    client: Option<ButtplugClient>,
    connection_state: ConnectionState,
    connection_task: Option<tokio::task::JoinHandle<Result<ButtplugClient, ButtplugClientError>>>,
    server_addr: Option<String>,
    server_name: String,
    capture_status: Arc<Mutex<String>>,
    devices: HashMap<String, Device>,

    // Audio data from capture thread
    sound_power: SharedF32,            // Legacy: simple RMS power
    spectral_data: SharedSpectralData, // NEW: full spectral analysis

    // Processed output that devices read
    processed_output: SharedF32, // NEW: final output after envelope/gate

    _capture_thread: JoinHandle<()>,
    is_scanning: bool,
    show_settings: bool,

    // Processing state
    vibration_level: f32,
    hold_start_time: Option<Instant>,

    // NEW: Signal processors (from ChloeVibes)
    gate: Gate,
    envelope: EnvelopeProcessor,
    beat_detector: BeatDetector,
    climax_engine: ClimaxEngine,

    // NEW: cached spectral data for UI display
    last_spectral: SpectralData,
    gate_is_open: bool,
    climax_phase: f32,

    // Visualization history buffers
    output_history: VecDeque<f32>,
    energy_history: VecDeque<f32>,
    input_level: f32,
    smoothed_energy: f32,
    raw_energy: f32,
    using_rms_fallback: bool,
    output_delay: VecDeque<f32>,
    tap_tempo: TapTempo,
    quantize_enabled: bool,
    beat_sync_mode: BeatSyncMode,
    quantize_division: BeatDivision,

    // Preset UI state
    selected_preset_category: PresetCategory,

    // Logo texture (optional — app works fine without it)
    logo_texture: Option<egui::TextureHandle>,

    settings: Settings,
}

// ---------------------------------------------------------------------------
// Color palette — dark theme with accent colors
// ---------------------------------------------------------------------------

mod palette {
    use eframe::egui::Color32;

    pub const BG_PRIMARY: Color32 = Color32::from_rgb(15, 15, 20);
    pub const BG_SECONDARY: Color32 = Color32::from_rgb(22, 22, 32);
    pub const BG_TERTIARY: Color32 = Color32::from_rgb(30, 30, 42);
    pub const BG_CARD: Color32 = Color32::from_rgb(26, 26, 38);
    pub const ACCENT_PURPLE: Color32 = Color32::from_rgb(124, 58, 237);
    pub const ACCENT_PURPLE_DIM: Color32 = Color32::from_rgb(88, 40, 168);
    pub const ACCENT_TEAL: Color32 = Color32::from_rgb(16, 185, 129);
    pub const ACCENT_TEAL_DIM: Color32 = Color32::from_rgb(10, 120, 84);
    pub const ACCENT_RED: Color32 = Color32::from_rgb(239, 68, 68);
    pub const ACCENT_PINK: Color32 = Color32::from_rgb(236, 72, 153);
    pub const ACCENT_PINK_DIM: Color32 = Color32::from_rgb(160, 50, 110);
    pub const ACCENT_AMBER: Color32 = Color32::from_rgb(245, 158, 11);
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(243, 244, 246);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(156, 163, 175);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(107, 114, 128);
    pub const GRID_LINE: Color32 = Color32::from_rgb(40, 40, 55);
    pub const NEON_BORDER: Color32 = Color32::from_rgba_premultiplied(124, 58, 237, 60);
    pub const NEON_BORDER_ACTIVE: Color32 = Color32::from_rgba_premultiplied(236, 72, 153, 90);
}

// ---------------------------------------------------------------------------
// Visualization constants
// ---------------------------------------------------------------------------

const HISTORY_LEN: usize = 256;
const ADSR_PREVIEW_HEIGHT: f32 = 100.0;
const OUTPUT_HISTORY_HEIGHT: f32 = 80.0;
const SPECTRUM_HEIGHT: f32 = 70.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BeatDivision {
    Half,
    Quarter,
    Eighth,
}

impl BeatDivision {
    fn period_multiplier(self) -> f32 {
        match self {
            Self::Half => 2.0,
            Self::Quarter => 1.0,
            Self::Eighth => 0.5,
        }
    }

    fn pulse_sharpness(self) -> f32 {
        match self {
            Self::Half => 2.0,
            Self::Quarter => 2.4,
            Self::Eighth => 2.9,
        }
    }

    fn accent_width(self) -> f32 {
        match self {
            Self::Half => 0.28,
            Self::Quarter => 0.22,
            Self::Eighth => 0.16,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BeatSyncMode {
    Tap,
    Auto,
    Hybrid,
}

impl BeatSyncMode {
    fn allows_auto(self) -> bool {
        matches!(self, Self::Auto | Self::Hybrid)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Tap => "Tap",
            Self::Auto => "Auto",
            Self::Hybrid => "Hybrid",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BeatSyncSource {
    None,
    Tap,
    Auto,
    Hybrid,
}

impl BeatSyncSource {
    fn label(self) -> &'static str {
        match self {
            Self::None => "--",
            Self::Tap => "Tap",
            Self::Auto => "Auto",
            Self::Hybrid => "Hybrid",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct BeatSyncState {
    bpm: Option<f32>,
    confidence: f32,
    source: BeatSyncSource,
}

struct TapTempo {
    manual_timestamps_ms: VecDeque<f32>,
    auto_timestamps_ms: VecDeque<f32>,
    manual_bpm: Option<f32>,
    auto_bpm: Option<f32>,
    manual_confidence: f32,
    auto_confidence: f32,
    manual_last_ms: Option<f32>,
    auto_last_ms: Option<f32>,
    phase_anchor_ms: Option<f32>,
    active_bpm: Option<f32>,
    active_confidence: f32,
    active_source: BeatSyncSource,
}

impl TapTempo {
    const MAX_TAPS: usize = 10;
    const MAX_AUTO_HITS: usize = 24;
    const TAP_RESET_GAP_MS: f32 = 2200.0;
    const AUTO_RESET_GAP_MS: f32 = 1800.0;
    const MIN_INTERVAL_MS: f32 = 120.0;
    const MAX_INTERVAL_MS: f32 = 2000.0;
    const MIN_BPM: f32 = 45.0;
    const MAX_BPM: f32 = 220.0;
    const MANUAL_STALE_MS: f32 = 9000.0;
    const AUTO_STALE_MS: f32 = 5000.0;

    fn new() -> Self {
        Self {
            manual_timestamps_ms: VecDeque::with_capacity(Self::MAX_TAPS),
            auto_timestamps_ms: VecDeque::with_capacity(Self::MAX_AUTO_HITS),
            manual_bpm: None,
            auto_bpm: None,
            manual_confidence: 0.0,
            auto_confidence: 0.0,
            manual_last_ms: None,
            auto_last_ms: None,
            phase_anchor_ms: None,
            active_bpm: None,
            active_confidence: 0.0,
            active_source: BeatSyncSource::None,
        }
    }

    fn reset(&mut self) {
        self.manual_timestamps_ms.clear();
        self.auto_timestamps_ms.clear();
        self.manual_bpm = None;
        self.auto_bpm = None;
        self.manual_confidence = 0.0;
        self.auto_confidence = 0.0;
        self.manual_last_ms = None;
        self.auto_last_ms = None;
        self.phase_anchor_ms = None;
        self.active_bpm = None;
        self.active_confidence = 0.0;
        self.active_source = BeatSyncSource::None;
    }

    fn tap_now(&mut self, current_time_ms: f32, mode: BeatSyncMode) {
        if let Some(previous_ms) = self.manual_last_ms {
            let delta_ms = current_time_ms - previous_ms;
            if delta_ms < Self::MIN_INTERVAL_MS * 0.6 {
                return;
            }
            if delta_ms > Self::TAP_RESET_GAP_MS {
                self.manual_timestamps_ms.clear();
            }
        }

        self.manual_last_ms = Some(current_time_ms);
        self.manual_timestamps_ms.push_back(current_time_ms);
        while self.manual_timestamps_ms.len() > Self::MAX_TAPS {
            self.manual_timestamps_ms.pop_front();
        }

        if let Some((bpm, confidence)) = Self::estimate_bpm(&self.manual_timestamps_ms) {
            self.manual_bpm = Some(bpm);
            self.manual_confidence = confidence;
        }
        self.align_phase(current_time_ms, true);
        self.refresh_state(current_time_ms, mode);
    }

    fn process_auto_onset(
        &mut self,
        is_onset: bool,
        onset_strength: f32,
        current_time_ms: f32,
        mode: BeatSyncMode,
    ) {
        if !is_onset {
            self.refresh_state(current_time_ms, mode);
            return;
        }
        if onset_strength < 1.02 {
            self.refresh_state(current_time_ms, mode);
            return;
        }

        if let Some(previous_ms) = self.auto_last_ms {
            let delta_ms = current_time_ms - previous_ms;
            if delta_ms < Self::MIN_INTERVAL_MS {
                return;
            }
            if delta_ms > Self::AUTO_RESET_GAP_MS {
                self.auto_timestamps_ms.clear();
            }
        }

        self.auto_last_ms = Some(current_time_ms);
        self.auto_timestamps_ms.push_back(current_time_ms);
        while self.auto_timestamps_ms.len() > Self::MAX_AUTO_HITS {
            self.auto_timestamps_ms.pop_front();
        }

        if let Some((bpm, confidence)) = Self::estimate_bpm(&self.auto_timestamps_ms) {
            self.auto_bpm = Some(bpm);
            self.auto_confidence = confidence;
        }

        if mode.allows_auto() {
            self.align_phase(current_time_ms, false);
        }

        self.refresh_state(current_time_ms, mode);
    }

    fn quantize_mod(
        &mut self,
        current_time_ms: f32,
        division: BeatDivision,
        mode: BeatSyncMode,
    ) -> f32 {
        self.refresh_state(current_time_ms, mode);

        let bpm = match self.active_bpm {
            Some(value) => value,
            None => return 1.0,
        };
        let phase_anchor_ms = self.phase_anchor_ms.unwrap_or(current_time_ms);
        let beat_period_ms = 60_000.0 / bpm.max(1.0);
        let cycle_ms = beat_period_ms * division.period_multiplier();
        if !cycle_ms.is_finite() || cycle_ms < 1.0 {
            return 1.0;
        }

        let phase = ((current_time_ms - phase_anchor_ms).rem_euclid(cycle_ms)) / cycle_ms;
        let sinusoid = 0.5 + 0.5 * (phase * std::f32::consts::TAU).cos();
        let nearest_edge = phase.min(1.0 - phase);
        let edge_accent =
            (1.0 - (nearest_edge / division.accent_width()).clamp(0.0, 1.0)).powf(3.0);
        let pulse =
            (0.7 * sinusoid.powf(division.pulse_sharpness()) + 0.3 * edge_accent).clamp(0.0, 1.0);

        let depth = (0.22 + 0.68 * self.active_confidence).clamp(0.20, 0.95);
        let floor = 1.0 - depth;
        (floor + depth * pulse).clamp(0.0, 1.0)
    }

    fn sync_state(&mut self, current_time_ms: f32, mode: BeatSyncMode) -> BeatSyncState {
        self.refresh_state(current_time_ms, mode);
        BeatSyncState {
            bpm: self.active_bpm,
            confidence: self.active_confidence,
            source: self.active_source,
        }
    }

    fn refresh_state(&mut self, current_time_ms: f32, mode: BeatSyncMode) {
        let manual = Self::fresh_signal(
            self.manual_bpm,
            self.manual_confidence,
            self.manual_last_ms,
            current_time_ms,
            Self::MANUAL_STALE_MS,
        );
        let auto = Self::fresh_signal(
            self.auto_bpm,
            self.auto_confidence,
            self.auto_last_ms,
            current_time_ms,
            Self::AUTO_STALE_MS,
        );

        self.active_bpm = None;
        self.active_confidence = 0.0;
        self.active_source = BeatSyncSource::None;

        match mode {
            BeatSyncMode::Tap => {
                if let Some((bpm, confidence)) = manual {
                    self.active_bpm = Some(bpm);
                    self.active_confidence = confidence;
                    self.active_source = BeatSyncSource::Tap;
                }
            }
            BeatSyncMode::Auto => {
                if let Some((bpm, confidence)) = auto {
                    self.active_bpm = Some(bpm);
                    self.active_confidence = confidence;
                    self.active_source = BeatSyncSource::Auto;
                }
            }
            BeatSyncMode::Hybrid => match (manual, auto) {
                (Some((manual_bpm, manual_conf)), Some((auto_bpm, auto_conf))) => {
                    let manual_weight = manual_conf.max(0.05) * 1.15;
                    let auto_weight = auto_conf.max(0.05);
                    let total_weight = (manual_weight + auto_weight).max(0.0001);
                    let blended_bpm =
                        (manual_bpm * manual_weight + auto_bpm * auto_weight) / total_weight;
                    self.active_bpm = Some(blended_bpm.clamp(Self::MIN_BPM, Self::MAX_BPM));
                    self.active_confidence = (manual_conf.max(auto_conf) * 0.85
                        + manual_conf.min(auto_conf) * 0.15)
                        .clamp(0.0, 1.0);
                    self.active_source = BeatSyncSource::Hybrid;
                }
                (Some((bpm, confidence)), None) => {
                    self.active_bpm = Some(bpm);
                    self.active_confidence = confidence;
                    self.active_source = BeatSyncSource::Tap;
                }
                (None, Some((bpm, confidence))) => {
                    self.active_bpm = Some(bpm);
                    self.active_confidence = confidence;
                    self.active_source = BeatSyncSource::Auto;
                }
                (None, None) => {}
            },
        }
    }

    fn fresh_signal(
        bpm: Option<f32>,
        confidence: f32,
        last_event_ms: Option<f32>,
        current_time_ms: f32,
        stale_after_ms: f32,
    ) -> Option<(f32, f32)> {
        let bpm = bpm?;
        let last_event_ms = last_event_ms?;
        let age_ms = (current_time_ms - last_event_ms).max(0.0);
        if age_ms > stale_after_ms {
            return None;
        }

        let freshness = (-age_ms / (stale_after_ms * 0.45)).exp().clamp(0.0, 1.0);
        Some((
            bpm.clamp(Self::MIN_BPM, Self::MAX_BPM),
            (confidence * freshness).clamp(0.0, 1.0),
        ))
    }

    fn estimate_bpm(timestamps_ms: &VecDeque<f32>) -> Option<(f32, f32)> {
        if timestamps_ms.len() < 2 {
            return None;
        }

        let mut intervals_ms = Vec::with_capacity(timestamps_ms.len().saturating_sub(1));
        let mut previous = *timestamps_ms.front()?;
        for &timestamp in timestamps_ms.iter().skip(1) {
            let delta_ms = timestamp - previous;
            previous = timestamp;
            if delta_ms >= Self::MIN_INTERVAL_MS && delta_ms <= Self::MAX_INTERVAL_MS {
                intervals_ms.push(delta_ms);
            }
        }

        if intervals_ms.is_empty() {
            return None;
        }

        if intervals_ms.len() > 8 {
            intervals_ms = intervals_ms[intervals_ms.len() - 8..].to_vec();
        }

        let mut sorted = intervals_ms.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let median_ms = sorted[sorted.len() / 2];
        let tolerance_ms = (median_ms * 0.24).max(18.0);
        let filtered: Vec<f32> = intervals_ms
            .into_iter()
            .filter(|value| (*value - median_ms).abs() <= tolerance_ms)
            .collect();

        if filtered.len() < 2 {
            return None;
        }

        let mean_ms = filtered.iter().sum::<f32>() / filtered.len() as f32;
        let variance = filtered
            .iter()
            .map(|value| (value - mean_ms).powi(2))
            .sum::<f32>()
            / filtered.len() as f32;
        let jitter = variance.sqrt() / mean_ms.max(1.0);

        let mut bpm = 60_000.0 / mean_ms.max(1.0);
        while bpm < Self::MIN_BPM {
            bpm *= 2.0;
        }
        while bpm > Self::MAX_BPM {
            bpm *= 0.5;
        }

        let stability = (1.0 - (jitter / 0.18)).clamp(0.0, 1.0);
        let sample_score = ((filtered.len() as f32 - 1.0) / 6.0).clamp(0.0, 1.0);
        let confidence = (0.28 + 0.72 * sample_score) * stability;

        Some((
            bpm.clamp(Self::MIN_BPM, Self::MAX_BPM),
            confidence.clamp(0.0, 1.0),
        ))
    }

    fn align_phase(&mut self, event_time_ms: f32, hard_reset: bool) {
        if hard_reset {
            self.phase_anchor_ms = Some(event_time_ms);
            return;
        }

        let period_ms = self
            .active_bpm
            .or(self.manual_bpm)
            .or(self.auto_bpm)
            .map(|bpm| 60_000.0 / bpm.max(1.0));
        let (Some(anchor_ms), Some(period_ms)) = (self.phase_anchor_ms, period_ms) else {
            self.phase_anchor_ms = Some(event_time_ms);
            return;
        };

        if !period_ms.is_finite() || period_ms <= 1.0 {
            self.phase_anchor_ms = Some(event_time_ms);
            return;
        }

        let beats_from_anchor = ((event_time_ms - anchor_ms) / period_ms).round();
        let expected_event = anchor_ms + beats_from_anchor * period_ms;
        let phase_error_ms = event_time_ms - expected_event;
        self.phase_anchor_ms = Some(anchor_ms + phase_error_ms * 0.35);
    }
}

// Stop devices on shutdown
impl Drop for GuiApp {
    fn drop(&mut self) {
        if let Some(client) = &self.client {
            if let Err(e) = self.runtime.block_on(client.stop_all_devices()) {
                eprintln!("Error stopping devices: {e:?}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Capture Thread — now with FFT spectral analysis
// ---------------------------------------------------------------------------

/// Audio capture thread. Reads system audio via WASAPI loopback,
/// performs FFT-based spectral analysis, and shares results.
///
/// This replaces the original simple RMS calculation with the full
/// spectral pipeline from ChloeVibes.
fn capture_thread(
    sound_power: SharedF32,
    spectral_shared: SharedSpectralData,
    low_pass_freq: SharedF32,
    polling_rate_ms: SharedF32,
    use_polling_rate: Arc<AtomicBool>,
    capture_status: Arc<Mutex<String>>,
) -> ! {
    loop {
        set_capture_status(&capture_status, "audio: initializing");

        // This audio-capture crate can panic internally on init failure.
        // Catch it so the thread keeps retrying instead of dying silently.
        let mut capture = match std::panic::catch_unwind(|| AudioCapture::init(Duration::ZERO)) {
            Ok(Ok(capture)) => capture,
            Ok(Err(e)) => {
                eprintln!("Audio init failed: {e}");
                set_capture_status(&capture_status, format!("audio: init failed ({e})"));
                sound_power.store(0.0);
                spectral_shared.store(SpectralData::default());
                std::thread::sleep(Duration::from_millis(750));
                continue;
            }
            Err(_) => {
                eprintln!("Audio init panicked; retrying with safe defaults");
                set_capture_status(&capture_status, "audio: init panicked");
                sound_power.store(0.0);
                spectral_shared.store(SpectralData::default());
                std::thread::sleep(Duration::from_millis(750));
                continue;
            }
        };

        let format = match capture.format() {
            Ok(format) => format,
            Err(e) => {
                eprintln!("Audio format error: {e}");
                set_capture_status(&capture_status, format!("audio: format error ({e})"));
                sound_power.store(0.0);
                spectral_shared.store(SpectralData::default());
                std::thread::sleep(Duration::from_millis(750));
                continue;
            }
        };

        let sample_rate = (format.sample_rate as f32).max(1.0);
        let channels = (format.channels as usize).max(1);

        // Poll near half-buffer cadence, but never faster than 5ms.
        let estimated_period =
            Duration::from_secs_f32(capture.buffer_frame_size as f32 / sample_rate);
        let mut default_poll = (estimated_period / 2).max(Duration::from_millis(5));
        if default_poll > Duration::from_millis(40) {
            default_poll = Duration::from_millis(40);
        }
        let sample_dt = Duration::from_secs_f32(1.0 / sample_rate);

        // Buffer large enough for FFT analysis.
        // Need at least FFT_SIZE mono samples = FFT_SIZE * channels interleaved
        let min_buffer = audio::FFT_SIZE * channels;
        let buffer_duration = Duration::from_millis(120);
        let buffer_size =
            ((sample_rate * buffer_duration.as_secs_f32()) as usize * channels).max(min_buffer * 2);
        let mut buf = VecDeque::with_capacity(buffer_size);

        let mut analyzer = SpectralAnalyzer::new(sample_rate);
        let mut total_frames_read: u64 = 0;
        let mut last_status_time = Instant::now();

        if let Err(e) = capture.start() {
            eprintln!("Audio start failed: {e}");
            set_capture_status(&capture_status, format!("audio: start failed ({e})"));
            sound_power.store(0.0);
            spectral_shared.store(SpectralData::default());
            std::thread::sleep(Duration::from_millis(750));
            continue;
        }

        set_capture_status(
            &capture_status,
            format!(
                "audio: running {} Hz / {} ch / {:?}",
                format.sample_rate, format.channels, format.sample_format
            ),
        );

        loop {
            let use_custom = use_polling_rate.load(Ordering::Relaxed);
            let sleep_duration = if use_custom {
                Duration::from_millis(polling_rate_ms.load().max(1.0) as u64)
            } else {
                default_poll
            };
            std::thread::sleep(sleep_duration);

            let mut frames_read_this_tick: usize = 0;
            let read_result = capture.read_samples::<(), _>(|samples, _| {
                buf.extend(samples.iter().copied());
                frames_read_this_tick += samples.len() / channels;
                let len = buf.len();
                if len > buffer_size {
                    buf.drain(0..(len - buffer_size));
                }
                Ok(())
            });
            if let Err(e) = read_result {
                eprintln!("Audio read failed, reinitializing capture: {e:?}");
                set_capture_status(&capture_status, format!("audio: read error ({e:?})"));
                sound_power.store(0.0);
                spectral_shared.store(SpectralData::default());
                break;
            }
            total_frames_read += frames_read_this_tick as u64;

            let samples = buf.make_contiguous();
            if samples.is_empty() {
                sound_power.store(0.0);
                spectral_shared.store(SpectralData::default());

                if last_status_time.elapsed() >= Duration::from_secs(1) {
                    set_capture_status(
                        &capture_status,
                        format!(
                            "audio: waiting packets ({} Hz / {} ch / poll {} ms)",
                            format.sample_rate,
                            format.channels,
                            sleep_duration.as_millis()
                        ),
                    );
                    last_status_time = Instant::now();
                }
                continue;
            }

            // Legacy: low-pass filter + RMS (kept for backward compat / fallback)
            let rc = 1.0 / low_pass_freq.load().max(1.0);
            let filtered = util::low_pass(samples, sample_dt, rc, channels);
            let low_pass_rms = if filtered.is_empty() {
                0.0
            } else {
                let speeds = util::calculate_power(&filtered, channels);
                sanitize_unit(util::avg(&speeds))
            };
            let raw_rms = if samples.is_empty() {
                0.0
            } else {
                let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
                sanitize_unit((sum_sq / samples.len() as f32).sqrt())
            };
            sound_power.store(low_pass_rms.max(raw_rms));

            // NEW: Full spectral analysis via FFT
            let spectral = analyzer.analyze(samples, channels);
            spectral_shared.store(spectral);

            if last_status_time.elapsed() >= Duration::from_secs(1) {
                set_capture_status(
                    &capture_status,
                    format!(
                        "audio: active {} Hz / {} ch / poll {} ms / frames {}",
                        format.sample_rate,
                        format.channels,
                        sleep_duration.as_millis(),
                        total_frames_read
                    ),
                );
                last_status_time = Instant::now();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GuiApp Construction
// ---------------------------------------------------------------------------

impl GuiApp {
    fn new(server_addr: Option<String>, ctx: &CreationContext) -> Self {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let client = None;
        let devices = Default::default();

        let connection_state = ConnectionState::Connecting;
        let server_addr_clone = server_addr.clone();
        let connection_task =
            Some(runtime.spawn(async move { util::start_bp_server(server_addr_clone).await }));

        let sound_power = SharedF32::new(0.0);
        let sound_power2 = sound_power.clone();
        let spectral_data = SharedSpectralData::new();
        let spectral_data2 = spectral_data.clone();
        let processed_output = SharedF32::new(0.0);
        let capture_status = Arc::new(Mutex::new(String::from("audio: starting")));
        let capture_status2 = capture_status.clone();

        let mut settings = ctx.storage.map(Settings::load).unwrap_or_default();
        if settings.current_preset_name.eq_ignore_ascii_case("Default") {
            if let Some(preset) = presets::find_preset("Ride Intensity") {
                settings.apply_preset(&preset);
            }
        }
        let low_pass_freq = settings.low_pass_freq.clone();
        let polling_rate_ms = settings.polling_rate_ms.clone();
        let use_polling_rate = settings.use_polling_rate.clone();

        let _capture_thread = std::thread::spawn(move || {
            capture_thread(
                sound_power2,
                spectral_data2,
                low_pass_freq,
                polling_rate_ms,
                use_polling_rate,
                capture_status2,
            )
        });

        // Load embedded logo (graceful — app works without it)
        let logo_texture = match image::load_from_memory(include_bytes!("../assets/logo.png")) {
            Ok(img) => {
                let logo_size = [img.width() as usize, img.height() as usize];
                let logo_rgba = img.to_rgba8();
                let logo_color_image =
                    egui::ColorImage::from_rgba_unmultiplied(logo_size, logo_rgba.as_raw());
                Some(ctx.egui_ctx.load_texture(
                    "chloevibes-logo",
                    logo_color_image,
                    egui::TextureOptions::LINEAR,
                ))
            }
            Err(e) => {
                eprintln!("Failed to load logo: {e}");
                None
            }
        };

        GuiApp {
            runtime,
            client,
            connection_state,
            connection_task,
            server_addr,
            server_name: String::from("<not connected>"),
            capture_status,
            devices,
            sound_power,
            spectral_data,
            processed_output,
            _capture_thread,
            is_scanning: false,
            show_settings: false,
            settings,
            vibration_level: 0.0,
            hold_start_time: None,

            // New processors
            gate: Gate::new(),
            envelope: EnvelopeProcessor::new(),
            beat_detector: BeatDetector::new(),
            climax_engine: ClimaxEngine::new(),
            last_spectral: SpectralData::default(),
            gate_is_open: false,
            climax_phase: 0.0,

            // Visualization history
            output_history: VecDeque::from(vec![0.0; HISTORY_LEN]),
            energy_history: VecDeque::from(vec![0.0; HISTORY_LEN]),
            input_level: 0.0,
            smoothed_energy: 0.0,
            raw_energy: 0.0,
            using_rms_fallback: false,
            output_delay: VecDeque::with_capacity(512),
            tap_tempo: TapTempo::new(),
            quantize_enabled: false,
            beat_sync_mode: BeatSyncMode::Hybrid,
            quantize_division: BeatDivision::Quarter,

            // Preset UI
            selected_preset_category: PresetCategory::Init,

            // Logo
            logo_texture,
        }
    }
}

// ---------------------------------------------------------------------------
// Main Update Loop
// ---------------------------------------------------------------------------

impl eframe::App for GuiApp {
    fn save(&mut self, storage: &mut dyn Storage) {
        // Save device settings if toggle is enabled
        if self.settings.save_device_settings {
            for (device_name, device) in &self.devices {
                let mut vibrators = Vec::new();
                let mut oscillators = Vec::new();
                let props = &device.props.lock().unwrap();
                for vibe in &props.vibrators {
                    vibrators.push(VibratorSettings {
                        is_enabled: vibe.is_enabled,
                        multiplier: vibe.multiplier,
                        min: vibe.min,
                        max: vibe.max,
                    });
                }
                for osc in &props.oscillators {
                    oscillators.push(OscillatorSettings {
                        is_enabled: osc.is_enabled,
                        multiplier: osc.multiplier,
                        min: osc.min,
                        max: osc.max,
                    });
                }
                let device_settings = DeviceSettings {
                    is_enabled: false,
                    multiplier: props.multiplier,
                    min: props.min,
                    max: props.max,
                    vibrators,
                    oscillators,
                };
                self.settings
                    .device_settings
                    .insert(device_name.clone(), device_settings);
            }
        }
        self.settings.save(storage);
        storage.flush();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply custom dark theme
        let mut visuals = if self.settings.use_dark_mode {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        if self.settings.use_dark_mode {
            visuals.panel_fill = palette::BG_PRIMARY;
            visuals.window_fill = palette::BG_SECONDARY;
            visuals.extreme_bg_color = Color32::from_rgb(10, 10, 14);
            visuals.faint_bg_color = palette::BG_TERTIARY;
            visuals.widgets.noninteractive.bg_fill = palette::BG_SECONDARY;
            visuals.widgets.inactive.bg_fill = palette::BG_TERTIARY;
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 45, 62);
            visuals.widgets.active.bg_fill = palette::ACCENT_PURPLE_DIM;
            visuals.selection.bg_fill = palette::ACCENT_PURPLE;
            visuals.selection.stroke = Stroke::new(1.0, palette::ACCENT_PURPLE);
            visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette::TEXT_SECONDARY);
            visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette::TEXT_PRIMARY);
            visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
            visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
            visuals.widgets.noninteractive.corner_radius = CornerRadius::same(6);
            visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
            visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
            visuals.widgets.active.corner_radius = CornerRadius::same(6);
            visuals.window_corner_radius = CornerRadius::same(10);
        }
        ctx.set_visuals(visuals);

        // --- Connection Handling (unchanged) ---
        if let Some(task) = self.connection_task.take() {
            if task.is_finished() {
                match self.runtime.block_on(task) {
                    Ok(Ok(client)) => {
                        self.server_name = client
                            .server_name()
                            .as_deref()
                            .unwrap_or("<unknown>")
                            .to_string();
                        self.client = Some(client);
                        self.connection_state = ConnectionState::Connected;
                        if self.settings.start_scanning_on_startup {
                            if let Some(client) = &self.client {
                                self.runtime.spawn(client.start_scanning());
                                self.is_scanning = true;
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        self.connection_state =
                            ConnectionState::Error(format!("Connection failed: {e}"));
                    }
                    Err(e) => {
                        self.connection_state =
                            ConnectionState::Error(format!("Task panicked: {e:?}"));
                    }
                }
            } else {
                self.connection_task = Some(task);
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // === ChloeVibes Logo ===
            if let Some(logo) = &self.logo_texture {
                let max_logo_w = ui.available_width() * 0.45;
                let tex_sz = logo.size();
                let tex_w = tex_sz[0] as f32;
                let tex_h = tex_sz[1] as f32;
                let scale = (max_logo_w / tex_w).min(1.0);
                let display_size = vec2(tex_w * scale, tex_h * scale);
                ui.vertical_centered(|ui| {
                    ui.add(
                        egui::Image::new(logo)
                            .fit_to_exact_size(display_size),
                    );
                });
            }

            // === Top Bar: Connection + Scanning + Settings + Stop ===
            ui.horizontal(|ui| {
                if !matches!(self.connection_state, ConnectionState::Connecting)
                {
                    match self.connection_state {
                        ConnectionState::Disconnected
                        | ConnectionState::Error(_) => {
                            if ui.button("Connect to Server").clicked() {
                                self.connection_state =
                                    ConnectionState::Connecting;
                                let addr = self.server_addr.clone();
                                self.connection_task =
                                    Some(self.runtime.spawn(async move {
                                        util::start_bp_server(addr).await
                                    }));
                            }
                        }
                        ConnectionState::Connected => {
                            if let Some(client) = &self.client {
                                let label = if self.is_scanning {
                                    "Stop scanning"
                                } else {
                                    "Start scanning"
                                };
                                let scan_btn = Button::new(
                                    RichText::new(label)
                                        .color(Color32::WHITE),
                                )
                                .fill(if self.is_scanning {
                                    palette::ACCENT_PURPLE
                                } else {
                                    palette::BG_TERTIARY
                                });
                                if ui.add(scan_btn).clicked() {
                                    if self.is_scanning {
                                        match self
                                            .runtime
                                            .block_on(client.stop_scanning())
                                        {
                                            Ok(_) => {
                                                self.is_scanning = false;
                                            }
                                            Err(e) => {
                                                self.connection_state = ConnectionState::Error(
                                                    format!(
                                                        "Stop scan failed: {e}"
                                                    ),
                                                );
                                            }
                                        }
                                    } else {
                                        match self
                                            .runtime
                                            .block_on(client.start_scanning())
                                        {
                                            Ok(_) => {
                                                self.is_scanning = true;
                                            }
                                            Err(e) => {
                                                self.is_scanning = false;
                                                self.connection_state = ConnectionState::Error(
                                                    format!(
                                                        "Start scan failed: {e}"
                                                    ),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        ConnectionState::Connecting => {}
                    }
                }

                match &self.connection_state {
                    ConnectionState::Disconnected => {
                        ui.label("Disconnected");
                    }
                    ConnectionState::Connecting => {
                        ui.label("Connecting...");
                    }
                    ConnectionState::Error(msg) => {
                        ui.colored_label(Color32::RED, "Error");
                        ui.label(
                            RichText::new(msg)
                                .size(9.0)
                                .color(palette::TEXT_DIM),
                        );
                    }
                    ConnectionState::Connected => {}
                }

                if matches!(self.connection_state, ConnectionState::Connected) {
                    ui.label(
                        RichText::new(format!("Server: {}", self.server_name))
                            .size(9.0)
                            .color(palette::TEXT_DIM)
                            .monospace(),
                    );
                }
                if let Ok(status) = self.capture_status.lock() {
                    ui.label(
                        RichText::new(format!("Audio: {}", &*status))
                            .size(9.0)
                            .color(palette::TEXT_DIM)
                            .monospace(),
                    );
                }

                if ui.button("Settings").clicked() {
                    self.show_settings = true;
                }

                let stop_w = 120.0;
                ui.add_space(ui.available_width() - stop_w);

                let stop_btn = Button::new(
                    RichText::new("Stop all devices").color(Color32::BLACK),
                )
                .fill(Color32::from_rgb(240, 0, 0));
                if ui.add_sized([stop_w, 30.0], stop_btn).clicked() {
                    if let Some(client) = &self.client {
                        self.runtime.spawn(client.stop_all_devices());
                        for device in self.devices.values_mut() {
                            device.props.lock().unwrap().is_enabled = false;
                        }
                    }
                }
            });

            ui.separator();

            // ============================================================
            // AUDIO PROCESSING PIPELINE
            // ============================================================
            let delta_time = ctx.input(|x| x.stable_dt);
            let main_mul = self.settings.main_volume.powi(2);
            let current_time_ms = {
                static START: std::sync::OnceLock<Instant> =
                    std::sync::OnceLock::new();
                let start = START.get_or_init(Instant::now);
                start.elapsed().as_secs_f32() * 1000.0
            };

            if self.settings.use_advanced_processing {
                // ====== NEW PIPELINE (ChloeVibes-derived) ======

                // 1. Read spectral data from capture thread
                let spectral = self.spectral_data.load();
                self.last_spectral = spectral.clone();

                // 2. Extract energy based on frequency mode
                let spectral_energy = SpectralAnalyzer::extract_energy(
                    &spectral,
                    self.settings.frequency_mode,
                    self.settings.target_frequency,
                );
                let spectral_energy = sanitize_unit(spectral_energy);
                let legacy_energy = sanitize_unit(self.sound_power.load());
                let spectral_rms = sanitize_unit(spectral.rms_power);
                let spectral_total = sanitize_unit(
                    spectral.band_energies.iter().copied().sum::<f32>()
                        / BAND_NAMES.len() as f32,
                );
                let using_rms_fallback =
                    spectral_total < 0.0008 && (spectral_rms > 0.003 || legacy_energy > 0.003);
                let capture_energy = if using_rms_fallback {
                    spectral_rms.max(legacy_energy)
                } else {
                    spectral_energy
                };
                let normalized_input = normalize_capture_energy(capture_energy);

                // 3. Apply main volume and smooth for musical consistency
                let energy = sanitize_unit(normalized_input * main_mul);
                let rise_alpha = smoothing_alpha(delta_time, self.settings.input_rise_ms);
                let fall_alpha = smoothing_alpha(delta_time, self.settings.input_fall_ms);
                if energy >= self.smoothed_energy {
                    self.smoothed_energy += (energy - self.smoothed_energy) * rise_alpha;
                } else {
                    self.smoothed_energy += (energy - self.smoothed_energy) * fall_alpha;
                }
                let stable_energy = sanitize_unit(self.smoothed_energy);
                self.input_level = energy;
                self.raw_energy = stable_energy;
                self.using_rms_fallback = using_rms_fallback;

                // 4. Beat detection (for onset retrigger)
                let (is_onset, onset_strength) =
                    self.beat_detector.process(spectral.spectral_flux, current_time_ms);
                let onset_ok = is_onset
                    && onset_strength > 1.02
                    && energy > self.settings.gate_threshold * 0.40;
                self.tap_tempo.process_auto_onset(
                    onset_ok,
                    onset_strength,
                    current_time_ms,
                    self.beat_sync_mode,
                );

                // 5. Gate
                self.gate_is_open = self.gate.process(
                    stable_energy,
                    self.settings.gate_threshold,
                    self.settings.auto_gate_amount,
                    self.settings.gate_smoothing,
                    self.settings.threshold_knee,
                );

                // 6. Envelope ADSR (the big upgrade)
                let envelope_output = self.envelope.drive(
                    self.gate_is_open,
                    energy,
                    onset_ok,
                    onset_strength,
                    current_time_ms,
                    self.settings.trigger_mode,
                    self.gate.effective_threshold(
                        self.settings.gate_threshold,
                        self.settings.auto_gate_amount,
                    ),
                    self.settings.threshold_knee,
                    self.settings.dynamic_curve,
                    self.settings.binary_level,
                    self.settings.hybrid_blend,
                    self.settings.attack_ms,
                    self.settings.decay_ms,
                    self.settings.sustain_level,
                    self.settings.release_ms,
                    self.settings.attack_curve,
                    self.settings.decay_curve,
                    self.settings.release_curve,
                );

                // 7. Optional climax modulation layer
                let mut shaped_output = self.climax_engine.process(
                    envelope_output,
                    energy,
                    self.gate_is_open,
                    onset_ok,
                    onset_strength,
                    current_time_ms,
                    self.settings.climax_mode_enabled,
                    self.settings.climax_intensity,
                    self.settings.climax_build_up_ms,
                    self.settings.climax_tease_ratio,
                    self.settings.climax_tease_drop,
                    self.settings.climax_surge_boost,
                    self.settings.climax_pulse_depth,
                    self.settings.climax_pattern,
                );
                if self.quantize_enabled {
                    shaped_output *= self.tap_tempo.quantize_mod(
                        current_time_ms,
                        self.quantize_division,
                        self.beat_sync_mode,
                    );
                }
                self.climax_phase = if self.settings.climax_mode_enabled {
                    self.climax_engine.phase_progress(
                        current_time_ms,
                        self.settings.climax_build_up_ms,
                    )
                } else {
                    0.0
                };

                // 8. Apply output range (min_vibe / max_vibe)
                let final_intensity = self.settings.min_vibe
                    + shaped_output
                        * (self.settings.max_vibe - self.settings.min_vibe);

                // 9. Safety: force to zero if energy is negligible.
                // Only reset the envelope, NEVER the climax engine — a brief
                // silence between notes should not destroy minutes of build-up.
                let final_intensity = if energy < 0.005 && !self.gate_is_open
                    && self.envelope.state == audio::EnvelopeState::Idle
                {
                    0.0
                } else {
                    final_intensity.clamp(0.0, 1.0)
                };

                // Apply timing trim (delay/advance) via small ring buffer
                let dt = delta_time.max(0.0001);
                let trim_ms = self.settings.trim_ms.clamp(-500.0, 500.0);
                let trim_frames =
                    ((trim_ms.abs() / (dt * 1000.0)).round() as usize).min(400);
                self.output_delay.push_back(final_intensity);
                if self.output_delay.len() > 1024 {
                    self.output_delay.pop_front();
                }
                let trimmed_intensity = if trim_ms >= 1.0 {
                    if self.output_delay.len() > trim_frames {
                        let idx = self.output_delay.len() - 1 - trim_frames;
                        self.output_delay.get(idx).copied().unwrap_or(final_intensity)
                    } else {
                        final_intensity
                    }
                } else if trim_ms <= -1.0 {
                    // Advance: drop some history to reduce latency
                    for _ in 0..trim_frames.min(self.output_delay.len()) {
                        self.output_delay.pop_front();
                    }
                    final_intensity
                } else {
                    final_intensity
                };

                let output_up_ms = (self.settings.output_slew_ms * 0.35).max(1.0);
                let output_down_ms = self.settings.output_slew_ms.max(1.0);
                let output_alpha = if final_intensity >= self.vibration_level {
                    smoothing_alpha(delta_time, output_up_ms)
                } else {
                    smoothing_alpha(delta_time, output_down_ms)
                };
                self.vibration_level +=
                    (trimmed_intensity - self.vibration_level) * output_alpha;
                self.vibration_level = self.vibration_level.clamp(0.0, 1.0);
            } else {
                // ====== LEGACY PIPELINE (original Chloe Vibes) ======
                let source = sanitize_unit(self.sound_power.load());
                let sound_power = sanitize_unit(source * main_mul);
                self.input_level = source;
                self.smoothed_energy = source;
                self.raw_energy = sound_power;
                self.using_rms_fallback = false;
                self.climax_phase = 0.0;
                self.climax_engine.reset(current_time_ms);

                let mut persistent_level = self.vibration_level;

                if !self.settings.enable_persistence {
                    persistent_level = sound_power;
                    self.hold_start_time = None;
                } else {
                    if sound_power >= persistent_level {
                        persistent_level = sound_power;
                        self.hold_start_time = None;
                    } else {
                        match self.hold_start_time {
                            None => {
                                if self.settings.hold_delay_ms >= 1.0 {
                                    self.hold_start_time = Some(Instant::now());
                                } else {
                                    let rate =
                                        self.settings.decay_rate_per_sec;
                                    if rate <= 0.0 {
                                        persistent_level = sound_power;
                                    } else {
                                        persistent_level -= rate * delta_time;
                                        persistent_level =
                                            persistent_level.max(sound_power);
                                    }
                                }
                            }
                            Some(start_time) => {
                                let hold = Duration::from_millis(
                                    self.settings.hold_delay_ms as u64,
                                );
                                if start_time.elapsed() >= hold {
                                    let rate =
                                        self.settings.decay_rate_per_sec;
                                    if rate <= 0.0 {
                                        persistent_level = sound_power;
                                        self.hold_start_time = None;
                                    } else {
                                        persistent_level -= rate * delta_time;
                                        persistent_level =
                                            persistent_level.max(sound_power);
                                    }
                                }
                            }
                        }
                    }
                    persistent_level = persistent_level.max(0.0);
                }
                self.vibration_level = persistent_level.clamp(0.0, 1.0);
            }

            // Store processed output for device tasks
            self.processed_output.store(self.vibration_level);

            // Push to visualization history
            self.output_history.push_back(self.vibration_level);
            if self.output_history.len() > HISTORY_LEN {
                self.output_history.pop_front();
            }
            self.energy_history.push_back(self.raw_energy);
            if self.energy_history.len() > HISTORY_LEN {
                self.energy_history.pop_front();
            }

            // ============================================================
            // UI: Visualizations & Controls (ChloeVibes-style)
            // ============================================================

            ui.add_space(4.0);

            // --- Output bar with glow ---
            {
                let desired_size = vec2(ui.available_width(), 14.0);
                let (rect, _) = ui.allocate_exact_size(
                    desired_size,
                    egui::Sense::hover(),
                );
                let painter = ui.painter();

                // Background
                painter.rect_filled(rect, 4.0, palette::BG_TERTIARY);

                // Filled portion
                let fill_width = rect.width() * self.vibration_level;
                if fill_width > 0.5 {
                    let fill_rect = Rect::from_min_size(
                        rect.min,
                        vec2(fill_width, rect.height()),
                    );
                    // Color based on intensity
                    let color = if self.vibration_level > 0.85 {
                        palette::ACCENT_PINK
                    } else if self.vibration_level > 0.5 {
                        palette::ACCENT_PURPLE
                    } else {
                        palette::ACCENT_TEAL
                    };
                    painter.rect_filled(fill_rect, 4.0, color);
                }
            }

            ui.horizontal(|ui| {
                let output_pct = self.vibration_level * 100.0;
                let input_pct = self.input_level * 100.0;
                ui.label(
                    RichText::new(format!("OUTPUT  {:.1}%", output_pct))
                        .size(9.0)
                        .color(palette::TEXT_DIM)
                        .monospace(),
                );
                ui.label(
                    RichText::new(format!("  INPUT  {:.1}%", input_pct))
                        .size(9.0)
                        .color(palette::TEXT_DIM)
                        .monospace(),
                );
                if self.gate_is_open && self.settings.use_advanced_processing {
                    ui.label(
                        RichText::new(" GATE OPEN")
                            .size(9.0)
                            .color(palette::ACCENT_TEAL)
                            .monospace(),
                    );
                }
                if self.settings.use_advanced_processing
                    && self.using_rms_fallback
                {
                    ui.label(
                        RichText::new(" RMS FALLBACK")
                            .size(9.0)
                            .color(palette::ACCENT_AMBER)
                            .monospace(),
                    );
                }
            });

            ui.add_space(6.0);

            // ==========================================================
            // ALGORITHM SELECTOR (Main Page)
            // ==========================================================
            ui.horizontal(|ui| {
                section_label(ui, "ALGORITHM", palette::ACCENT_AMBER);

                let legacy_active = !self.settings.use_advanced_processing;
                let legacy_btn = Button::new(
                    RichText::new("Original Chloe Vibes (RMS)")
                        .size(10.0)
                        .monospace()
                        .color(if legacy_active {
                            Color32::WHITE
                        } else {
                            palette::TEXT_PRIMARY
                        }),
                )
                .fill(if legacy_active {
                    palette::ACCENT_PURPLE
                } else {
                    palette::BG_SECONDARY
                })
                .stroke(Stroke::new(
                    0.5,
                    if legacy_active {
                        palette::ACCENT_PURPLE
                    } else {
                        palette::BG_TERTIARY
                    },
                ))
                .corner_radius(CornerRadius::same(4));

                if ui.add(legacy_btn).clicked() {
                    self.settings.use_advanced_processing = false;
                    self.settings.climax_mode_enabled = false;
                    self.quantize_enabled = false;
                    self.tap_tempo.reset();
                    self.envelope.reset();
                    self.climax_engine.reset(current_time_ms);
                    mark_custom(&mut self.settings);
                }

                let advanced_active = self.settings.use_advanced_processing;
                let advanced_btn = Button::new(
                    RichText::new("Advanced FFT + ADSR")
                        .size(10.0)
                        .monospace()
                        .color(if advanced_active {
                            Color32::WHITE
                        } else {
                            palette::TEXT_PRIMARY
                        }),
                )
                .fill(if advanced_active {
                    palette::ACCENT_PURPLE
                } else {
                    palette::BG_SECONDARY
                })
                .stroke(Stroke::new(
                    0.5,
                    if advanced_active {
                        palette::ACCENT_PURPLE
                    } else {
                        palette::BG_TERTIARY
                    },
                ))
                .corner_radius(CornerRadius::same(4));

                if ui.add(advanced_btn).clicked() {
                    self.settings.use_advanced_processing = true;
                    self.envelope.reset();
                    self.climax_engine.reset(current_time_ms);
                    mark_custom(&mut self.settings);
                }
            });

            let algorithm_text = if self.settings.use_advanced_processing {
                "FFT spectrum -> gate -> trigger -> ADSR -> optional beat/climax modulation -> output range."
            } else {
                "RMS loudness -> volume^2 -> optional hold/decay persistence -> clamp 0..1 -> device output."
            };
            ui.label(
                RichText::new(algorithm_text)
                    .size(9.0)
                    .color(palette::TEXT_DIM)
                    .monospace(),
            );

            ui.add_space(6.0);

            if self.settings.use_advanced_processing {
                // ==========================================================
                // Visualization panels in a two-column grid
                // ==========================================================

                ui.columns(2, |cols| {
                    // --- LEFT: Output History Waveform ---
                    cols[0].vertical(|ui| {
                        let desired = vec2(
                            ui.available_width(),
                            OUTPUT_HISTORY_HEIGHT,
                        );
                        let (rect, _) = ui.allocate_exact_size(
                            desired,
                            egui::Sense::hover(),
                        );
                        draw_output_history(
                            ui.painter(),
                            rect,
                            &self.output_history,
                            &self.energy_history,
                            self.gate_is_open,
                            self.settings.gate_threshold,
                        );
                        ui.label(
                            RichText::new("ENERGY GATE")
                                .size(9.0)
                                .color(palette::ACCENT_TEAL)
                                .monospace(),
                        );
                    });

                    // --- RIGHT: Spectrum Bands ---
                    cols[1].vertical(|ui| {
                        let desired = vec2(
                            ui.available_width(),
                            OUTPUT_HISTORY_HEIGHT,
                        );
                        let (rect, _) = ui.allocate_exact_size(
                            desired,
                            egui::Sense::hover(),
                        );
                        draw_spectrum_bars(
                            ui.painter(),
                            rect,
                            &self.last_spectral.band_energies,
                            self.gate_is_open,
                        );
                        ui.label(
                            RichText::new("SPECTRUM")
                                .size(9.0)
                                .color(palette::ACCENT_PURPLE)
                                .monospace(),
                        );
                    });
                });

                ui.add_space(4.0);

                // --- ADSR Envelope Shape Preview ---
                let desired = vec2(ui.available_width(), ADSR_PREVIEW_HEIGHT);
                let (rect, _) = ui.allocate_exact_size(
                    desired,
                    egui::Sense::hover(),
                );
                draw_adsr_envelope(
                    ui.painter(),
                    rect,
                    self.settings.attack_ms,
                    self.settings.decay_ms,
                    self.settings.sustain_level,
                    self.settings.release_ms,
                    self.settings.attack_curve,
                    self.settings.decay_curve,
                    self.settings.release_curve,
                    &self.envelope,
                );
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("ENVELOPE SHAPE")
                            .size(9.0)
                            .color(palette::ACCENT_TEAL)
                            .monospace(),
                    );
                    ui.label(
                        RichText::new("  A → D → S → R")
                            .size(8.0)
                            .color(palette::TEXT_DIM)
                            .monospace(),
                    );
                });

                ui.add_space(8.0);

                // ==========================================================
                // PRESET SELECTOR — Synth-style patch browser
                // ==========================================================

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("PRESET")
                            .size(9.0)
                            .color(palette::ACCENT_AMBER)
                            .monospace(),
                    );

                    // Category tabs
                    for &cat in PresetCategory::all() {
                        let is_selected = self.selected_preset_category == cat;
                        let label = RichText::new(cat.label())
                            .size(9.0)
                            .monospace()
                            .color(if is_selected {
                                Color32::WHITE
                            } else {
                                palette::TEXT_DIM
                            });
                        let btn = Button::new(label)
                            .fill(if is_selected {
                                palette::ACCENT_PURPLE_DIM
                            } else {
                                palette::BG_TERTIARY
                            })
                            .corner_radius(CornerRadius::same(4));
                        if ui.add(btn).clicked() {
                            self.selected_preset_category = cat;
                        }
                    }

                    // Show current preset name
                    ui.add_space(8.0);
                    let preset_label = if self.settings.current_preset_name.is_empty() {
                        "Custom".to_string()
                    } else {
                        self.settings.current_preset_name.clone()
                    };
                    ui.label(
                        RichText::new(format!("▸ {}", preset_label))
                            .size(10.0)
                            .color(palette::ACCENT_TEAL)
                            .monospace(),
                    );
                });

                // Preset buttons for selected category
                {
                    let category_presets = presets::presets_in_category(
                        self.selected_preset_category,
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(62.0); // Indent past "PRESET" label
                        for preset in &category_presets {
                            let is_active = self.settings.current_preset_name == preset.name;
                            let btn_text = RichText::new(preset.name)
                                .size(10.0)
                                .monospace()
                                .color(if is_active {
                                    Color32::WHITE
                                } else {
                                    palette::TEXT_PRIMARY
                                });
                            let btn = Button::new(btn_text)
                                .fill(if is_active {
                                    palette::ACCENT_PURPLE
                                } else {
                                    palette::BG_SECONDARY
                                })
                                .stroke(Stroke::new(
                                    0.5,
                                    if is_active {
                                        palette::ACCENT_PURPLE
                                    } else {
                                        palette::BG_TERTIARY
                                    },
                                ))
                                .corner_radius(CornerRadius::same(4));
                            let response = ui.add(btn);
                            if response.clicked() {
                                self.settings.apply_preset(preset);
                            }
                            response.on_hover_text(preset.description);
                        }
                    });
                }

                ui.horizontal(|ui| {
                    section_label(ui, "CHLOE", palette::ACCENT_PINK);
                    ui.label(
                        RichText::new("Auto rhythm macro")
                            .size(9.0)
                            .color(palette::TEXT_DIM)
                            .monospace(),
                    );
                    if ui.button("Ride Intensity").clicked() {
                        if let Some(preset) = presets::find_preset("Ride Intensity")
                        {
                            self.settings.apply_preset(&preset);
                        }
                        self.settings.use_advanced_processing = true;
                        self.settings.climax_mode_enabled = false;
                        self.quantize_enabled = false;
                        self.tap_tempo.reset();
                        self.envelope.reset();
                        self.climax_engine.reset(current_time_ms);
                    }
                    if ui.button("Loose").clicked() {
                        apply_chloe_rhythm_profile(
                            &mut self.settings,
                            ChloeRhythmProfile::Loose,
                        );
                        self.envelope.reset();
                        self.climax_engine.reset(current_time_ms);
                    }
                    if ui.button("Medium").clicked() {
                        apply_chloe_rhythm_profile(
                            &mut self.settings,
                            ChloeRhythmProfile::Medium,
                        );
                        self.envelope.reset();
                        self.climax_engine.reset(current_time_ms);
                    }
                    if ui.button("Ultimate").clicked() {
                        apply_chloe_rhythm_profile(
                            &mut self.settings,
                            ChloeRhythmProfile::Ultimate,
                        );
                        self.envelope.reset();
                        self.climax_engine.reset(current_time_ms);
                    }
                });

                ui.add_space(4.0);

                // ==========================================================
                // INPUT — Volume + Frequency Filter
                // ==========================================================

                ui.horizontal(|ui| {
                    section_label(ui, "INPUT", palette::TEXT_DIM);
                    let mut vol_pct = self.settings.main_volume * 100.0;
                    let slider = ui.add(
                        Slider::new(&mut vol_pct, 0.0..=500.0)
                            .suffix("%")
                            .text("Volume"),
                    );
                    if slider.changed() {
                        self.settings.main_volume = vol_pct / 100.0;
                        mark_custom(&mut self.settings);
                    }
                    if slider.double_clicked() {
                        self.settings.main_volume = defaults::MAIN_VOLUME;
                    }
                });

                ui.horizontal(|ui| {
                    section_label(ui, "FREQ", palette::TEXT_DIM);
                    ComboBox::from_id_salt("freq_mode")
                        .selected_text(match self.settings.frequency_mode {
                            FrequencyMode::Full => "Full Range",
                            FrequencyMode::LowPass => "Low Pass",
                            FrequencyMode::HighPass => "High Pass",
                            FrequencyMode::BandPass => "Band Pass",
                        })
                        .show_ui(ui, |ui| {
                            let changed = false
                                | ui.selectable_value(
                                    &mut self.settings.frequency_mode,
                                    FrequencyMode::Full,
                                    "Full Range — all frequencies",
                                )
                                .changed()
                                | ui.selectable_value(
                                    &mut self.settings.frequency_mode,
                                    FrequencyMode::LowPass,
                                    "Low Pass — bass & sub only",
                                )
                                .changed()
                                | ui.selectable_value(
                                    &mut self.settings.frequency_mode,
                                    FrequencyMode::HighPass,
                                    "High Pass — treble & air only",
                                )
                                .changed()
                                | ui.selectable_value(
                                    &mut self.settings.frequency_mode,
                                    FrequencyMode::BandPass,
                                    "Band Pass — narrow range",
                                )
                                .changed();
                            if changed {
                                mark_custom(&mut self.settings);
                            }
                        });

                    if self.settings.frequency_mode != FrequencyMode::Full {
                        let slider = ui.add(
                            Slider::new(
                                &mut self.settings.target_frequency,
                                20.0..=8000.0,
                            )
                            .logarithmic(true)
                            .suffix(" Hz")
                            .text("Cutoff"),
                        );
                        if slider.changed() {
                            mark_custom(&mut self.settings);
                        }
                        if slider.double_clicked() {
                            self.settings.target_frequency =
                                defaults::TARGET_FREQUENCY;
                        }
                    }

                    // Polling rate (if enabled)
                    let is_custom =
                        self.settings.use_polling_rate.load(Ordering::Relaxed);
                    if is_custom {
                        ui.separator();
                        ui.label(
                            RichText::new("POLL")
                                .size(9.0)
                                .color(palette::TEXT_DIM)
                                .monospace(),
                        );
                        let mut rate = self.settings.polling_rate_ms.load();
                        let slider = ui.add(
                            Slider::new(&mut rate, 1.0..=500.0)
                                .integer()
                                .logarithmic(true),
                        );
                        if slider.changed() {
                            self.settings.polling_rate_ms.store(rate);
                        }
                        if slider.double_clicked() {
                            self.settings
                                .polling_rate_ms
                                .store(defaults::POLLING_RATE_MS);
                        }
                    }
                });

                ui.horizontal(|ui| {
                    section_label(ui, "RESP", palette::TEXT_DIM);

                    let rise = ui.add(
                        Slider::new(&mut self.settings.input_rise_ms, 1.0..=160.0)
                            .logarithmic(true)
                            .suffix("ms")
                            .text("Rise"),
                    );
                    let rise = rise.on_hover_text(
                        "Detector rise speed (pre-gate).\n\
                         Lower = tighter detection. Higher = smoother gate opening.",
                    );
                    if rise.changed() {
                        mark_custom(&mut self.settings);
                    }
                    if rise.double_clicked() {
                        self.settings.input_rise_ms = defaults::INPUT_RISE_MS;
                    }

                    let fall = ui.add(
                        Slider::new(&mut self.settings.input_fall_ms, 1.0..=300.0)
                            .logarithmic(true)
                            .suffix("ms")
                            .text("Fall"),
                    );
                    let fall = fall.on_hover_text(
                        "Detector fall speed (pre-gate).\n\
                         Lower = less trailing gate hold. Higher = smoother hold.",
                    );
                    if fall.changed() {
                        mark_custom(&mut self.settings);
                    }
                    if fall.double_clicked() {
                        self.settings.input_fall_ms = defaults::INPUT_FALL_MS;
                    }

                    let slew = ui.add(
                        Slider::new(&mut self.settings.output_slew_ms, 1.0..=220.0)
                            .logarithmic(true)
                            .suffix("ms")
                            .text("Slew"),
                    );
                    let slew = slew.on_hover_text(
                        "Post-envelope output slew.\n\
                         Lower = snappier; higher = smoother fall transitions.",
                    );
                    if slew.changed() {
                        mark_custom(&mut self.settings);
                    }
                    if slew.double_clicked() {
                        self.settings.output_slew_ms = defaults::OUTPUT_SLEW_MS;
                    }

                    let trim = ui.add(
                        Slider::new(&mut self.settings.trim_ms, -250.0..=250.0)
                            .suffix("ms")
                            .text("Trim"),
                    );
                    let trim = trim.on_hover_text(
                        "Lead/lag haptics vs audio. Positive = delay. Negative = advance (limited).",
                    );
                    if trim.changed() {
                        mark_custom(&mut self.settings);
                    }
                    if trim.double_clicked() {
                        self.settings.trim_ms = defaults::TRIM_MS;
                    }

                    ui.separator();
                    ui.checkbox(&mut self.quantize_enabled, "Quantize");
                    if self.quantize_enabled {
                        ComboBox::from_id_salt("beat_sync_mode")
                            .selected_text(self.beat_sync_mode.label())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.beat_sync_mode,
                                    BeatSyncMode::Tap,
                                    "Tap",
                                );
                                ui.selectable_value(
                                    &mut self.beat_sync_mode,
                                    BeatSyncMode::Auto,
                                    "Auto",
                                );
                                ui.selectable_value(
                                    &mut self.beat_sync_mode,
                                    BeatSyncMode::Hybrid,
                                    "Hybrid",
                                );
                            });

                        ComboBox::from_id_salt("quantize_div")
                            .selected_text(match self.quantize_division {
                                BeatDivision::Quarter => "1/4",
                                BeatDivision::Eighth => "1/8",
                                BeatDivision::Half => "1/2",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.quantize_division,
                                    BeatDivision::Half,
                                    "1/2",
                                );
                                ui.selectable_value(
                                    &mut self.quantize_division,
                                    BeatDivision::Quarter,
                                    "1/4",
                                );
                                ui.selectable_value(
                                    &mut self.quantize_division,
                                    BeatDivision::Eighth,
                                    "1/8",
                                );
                            });
                        if ui.button("Tap").clicked() {
                            self.tap_tempo
                                .tap_now(current_time_ms, self.beat_sync_mode);
                        }
                        if ui.button("Reset BPM").clicked() {
                            self.tap_tempo.reset();
                        }
                        let sync_state = self
                            .tap_tempo
                            .sync_state(current_time_ms, self.beat_sync_mode);
                        if let Some(bpm) = sync_state.bpm {
                            ui.label(
                                RichText::new(format!("BPM {:.1}", bpm))
                                    .size(10.0)
                                    .color(palette::ACCENT_TEAL)
                                    .monospace(),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{} {:>2.0}%",
                                    sync_state.source.label(),
                                    sync_state.confidence * 100.0
                                ))
                                .size(10.0)
                                .color(palette::TEXT_SECONDARY)
                                .monospace(),
                            );
                        } else {
                            ui.label(
                                RichText::new("BPM --")
                                    .size(10.0)
                                    .color(palette::TEXT_DIM)
                                    .monospace(),
                            );
                        }
                    }
                });

                ui.add_space(2.0);

                // ==========================================================
                // NOISE GATE — When does the vibrator activate?
                // ==========================================================
                //
                // Simplified: just the threshold slider. Auto-sense and
                // smoothing are gone — they added confusion, not value.
                // The dashed line on the energy gate visualization shows
                // exactly where this threshold sits.

                ui.horizontal(|ui| {
                    section_label(ui, "THRESHOLD", palette::ACCENT_PINK);

                    // Gate status indicator
                    let gate_text = if self.gate_is_open { "OPEN" } else { "CLOSED" };
                    let gate_color = if self.gate_is_open {
                        palette::ACCENT_TEAL
                    } else {
                        palette::TEXT_DIM
                    };
                    ui.label(
                        RichText::new(gate_text)
                            .size(9.0)
                            .color(gate_color)
                            .monospace(),
                    );

                    let slider = ui.add(
                        Slider::new(
                            &mut self.settings.gate_threshold,
                            0.0..=1.0,
                        )
                        .fixed_decimals(2),
                    );
                    let slider = slider.on_hover_text(
                        "How loud audio must be to trigger vibration.\n\
                         The dashed line on the Energy Gate shows this level.\n\
                         Higher = only loud sounds. Lower = reacts to quiet sounds too.",
                    );
                    if slider.changed() {
                        mark_custom(&mut self.settings);
                    }
                    if slider.double_clicked() {
                        self.settings.gate_threshold =
                            defaults::GATE_THRESHOLD;
                    }

                    let knee_slider = ui.add(
                        Slider::new(
                            &mut self.settings.threshold_knee,
                            0.0..=0.35,
                        )
                        .fixed_decimals(2)
                        .text("Knee"),
                    );
                    let knee_slider = knee_slider.on_hover_text(
                        "Softens the threshold edge.\n\
                         Higher = smoother transition and less all-or-nothing behavior.",
                    );
                    if knee_slider.changed() {
                        mark_custom(&mut self.settings);
                    }
                    if knee_slider.double_clicked() {
                        self.settings.threshold_knee = defaults::THRESHOLD_KNEE;
                    }
                });

                ui.add_space(2.0);

                // ==========================================================
                // TRIGGER MODE — How does audio energy become intensity?
                // ==========================================================

                ui.horizontal(|ui| {
                    section_label(ui, "TRIGGER", palette::TEXT_DIM);
                    ComboBox::from_id_salt("trigger_mode")
                        .selected_text(match self.settings.trigger_mode {
                            TriggerMode::Dynamic => "Dynamic",
                            TriggerMode::Binary => "Binary (On/Off)",
                            TriggerMode::Hybrid => "Hybrid",
                        })
                        .show_ui(ui, |ui| {
                            let changed = false
                                | ui.selectable_value(
                                    &mut self.settings.trigger_mode,
                                    TriggerMode::Dynamic,
                                    "Dynamic — louder = stronger vibration",
                                )
                                .changed()
                                | ui.selectable_value(
                                    &mut self.settings.trigger_mode,
                                    TriggerMode::Binary,
                                    "Binary — fixed level when gate opens",
                                )
                                .changed()
                                | ui.selectable_value(
                                    &mut self.settings.trigger_mode,
                                    TriggerMode::Hybrid,
                                    "Hybrid — blend of dynamic + binary",
                                )
                                .changed();
                            if changed {
                                mark_custom(&mut self.settings);
                            }
                        });

                    match self.settings.trigger_mode {
                        TriggerMode::Binary => {
                            let slider = ui.add(
                                Slider::new(
                                    &mut self.settings.binary_level,
                                    0.0..=1.0,
                                )
                                .fixed_decimals(2)
                                .text("Level"),
                            );
                            let slider = slider.on_hover_text(
                                "Fixed output intensity when the gate is open.",
                            );
                            if slider.changed() {
                                mark_custom(&mut self.settings);
                            }
                        }
                        TriggerMode::Hybrid => {
                            let slider = ui.add(
                                Slider::new(
                                    &mut self.settings.binary_level,
                                    0.0..=1.0,
                                )
                                .fixed_decimals(2)
                                .text("Level"),
                            );
                            if slider.changed() {
                                mark_custom(&mut self.settings);
                            }
                            let slider = ui.add(
                                Slider::new(
                                    &mut self.settings.hybrid_blend,
                                    0.0..=1.0,
                                )
                                .fixed_decimals(2)
                                .text("Blend"),
                            );
                            let slider = slider.on_hover_text(
                                "0 = fully dynamic (louder = more).\n\
                                 1 = fully binary (fixed level).",
                            );
                            if slider.changed() {
                                mark_custom(&mut self.settings);
                            }
                        }
                        _ => {}
                    }

                    if matches!(
                        self.settings.trigger_mode,
                        TriggerMode::Dynamic | TriggerMode::Hybrid
                    ) {
                        let curve_slider = ui.add(
                            Slider::new(
                                &mut self.settings.dynamic_curve,
                                0.4..=2.4,
                            )
                            .fixed_decimals(2)
                            .text("Range"),
                        );
                        let curve_slider = curve_slider.on_hover_text(
                            "Dynamic response curve.\n\
                             Lower = punchier/compressed.\n\
                             Higher = expanded range and smoother buildup.",
                        );
                        if curve_slider.changed() {
                            mark_custom(&mut self.settings);
                        }
                        if curve_slider.double_clicked() {
                            self.settings.dynamic_curve = defaults::DYNAMIC_CURVE;
                        }
                    }
                });

                ui.add_space(2.0);

                // ==========================================================
                // ADSR ENVELOPE — The shape of each vibration pulse
                // ==========================================================
                //
                // This is the heart of the synth. The ADSR controls are
                // displayed prominently with clear single-letter labels
                // matching the visual preview above.

                ui.horizontal(|ui| {
                    section_label(ui, "ENVELOPE", palette::ACCENT_TEAL);

                    ui.label(
                        RichText::new("A")
                            .size(11.0)
                            .color(palette::ACCENT_TEAL)
                            .strong()
                            .monospace(),
                    );
                    let slider = ui.add(
                        Slider::new(
                            &mut self.settings.attack_ms,
                            0.0..=500.0,
                        )
                        .logarithmic(true)
                        .suffix("ms"),
                    );
                    let slider = slider.on_hover_text(
                        "ATTACK — How fast vibration ramps up.\n\
                         0 = instant hit (drum-like).\n\
                         High = slow fade-in (pad-like).",
                    );
                    if slider.changed() {
                        mark_custom(&mut self.settings);
                    }
                    if slider.double_clicked() {
                        self.settings.attack_ms = defaults::ATTACK_MS;
                    }

                    ui.label(
                        RichText::new("D")
                            .size(11.0)
                            .color(palette::ACCENT_PURPLE)
                            .strong()
                            .monospace(),
                    );
                    let slider = ui.add(
                        Slider::new(
                            &mut self.settings.decay_ms,
                            0.0..=1000.0,
                        )
                        .logarithmic(true)
                        .suffix("ms"),
                    );
                    let slider = slider.on_hover_text(
                        "DECAY — How fast it drops from peak to sustain.\n\
                         Short = punchy, percussive.\n\
                         Long = gradual, natural.",
                    );
                    if slider.changed() {
                        mark_custom(&mut self.settings);
                    }
                    if slider.double_clicked() {
                        self.settings.decay_ms = defaults::DECAY_MS;
                    }

                    ui.label(
                        RichText::new("S")
                            .size(11.0)
                            .color(palette::ACCENT_AMBER)
                            .strong()
                            .monospace(),
                    );
                    let slider = ui.add(
                        Slider::new(
                            &mut self.settings.sustain_level,
                            0.0..=1.0,
                        )
                        .fixed_decimals(2),
                    );
                    let slider = slider.on_hover_text(
                        "SUSTAIN — Level held while audio stays above gate.\n\
                         0 = pluck/stab (no sustain).\n\
                         1.0 = organ/pad (full sustain).",
                    );
                    if slider.changed() {
                        mark_custom(&mut self.settings);
                    }
                    if slider.double_clicked() {
                        self.settings.sustain_level = defaults::SUSTAIN_LEVEL;
                    }

                    ui.label(
                        RichText::new("R")
                            .size(11.0)
                            .color(Color32::from_rgb(200, 100, 100))
                            .strong()
                            .monospace(),
                    );
                    let slider = ui.add(
                        Slider::new(
                            &mut self.settings.release_ms,
                            0.0..=2000.0,
                        )
                        .logarithmic(true)
                        .suffix("ms"),
                    );
                    let slider = slider.on_hover_text(
                        "RELEASE — How fast vibration fades after audio stops.\n\
                         Short = stops instantly.\n\
                         Long = lingering fade-out (reverb-like).",
                    );
                    if slider.changed() {
                        mark_custom(&mut self.settings);
                    }
                    if slider.double_clicked() {
                        self.settings.release_ms = defaults::RELEASE_MS;
                    }
                });

                // --- Envelope Curves (collapsible — power users only) ---
                ui.collapsing("Envelope Curves (advanced)", |ui| {
                    ui.horizontal(|ui| {
                        let s1 = ui.add(
                            Slider::new(
                                &mut self.settings.attack_curve,
                                0.1..=4.0,
                            )
                            .fixed_decimals(1)
                            .text("A curve"),
                        );
                        let s1 = s1.on_hover_text("< 1 = logarithmic (fast start).\n> 1 = exponential (slow start).\n1 = linear.");

                        let s2 = ui.add(
                            Slider::new(
                                &mut self.settings.decay_curve,
                                0.1..=4.0,
                            )
                            .fixed_decimals(1)
                            .text("D curve"),
                        );

                        let s3 = ui.add(
                            Slider::new(
                                &mut self.settings.release_curve,
                                0.1..=4.0,
                            )
                            .fixed_decimals(1)
                            .text("R curve"),
                        );

                        if s1.changed() || s2.changed() || s3.changed() {
                            mark_custom(&mut self.settings);
                        }
                    });
                });

                ui.add_space(2.0);

                // ==========================================================
                // CLIMAX ENGINE - Long-cycle build/tease/surge modulation
                // ==========================================================
                ui.horizontal(|ui| {
                    section_label(ui, "CLIMAX", palette::ACCENT_PINK);
                    let changed = ui
                        .checkbox(
                            &mut self.settings.climax_mode_enabled,
                            "Enable",
                        )
                        .on_hover_text(
                            "Adds a longer build-up cycle with tease dips,\n\
                             end surges, and faster micro-pulses.",
                        )
                        .changed();
                    if changed {
                        if self.settings.climax_mode_enabled {
                            self.climax_engine.reset(current_time_ms);
                        }
                        mark_custom(&mut self.settings);
                    }

                    if self.settings.climax_mode_enabled {
                        ui.label(
                            RichText::new(format!(
                                "CYCLE {:>5.1}%",
                                self.climax_phase * 100.0
                            ))
                            .size(9.0)
                            .color(palette::ACCENT_TEAL)
                            .monospace(),
                        );
                        if ui.button("Reset").clicked() {
                            self.climax_engine.reset(current_time_ms);
                        }
                    }
                });

                if self.settings.climax_mode_enabled {
                    ui.horizontal(|ui| {
                        let intensity = ui.add(
                            Slider::new(
                                &mut self.settings.climax_intensity,
                                0.0..=1.0,
                            )
                            .fixed_decimals(2)
                            .text("Heat"),
                        );
                        let intensity = intensity.on_hover_text(
                            "How strongly climax modulation pushes output upward.",
                        );
                        if intensity.changed() {
                            mark_custom(&mut self.settings);
                        }
                        if intensity.double_clicked() {
                            self.settings.climax_intensity =
                                defaults::CLIMAX_INTENSITY;
                        }

                        let mut build_secs =
                            self.settings.climax_build_up_ms / 1000.0;
                        let cycle = ui.add(
                            Slider::new(&mut build_secs, 8.0..=240.0)
                                .fixed_decimals(0)
                                .suffix("s")
                                .text("Cycle"),
                        );
                        let cycle = cycle.on_hover_text(
                            "Duration of one full build cycle.\n\
                             Lower values ramp faster; higher values edge longer.",
                        );
                        if cycle.changed() {
                            self.settings.climax_build_up_ms =
                                build_secs * 1000.0;
                            mark_custom(&mut self.settings);
                        }
                        if cycle.double_clicked() {
                            self.settings.climax_build_up_ms =
                                defaults::CLIMAX_BUILD_UP_MS;
                        }
                    });

                    ui.horizontal(|ui| {
                        let tease_ratio = ui.add(
                            Slider::new(
                                &mut self.settings.climax_tease_ratio,
                                0.05..=0.50,
                            )
                            .fixed_decimals(2)
                            .text("Tease Span"),
                        );
                        if tease_ratio.changed() {
                            mark_custom(&mut self.settings);
                        }
                        if tease_ratio.double_clicked() {
                            self.settings.climax_tease_ratio =
                                defaults::CLIMAX_TEASE_RATIO;
                        }

                        let tease_drop = ui.add(
                            Slider::new(
                                &mut self.settings.climax_tease_drop,
                                0.0..=0.90,
                            )
                            .fixed_decimals(2)
                            .text("Tease Dip"),
                        );
                        if tease_drop.changed() {
                            mark_custom(&mut self.settings);
                        }
                        if tease_drop.double_clicked() {
                            self.settings.climax_tease_drop =
                                defaults::CLIMAX_TEASE_DROP;
                        }

                        let surge_boost = ui.add(
                            Slider::new(
                                &mut self.settings.climax_surge_boost,
                                0.0..=1.20,
                            )
                            .fixed_decimals(2)
                            .text("Surge"),
                        );
                        if surge_boost.changed() {
                            mark_custom(&mut self.settings);
                        }
                        if surge_boost.double_clicked() {
                            self.settings.climax_surge_boost =
                                defaults::CLIMAX_SURGE_BOOST;
                        }

                        let pulse_depth = ui.add(
                            Slider::new(
                                &mut self.settings.climax_pulse_depth,
                                0.0..=0.45,
                            )
                            .fixed_decimals(2)
                            .text("Pulse"),
                        );
                        if pulse_depth.changed() {
                            mark_custom(&mut self.settings);
                        }
                        if pulse_depth.double_clicked() {
                            self.settings.climax_pulse_depth =
                                defaults::CLIMAX_PULSE_DEPTH;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Pattern")
                                .size(9.0)
                                .color(palette::TEXT_DIM)
                                .monospace(),
                        );
                        ComboBox::from_id_salt("climax_pattern")
                            .selected_text(match self.settings.climax_pattern {
                                ClimaxPattern::Wave => "Wave",
                                ClimaxPattern::Stairs => "Stairs",
                                ClimaxPattern::Surge => "Surge",
                            })
                            .show_ui(ui, |ui| {
                                let changed = false
                                    | ui
                                        .selectable_value(
                                            &mut self.settings.climax_pattern,
                                            ClimaxPattern::Wave,
                                            "Wave - smooth rise",
                                        )
                                        .changed()
                                    | ui
                                        .selectable_value(
                                            &mut self.settings.climax_pattern,
                                            ClimaxPattern::Stairs,
                                            "Stairs - stepped climb",
                                        )
                                        .changed()
                                    | ui
                                        .selectable_value(
                                            &mut self.settings.climax_pattern,
                                            ClimaxPattern::Surge,
                                            "Surge - hard end ramp",
                                        )
                                        .changed();
                                if changed {
                                    mark_custom(&mut self.settings);
                                }
                            });

                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Profiles")
                                .size(9.0)
                                .color(palette::TEXT_DIM)
                                .monospace(),
                        );
                        if ui.button("Edge").clicked() {
                            apply_climax_profile(
                                &mut self.settings,
                                ClimaxProfile::Edge,
                            );
                        }
                        if ui.button("Overload").clicked() {
                            apply_climax_profile(
                                &mut self.settings,
                                ClimaxProfile::Overload,
                            );
                        }
                        if ui.button("Punisher").clicked() {
                            apply_climax_profile(
                                &mut self.settings,
                                ClimaxProfile::Punisher,
                            );
                        }
                    });
                }

                ui.add_space(2.0);

                // ==========================================================
                // OUTPUT RANGE - Final output floor and ceiling
                // ==========================================================
                ui.horizontal(|ui| {
                    section_label(ui, "OUTPUT", palette::TEXT_DIM);
                    let slider = ui.add(
                        Slider::new(&mut self.settings.min_vibe, 0.0..=1.0)
                            .fixed_decimals(2)
                            .text("Floor"),
                    );
                    let slider = slider.on_hover_text(
                        "Minimum output level when active.\n\
                         Raise this to keep a base vibration always present.",
                    );
                    if slider.changed() {
                        mark_custom(&mut self.settings);
                    }
                    if slider.double_clicked() {
                        self.settings.min_vibe = defaults::MIN_VIBE;
                    }

                    let slider = ui.add(
                        Slider::new(&mut self.settings.max_vibe, 0.0..=1.0)
                            .fixed_decimals(2)
                            .text("Ceiling"),
                    );
                    let slider = slider.on_hover_text(
                        "Maximum output level. Limits intensity to protect\n\
                         the device or your... comfort threshold.",
                    );
                    if slider.changed() {
                        mark_custom(&mut self.settings);
                    }
                    if slider.double_clicked() {
                        self.settings.max_vibe = defaults::MAX_VIBE;
                    }
                });
            } else {
                // --- Legacy pipeline UI ---
                ui.horizontal(|ui| {
                    let r1 = ui.label("Main volume: ");
                    let mut vol_pct = self.settings.main_volume * 100.0;
                    let slider = ui.add(
                        Slider::new(&mut vol_pct, 0.0..=500.0).suffix("%"),
                    );
                    if slider.changed() {
                        self.settings.main_volume = vol_pct / 100.0;
                    }
                    if slider.double_clicked() {
                        self.settings.main_volume = defaults::MAIN_VOLUME;
                    }
                    let mut text = LayoutJob::default();
                    text.append(
                        "Controls global volume\n",
                        0.0,
                        TextFormat::default(),
                    );
                    text.append(
                        "Warning!!!",
                        0.0,
                        TextFormat {
                            color: Color32::RED,
                            ..Default::default()
                        },
                    );
                    text.append(
                        " Exponential: 200% = 4x!",
                        0.0,
                        TextFormat::default(),
                    );
                    r1.union(slider).on_hover_text_at_pointer(text);

                    let mut lpf = self.settings.low_pass_freq.load();
                    let r1 = ui.label("Low pass: ");
                    let slider = ui.add(
                        Slider::new(&mut lpf, 0.0..=20_000.0)
                            .logarithmic(true)
                            .integer(),
                    );
                    if slider.changed() {
                        self.settings.low_pass_freq.store(lpf);
                    }
                    if slider.double_clicked() {
                        self.settings
                            .low_pass_freq
                            .store(defaults::LOW_PASS_FREQ);
                    }
                    r1.union(slider).on_hover_text_at_pointer(
                        "Filters frequencies above this value",
                    );
                });

                ui.horizontal(|ui| {
                    if self.settings.enable_persistence {
                        ui.label("Hold Delay: ");
                        ui.add(
                            Slider::new(
                                &mut self.settings.hold_delay_ms,
                                0.0..=500.0,
                            )
                            .integer()
                            .logarithmic(true),
                        );
                        ui.label("Decay Rate: ");
                        ui.add(
                            Slider::new(
                                &mut self.settings.decay_rate_per_sec,
                                0.01..=4.0,
                            )
                            .fixed_decimals(2),
                        );
                    }
                });
            }

            ui.separator();

            // ============================================================
            // Devices (updated to use processed_output)
            // ============================================================
            ui.heading("Devices");
            if let Some(client) = &self.client {
                for bp_device in client.devices() {
                    let device_name = bp_device.name().to_string();
                    if !self.devices.contains_key(&device_name) {
                        let props = Arc::new(Mutex::new(DeviceProps::new(
                            &self.runtime,
                            bp_device.clone(),
                            &self.settings,
                        )));

                        // Spawn per-device task.
                        // Reads processed_output (after envelope/gate).
                        // RATE LIMITED: Only sends BT commands when intensity
                        // actually changes, at max 50Hz. Prevents Intiface
                        // crash/lag from BT packet flooding.
                        let task = self.runtime.spawn({
                            let bp_device = bp_device.clone();
                            let props = props.clone();
                            let processed = self.processed_output.clone();
                            async move {
                                // Rate limiter state (from Gemini/ChloeVibes approach)
                                let mut last_sent: f64 = -1.0;
                                // 0.5% dead-band: the Domi 2 has decent granularity.
                                // Finer resolution captures subtle dynamics that
                                // make the difference between "buzzing" and "alive".
                                let resolution: f64 = 0.005;

                                loop {
                                    let now = tokio::time::Instant::now();
                                    let vibration_level = processed.load();
                                    let mut should_send = false;
                                    let mut vibrate_cmd = None;
                                    let mut oscillate_cmd = None;
                                    {
                                        let guard = props.lock().unwrap();
                                        let speed =
                                            guard.calculate_output(
                                                vibration_level,
                                            );
                                        let speed_f64 = speed as f64;

                                        // === RATE LIMITER ===
                                        // Only send if:
                                        //   a) intensity changed by more than 1%, OR
                                        //   b) this is a hard stop (going to zero)
                                        let change = (speed_f64 - last_sent).abs();
                                        let is_hard_stop = speed_f64 < 0.005
                                            && last_sent >= 0.005;

                                        if change >= resolution || is_hard_stop {
                                            should_send = true;
                                            last_sent = speed_f64;
                                        }

                                        if should_send {
                                            if !guard.vibrators.is_empty() {
                                                vibrate_cmd = Some(
                                                    ScalarValueCommand::ScalarValueVec(
                                                        guard
                                                            .vibrators
                                                            .iter()
                                                            .map(|v| {
                                                                if v.is_enabled
                                                                    && guard
                                                                        .is_enabled
                                                                {
                                                                    (speed
                                                                        * v.multiplier)
                                                                        .clamp(
                                                                            0.0,
                                                                            v.max,
                                                                        )
                                                                        .min_cutoff(
                                                                            v.min,
                                                                        )
                                                                        as f64
                                                                } else {
                                                                    0.0
                                                                }
                                                            })
                                                            .collect(),
                                                    ),
                                                );
                                            }
                                            if !guard.oscillators.is_empty() {
                                                oscillate_cmd = Some(
                                                    ScalarValueCommand::ScalarValueVec(
                                                        guard
                                                            .oscillators
                                                            .iter()
                                                            .map(|o| {
                                                                if o.is_enabled
                                                                    && guard
                                                                        .is_enabled
                                                                {
                                                                    (speed
                                                                        * o.multiplier)
                                                                        .clamp(
                                                                            0.0,
                                                                            o.max,
                                                                        )
                                                                        .min_cutoff(
                                                                            o.min,
                                                                        )
                                                                        as f64
                                                                } else {
                                                                    0.0
                                                                }
                                                            })
                                                            .collect(),
                                                    ),
                                                );
                                            }
                                        }
                                    };

                                    // Only hit the BT stack when we have something new
                                    if should_send {
                                        if let Some(cmd) = vibrate_cmd {
                                            if let Err(e) =
                                                bp_device.vibrate(&cmd).await
                                            {
                                                eprintln!(
                                                    "Vibrate error: {e}"
                                                );
                                            }
                                        }
                                        if let Some(cmd) = oscillate_cmd {
                                            if let Err(e) =
                                                bp_device.oscillate(&cmd).await
                                            {
                                                eprintln!(
                                                    "Oscillate error: {e}"
                                                );
                                            }
                                        }
                                    }

                                    // 20ms = 50Hz max update rate.
                                    // Most BT toys can't actually process faster
                                    // than ~30-50Hz anyway, so 5ms was pure waste.
                                    tokio::time::sleep_until(
                                        now + Duration::from_millis(20),
                                    )
                                    .await;
                                }
                            }
                        });

                        let device = Device {
                            props,
                            _task: task,
                        };
                        self.devices.insert(device_name.clone(), device);
                    }
                    let device =
                        self.devices.get_mut(&device_name).unwrap();
                    device_widget(
                        ui,
                        bp_device,
                        &mut device.props.lock().unwrap(),
                        self.vibration_level,
                        &self.runtime,
                    );
                }
            }
        });

        settings_window_widget(ctx, &mut self.show_settings, &mut self.settings);
        ctx.request_repaint();
    }
}

// ---------------------------------------------------------------------------
// Visualization Drawing Functions
// ---------------------------------------------------------------------------

/// Draw the ADSR envelope shape preview — shows the static curve shape
/// plus a phase indicator dot showing where the envelope currently is.
fn draw_adsr_envelope(
    painter: &egui::Painter,
    rect: Rect,
    attack_ms: f32,
    decay_ms: f32,
    sustain_level: f32,
    release_ms: f32,
    attack_curve: f32,
    decay_curve: f32,
    release_curve: f32,
    envelope: &EnvelopeProcessor,
) {
    // Background
    painter.rect_filled(rect, 8.0, palette::BG_SECONDARY);
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, Color32::from_rgba_premultiplied(124, 58, 237, 25)),
        StrokeKind::Outside,
    );

    let w = rect.width();
    let h = rect.height();
    let padding = 8.0;
    let draw_w = w - padding * 2.0;
    let draw_h = h - padding * 2.0;
    let origin = pos2(rect.min.x + padding, rect.max.y - padding);

    // Grid lines
    for i in 1..=4 {
        let y = origin.y - draw_h * (i as f32 / 4.0);
        painter.line_segment(
            [pos2(origin.x, y), pos2(origin.x + draw_w, y)],
            Stroke::new(0.5, palette::GRID_LINE),
        );
    }

    // Calculate phase widths
    let sustain_display = 200.0_f32;
    let total = attack_ms + decay_ms + sustain_display + release_ms;
    if total <= 0.0 {
        return;
    }
    let time_scale = draw_w / total;

    // Build the curve path
    let mut points: Vec<Pos2> = Vec::with_capacity(256);
    points.push(origin);

    let resolution = 2.0_f32;
    let mut current_x = 0.0_f32;

    // Attack
    let mut t = 0.0_f32;
    while t <= attack_ms {
        let progress = if attack_ms > 0.0 { t / attack_ms } else { 1.0 };
        let curved = progress.powf(attack_curve);
        let x = origin.x + t * time_scale;
        let y = origin.y - curved * draw_h;
        points.push(pos2(x, y));
        current_x = t * time_scale;
        t += resolution;
    }

    // Decay
    let attack_x = current_x;
    let sustain_y = origin.y - sustain_level * draw_h;

    t = 0.0;
    while t <= decay_ms {
        let progress = if decay_ms > 0.0 { t / decay_ms } else { 1.0 };
        let decay_factor = (1.0 - progress).powf(decay_curve);
        let value = sustain_level + (1.0 - sustain_level) * decay_factor;
        let x = origin.x + attack_x + t * time_scale;
        let y = origin.y - value * draw_h;
        points.push(pos2(x, y));
        current_x = attack_x + t * time_scale;
        t += resolution;
    }

    // Sustain
    let sustain_end_x = current_x + sustain_display * time_scale;
    points.push(pos2(origin.x + sustain_end_x, sustain_y));

    // Release
    t = 0.0;
    while t <= release_ms {
        let progress = if release_ms > 0.0 {
            t / release_ms
        } else {
            1.0
        };
        let release_factor = (1.0 - progress).powf(release_curve);
        let value = sustain_level * release_factor;
        let x = origin.x + sustain_end_x + t * time_scale;
        let y = origin.y - value * draw_h;
        points.push(pos2(x, y));
        t += resolution;
    }

    // Draw filled area under curve (as quad strips — the curve is not convex)
    for i in 0..points.len().saturating_sub(1) {
        let p0 = points[i];
        let p1 = points[i + 1];
        let quad = vec![pos2(p0.x, origin.y), p0, p1, pos2(p1.x, origin.y)];
        painter.add(Shape::convex_polygon(
            quad,
            Color32::from_rgba_premultiplied(16, 185, 129, 15),
            Stroke::NONE,
        ));
    }

    // Draw the curve line with glow
    if points.len() >= 2 {
        // Glow pass (wider, dimmer)
        painter.add(Shape::line(
            points.clone(),
            Stroke::new(4.0, Color32::from_rgba_premultiplied(16, 185, 129, 40)),
        ));
        // Main line
        painter.add(Shape::line(
            points.clone(),
            Stroke::new(2.0, palette::ACCENT_TEAL),
        ));
    }

    // Phase separator lines
    let phase_xs = [
        origin.x + attack_ms * time_scale,
        origin.x + (attack_ms + decay_ms) * time_scale,
        origin.x + sustain_end_x,
    ];
    for &px in &phase_xs {
        painter.line_segment(
            [pos2(px, origin.y), pos2(px, origin.y - draw_h)],
            Stroke::new(0.5, Color32::from_rgba_premultiplied(255, 255, 255, 20)),
        );
    }

    // Phase labels — color-coded to match the slider labels
    let label_y = origin.y - 6.0;
    let font = FontId::monospace(9.0);

    let a_center = origin.x + attack_ms * time_scale * 0.5;
    painter.text(
        pos2(a_center, label_y),
        egui::Align2::CENTER_BOTTOM,
        "A",
        font.clone(),
        palette::ACCENT_TEAL,
    );

    let d_center = origin.x + attack_ms * time_scale + decay_ms * time_scale * 0.5;
    painter.text(
        pos2(d_center, label_y),
        egui::Align2::CENTER_BOTTOM,
        "D",
        font.clone(),
        palette::ACCENT_PURPLE,
    );

    let s_center =
        origin.x + (attack_ms + decay_ms) * time_scale + sustain_display * time_scale * 0.5;
    painter.text(
        pos2(s_center, label_y),
        egui::Align2::CENTER_BOTTOM,
        "S",
        font.clone(),
        palette::ACCENT_AMBER,
    );

    let r_center = origin.x + sustain_end_x + release_ms * time_scale * 0.5;
    painter.text(
        pos2(r_center, label_y),
        egui::Align2::CENTER_BOTTOM,
        "R",
        font.clone(),
        Color32::from_rgb(200, 100, 100),
    );

    // Current phase indicator dot
    let dot_color = match envelope.state {
        EnvelopeState::Attack => palette::ACCENT_TEAL,
        EnvelopeState::Decay => palette::ACCENT_PURPLE,
        EnvelopeState::Sustain => palette::ACCENT_AMBER,
        EnvelopeState::Release => Color32::from_rgb(200, 100, 100),
        EnvelopeState::Idle => palette::TEXT_DIM,
    };
    let dot_y = origin.y - envelope.value * draw_h;

    // Approximate x position based on current phase
    let dot_x = match envelope.state {
        EnvelopeState::Attack => origin.x + envelope.value * attack_ms * time_scale,
        EnvelopeState::Decay => {
            origin.x
                + attack_ms * time_scale
                + (1.0 - (envelope.value - sustain_level) / (1.0 - sustain_level).max(0.01))
                    * decay_ms
                    * time_scale
        }
        EnvelopeState::Sustain => {
            origin.x + (attack_ms + decay_ms) * time_scale + sustain_display * time_scale * 0.5
        }
        EnvelopeState::Release => {
            let progress = 1.0 - (envelope.value / sustain_level.max(0.01)).clamp(0.0, 1.0);
            origin.x + sustain_end_x + progress * release_ms * time_scale
        }
        EnvelopeState::Idle => origin.x,
    };

    if envelope.state != EnvelopeState::Idle {
        // Glow
        painter.circle_filled(
            pos2(dot_x.clamp(rect.min.x, rect.max.x), dot_y),
            8.0,
            Color32::from_rgba_premultiplied(dot_color.r(), dot_color.g(), dot_color.b(), 40),
        );
        // Dot
        painter.circle_filled(
            pos2(dot_x.clamp(rect.min.x, rect.max.x), dot_y),
            4.0,
            dot_color,
        );

        // Phase state label in top right
        let state_text = match envelope.state {
            EnvelopeState::Attack => "ATTACK",
            EnvelopeState::Decay => "DECAY",
            EnvelopeState::Sustain => "SUSTAIN",
            EnvelopeState::Release => "RELEASE",
            EnvelopeState::Idle => "",
        };
        painter.text(
            pos2(rect.max.x - padding, rect.min.y + padding + 10.0),
            egui::Align2::RIGHT_TOP,
            state_text,
            FontId::monospace(10.0),
            dot_color,
        );
    }
}

/// Draw the rolling output history as a waveform with energy overlay.
fn draw_output_history(
    painter: &egui::Painter,
    rect: Rect,
    output_history: &VecDeque<f32>,
    energy_history: &VecDeque<f32>,
    gate_is_open: bool,
    gate_threshold: f32,
) {
    painter.rect_filled(rect, 8.0, palette::BG_SECONDARY);
    let border_color = if gate_is_open {
        Color32::from_rgba_premultiplied(236, 72, 153, 70) // neon pink glow when open
    } else {
        Color32::from_rgba_premultiplied(124, 58, 237, 30) // subtle purple when closed
    };
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, border_color),
        StrokeKind::Outside,
    );

    let w = rect.width();
    let h = rect.height();
    let padding = 6.0;
    let draw_w = w - padding * 2.0;
    let draw_h = h - padding * 2.0;

    // Grid lines
    for i in 1..=3 {
        let y = rect.min.y + padding + draw_h * (1.0 - i as f32 / 3.0);
        painter.line_segment(
            [pos2(rect.min.x + padding, y), pos2(rect.max.x - padding, y)],
            Stroke::new(0.5, palette::GRID_LINE),
        );
    }

    // Gate threshold line (neon pink to match THRESHOLD label)
    let thresh_y = rect.min.y + padding + draw_h * (1.0 - gate_threshold);
    // Dashed line effect
    let dash_len = 6.0;
    let gap_len = 4.0;
    let x_start = rect.min.x + padding;
    let x_end = rect.max.x - padding;
    let mut x = x_start;
    let thresh_color = if gate_is_open {
        Color32::from_rgba_premultiplied(236, 72, 153, 140)
    } else {
        Color32::from_rgba_premultiplied(236, 72, 153, 70)
    };
    while x < x_end {
        let seg_end = (x + dash_len).min(x_end);
        painter.line_segment(
            [pos2(x, thresh_y), pos2(seg_end, thresh_y)],
            Stroke::new(1.5, thresh_color),
        );
        x = seg_end + gap_len;
    }

    let len = output_history.len();
    if len < 2 {
        return;
    }

    // Energy history (dim background fill — draw as vertical strips)
    for i in 0..len.saturating_sub(1) {
        let val0 = energy_history[i].clamp(0.0, 1.0);
        let val1 = energy_history[i + 1].clamp(0.0, 1.0);
        let x0 = rect.min.x + padding + (i as f32 / (len - 1) as f32) * draw_w;
        let x1 = rect.min.x + padding + ((i + 1) as f32 / (len - 1) as f32) * draw_w;
        let y0 = rect.min.y + padding + draw_h * (1.0 - val0);
        let y1 = rect.min.y + padding + draw_h * (1.0 - val1);
        let bottom = rect.min.y + padding + draw_h;

        let quad = vec![
            pos2(x0, bottom),
            pos2(x0, y0),
            pos2(x1, y1),
            pos2(x1, bottom),
        ];
        painter.add(Shape::convex_polygon(
            quad,
            Color32::from_rgba_premultiplied(124, 58, 237, 12),
            Stroke::NONE,
        ));
    }

    // Output history line
    let mut line_points: Vec<Pos2> = Vec::with_capacity(len);
    for (i, &val) in output_history.iter().enumerate() {
        let x = rect.min.x + padding + (i as f32 / (len - 1) as f32) * draw_w;
        let y = rect.min.y + padding + draw_h * (1.0 - val.clamp(0.0, 1.0));
        line_points.push(pos2(x, y));
    }

    if line_points.len() >= 2 {
        // Glow
        painter.add(Shape::line(
            line_points.clone(),
            Stroke::new(3.0, Color32::from_rgba_premultiplied(16, 185, 129, 35)),
        ));
        // Main line
        painter.add(Shape::line(
            line_points,
            Stroke::new(1.5, palette::ACCENT_TEAL),
        ));
    }
}

/// Draw prettier spectrum frequency bars.
fn draw_spectrum_bars(
    painter: &egui::Painter,
    rect: Rect,
    band_energies: &[f32],
    gate_is_open: bool,
) {
    painter.rect_filled(rect, 8.0, palette::BG_SECONDARY);
    let border_color = if gate_is_open {
        Color32::from_rgba_premultiplied(16, 185, 129, 50) // teal glow when active
    } else {
        Color32::from_rgba_premultiplied(124, 58, 237, 30)
    };
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, border_color),
        StrokeKind::Outside,
    );

    let padding = 8.0;
    let draw_w = rect.width() - padding * 2.0;
    let draw_h = rect.height() - padding * 2.0;
    let num_bands = band_energies.len().max(1);
    let gap = 3.0;
    let bar_w = (draw_w - gap * (num_bands as f32 - 1.0)) / num_bands as f32;

    for (i, &energy) in band_energies.iter().enumerate() {
        let x = rect.min.x + padding + i as f32 * (bar_w + gap);
        let bar_h = energy.clamp(0.0, 1.0) * draw_h;
        let y = rect.max.y - padding - bar_h;

        // Color gradient based on band index (low=purple, mid=teal, high=red)
        let t = i as f32 / (num_bands - 1).max(1) as f32;
        let (r, g, b) = if t < 0.5 {
            // Purple -> Teal
            let s = t * 2.0;
            (
                (124.0 * (1.0 - s) + 16.0 * s) as u8,
                (58.0 * (1.0 - s) + 185.0 * s) as u8,
                (237.0 * (1.0 - s) + 129.0 * s) as u8,
            )
        } else {
            // Teal -> Red/amber
            let s = (t - 0.5) * 2.0;
            (
                (16.0 * (1.0 - s) + 239.0 * s) as u8,
                (185.0 * (1.0 - s) + 68.0 * s) as u8,
                (129.0 * (1.0 - s) + 68.0 * s) as u8,
            )
        };

        let alpha = if gate_is_open { 220 } else { 100 };
        let bar_color = Color32::from_rgba_premultiplied(r, g, b, alpha);

        let bar_rect = Rect::from_min_size(pos2(x, y), vec2(bar_w, bar_h));
        painter.rect_filled(bar_rect, 2.0, bar_color);

        // Subtle reflection below
        if bar_h > 2.0 {
            let reflection_h = (bar_h * 0.15).min(6.0);
            let refl_rect =
                Rect::from_min_size(pos2(x, rect.max.y - padding), vec2(bar_w, reflection_h));
            painter.rect_filled(
                refl_rect,
                1.0,
                Color32::from_rgba_premultiplied(r, g, b, 30),
            );
        }

        // Band label
        if i < BAND_NAMES.len() {
            painter.text(
                pos2(x + bar_w * 0.5, rect.max.y - 2.0),
                egui::Align2::CENTER_BOTTOM,
                BAND_NAMES[i],
                FontId::monospace(7.0),
                palette::TEXT_DIM,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// UI Helper Functions
// ---------------------------------------------------------------------------

/// Draw a consistent section label (e.g., "GATE", "ENVELOPE", "INPUT")
fn section_label(ui: &mut Ui, text: &str, color: Color32) {
    let label = RichText::new(text).size(9.0).color(color).monospace();
    ui.add_sized([62.0, 18.0], egui::Label::new(label));
}

#[derive(Clone, Copy)]
enum ClimaxProfile {
    Edge,
    Overload,
    Punisher,
}

#[derive(Clone, Copy)]
enum ChloeRhythmProfile {
    Loose,
    Medium,
    Ultimate,
}

fn apply_climax_profile(settings: &mut Settings, profile: ClimaxProfile) {
    settings.climax_mode_enabled = true;
    match profile {
        ClimaxProfile::Edge => {
            settings.climax_pattern = ClimaxPattern::Wave;
            settings.climax_intensity = 0.62;
            settings.climax_build_up_ms = 130_000.0;
            settings.climax_tease_ratio = 0.28;
            settings.climax_tease_drop = 0.48;
            settings.climax_surge_boost = 0.34;
            settings.climax_pulse_depth = 0.16;
            settings.min_vibe = settings.min_vibe.max(0.08);
        }
        ClimaxProfile::Overload => {
            settings.climax_pattern = ClimaxPattern::Surge;
            settings.climax_intensity = 0.88;
            settings.climax_build_up_ms = 75_000.0;
            settings.climax_tease_ratio = 0.16;
            settings.climax_tease_drop = 0.26;
            settings.climax_surge_boost = 0.82;
            settings.climax_pulse_depth = 0.28;
            settings.min_vibe = settings.min_vibe.max(0.14);
        }
        ClimaxProfile::Punisher => {
            settings.climax_pattern = ClimaxPattern::Stairs;
            settings.climax_intensity = 1.0;
            settings.climax_build_up_ms = 55_000.0;
            settings.climax_tease_ratio = 0.12;
            settings.climax_tease_drop = 0.15;
            settings.climax_surge_boost = 1.0;
            settings.climax_pulse_depth = 0.36;
            settings.min_vibe = settings.min_vibe.max(0.20);
        }
    }
    if settings.max_vibe <= settings.min_vibe {
        settings.max_vibe = (settings.min_vibe + 0.1).min(1.0);
    }
    settings.current_preset_name.clear();
}

fn apply_chloe_rhythm_profile(settings: &mut Settings, profile: ChloeRhythmProfile) {
    settings.use_advanced_processing = true;
    settings.auto_gate_amount = 0.0;
    settings.gate_smoothing = 0.08;
    settings.trigger_mode = TriggerMode::Hybrid;
    settings.frequency_mode = FrequencyMode::LowPass;
    settings.attack_curve = 0.42;
    settings.decay_curve = 1.65;
    settings.release_curve = 1.95;

    match profile {
        ChloeRhythmProfile::Loose => {
            settings.current_preset_name = String::from("Chloe Loose");
            settings.main_volume = 1.45;
            settings.target_frequency = 175.0;
            settings.gate_threshold = 0.17;
            settings.threshold_knee = 0.17;
            settings.dynamic_curve = 1.35;
            settings.binary_level = 0.64;
            settings.hybrid_blend = 0.34;
            settings.attack_ms = 4.5;
            settings.decay_ms = 78.0;
            settings.sustain_level = 0.50;
            settings.release_ms = 95.0;
            settings.input_rise_ms = 10.0;
            settings.input_fall_ms = 45.0;
            settings.output_slew_ms = 12.0;
            settings.min_vibe = 0.03;
            settings.max_vibe = 0.92;
            settings.climax_mode_enabled = false;
        }
        ChloeRhythmProfile::Medium => {
            settings.current_preset_name = String::from("Chloe Medium");
            settings.main_volume = 1.85;
            settings.target_frequency = 150.0;
            settings.gate_threshold = 0.185;
            settings.threshold_knee = 0.14;
            settings.dynamic_curve = 1.52;
            settings.binary_level = 0.74;
            settings.hybrid_blend = 0.43;
            settings.attack_ms = 2.6;
            settings.decay_ms = 58.0;
            settings.sustain_level = 0.42;
            settings.release_ms = 78.0;
            settings.input_rise_ms = 7.0;
            settings.input_fall_ms = 28.0;
            settings.output_slew_ms = 8.0;
            settings.min_vibe = 0.07;
            settings.max_vibe = 1.0;
            settings.climax_mode_enabled = true;
            settings.climax_pattern = ClimaxPattern::Wave;
            settings.climax_intensity = 0.58;
            settings.climax_build_up_ms = 75_000.0;
            settings.climax_tease_ratio = 0.16;
            settings.climax_tease_drop = 0.18;
            settings.climax_surge_boost = 0.42;
            settings.climax_pulse_depth = 0.20;
        }
        ChloeRhythmProfile::Ultimate => {
            settings.current_preset_name = String::from("Chloe Ultimate");
            settings.main_volume = 2.10;
            settings.target_frequency = 125.0;
            settings.gate_threshold = 0.16;
            settings.threshold_knee = 0.14;
            settings.dynamic_curve = 1.55;
            settings.binary_level = 0.90;
            settings.hybrid_blend = 0.52;
            // Faster attack = feel every beat land. Shorter decay = punchy contrast.
            settings.attack_ms = 1.0;
            settings.decay_ms = 45.0;
            // Higher sustain = more body between beats (avoids feeling "empty").
            settings.sustain_level = 0.48;
            settings.release_ms = 65.0;
            // Tighter response chain for maximum music sync:
            settings.input_rise_ms = 3.5;
            settings.input_fall_ms = 16.0;
            settings.output_slew_ms = 3.0;
            settings.min_vibe = 0.08;
            settings.max_vibe = 1.0;
            settings.climax_mode_enabled = true;
            settings.climax_pattern = ClimaxPattern::Surge;
            settings.climax_intensity = 0.82;
            // Longer cycle = more anticipation = more powerful peak.
            settings.climax_build_up_ms = 60_000.0;
            // More pronounced tease creates stronger contrast before surge.
            settings.climax_tease_ratio = 0.18;
            settings.climax_tease_drop = 0.30;
            settings.climax_surge_boost = 0.90;
            // Lower pulse depth — let the music rhythm dominate, not the micro-pulse.
            settings.climax_pulse_depth = 0.22;
        }
    }
}

/// Mark settings as custom (no longer matching a preset)
fn mark_custom(settings: &mut Settings) {
    settings.current_preset_name.clear();
}

fn sanitize_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn normalize_capture_energy(value: f32) -> f32 {
    // Gentler normalization that preserves dynamic range.
    // Old: value*10 then pow(0.75) — anything above 0.1 saturated, killing dynamics.
    // New: value*6 then pow(0.65) — broader usable range before saturation,
    // with slightly more compression to keep quiet signals visible.
    // This means:
    //   0.02 raw → 0.08 output  (quiet passage — perceptible but subtle)
    //   0.05 raw → 0.18 output  (moderate — clear vibration)
    //   0.10 raw → 0.31 output  (loud — strong vibration)
    //   0.16 raw → 0.55 output  (very loud — near peak)
    //   0.25 raw → 0.80 output  (peak — near max)
    let boosted = sanitize_unit(value * 6.0);
    sanitize_unit(boosted.powf(0.65))
}

fn smoothing_alpha(delta_time_s: f32, time_ms: f32) -> f32 {
    if time_ms <= 1.0 {
        1.0
    } else {
        let tau = (time_ms / 1000.0).max(0.001);
        (1.0 - (-delta_time_s / tau).exp()).clamp(0.0, 1.0)
    }
}

fn set_capture_status(status: &Arc<Mutex<String>>, value: impl Into<String>) {
    if let Ok(mut guard) = status.lock() {
        *guard = value.into();
    }
}

// ---------------------------------------------------------------------------
// Settings Window
// ---------------------------------------------------------------------------

fn settings_window_widget(ctx: &egui::Context, show_settings: &mut bool, settings: &mut Settings) {
    Window::new("Settings")
        .open(show_settings)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.checkbox(&mut settings.use_dark_mode, "Use dark mode");
            ui.checkbox(
                &mut settings.start_scanning_on_startup,
                "Start scanning on startup",
            );
            ui.checkbox(
                &mut settings.save_device_settings,
                "Remember device settings",
            );

            let mut current_value = settings.use_polling_rate.load(Ordering::Relaxed);
            if ui
                .checkbox(&mut current_value, "Use fixed polling rate")
                .changed()
            {
                settings
                    .use_polling_rate
                    .store(current_value, Ordering::Relaxed);
            }

            ui.separator();

            // Pipeline toggle
            ui.checkbox(
                &mut settings.use_advanced_processing,
                "Use advanced processing (ChloeVibes engine)",
            )
            .on_hover_text(
                "When enabled, uses FFT spectral analysis, ADSR envelope,\n\
                 and gate with hysteresis. When disabled, uses original\n\
                 simple RMS processing.",
            );

            if !settings.use_advanced_processing {
                ui.checkbox(
                    &mut settings.enable_persistence,
                    "Enable vibration persistence (legacy)",
                );
            }
        });
}

// ---------------------------------------------------------------------------
// Device Widget (unchanged from original)
// ---------------------------------------------------------------------------

struct VibratorProps {
    is_enabled: bool,
    multiplier: f32,
    min: f32,
    max: f32,
}

impl Default for VibratorProps {
    fn default() -> Self {
        Self {
            is_enabled: true,
            multiplier: 1.0,
            min: 0.0,
            max: 1.0,
        }
    }
}

struct OscillatorProps {
    is_enabled: bool,
    multiplier: f32,
    min: f32,
    max: f32,
}

impl Default for OscillatorProps {
    fn default() -> Self {
        Self {
            is_enabled: true,
            multiplier: 1.0,
            min: 0.0,
            max: 1.0,
        }
    }
}

fn device_widget(
    ui: &mut Ui,
    device: Arc<ButtplugClientDevice>,
    props: &mut DeviceProps,
    vibration_level: f32,
    runtime: &Runtime,
) {
    ui.group(|ui| {
        if cfg!(debug_assertions) {
            ui.label(format!("({}) {}", device.index(), device.name()));
        } else {
            ui.label(device.name());
        }

        if let Some(bat) = props.battery_state.get_level() {
            ui.label(format!("Battery: {}%", bat * 100.0));
        }

        let (speed, cutoff) = props.calculate_visual_output(vibration_level);

        ui.horizontal(|ui| {
            let label = if props.is_enabled {
                "Enabled"
            } else {
                "Enable"
            };
            let enable_button =
                Button::new(RichText::new(label).color(Color32::WHITE)).fill(if props.is_enabled {
                    palette::ACCENT_PURPLE
                } else {
                    palette::BG_TERTIARY
                });
            ui.vertical(|ui| {
                ui.group(|ui| {
                    if ui.add_sized([60.0, 60.0], enable_button).clicked() {
                        props.is_enabled = !props.is_enabled;
                        if !props.is_enabled {
                            runtime.spawn(device.stop());
                        }
                    }
                });
            });
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("{:.2}%", speed * 100.0));
                    if cutoff {
                        ui.visuals_mut().selection.bg_fill = Color32::RED;
                    }
                    if !props.is_enabled {
                        ui.visuals_mut().selection.bg_fill = Color32::GRAY;
                    }
                    ui.add(ProgressBar::new(speed));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Multiplier: ");
                    let slider = ui.add(Slider::new(&mut props.multiplier, 0.0..=20.0));
                    if slider.double_clicked() {
                        props.multiplier = 1.0;
                    }
                    ui.label("Minimum (cut-off): ");
                    let slider = ui.add(Slider::new(&mut props.min, 0.0..=1.0).fixed_decimals(2));
                    if slider.double_clicked() {
                        props.min = 0.0;
                    }
                    ui.label("Maximum: ");
                    let slider = ui.add(Slider::new(&mut props.max, 0.0..=1.0).fixed_decimals(2));
                    if slider.double_clicked() {
                        props.max = 1.0;
                    }
                });
                if !props.vibrators.is_empty() {
                    ui.push_id(format!("vibrators_{}", device.name()), |ui| {
                        ui.collapsing("Vibrators", |ui| {
                            ui.group(|ui| {
                                for (i, vibe) in props.vibrators.iter_mut().enumerate() {
                                    vibrator_widget(ui, i, vibe);
                                }
                            });
                        });
                    });
                }
                if !props.oscillators.is_empty() {
                    ui.push_id(format!("oscillators_{}", device.name()), |ui| {
                        ui.collapsing("Oscillators", |ui| {
                            ui.group(|ui| {
                                for (i, osc) in props.oscillators.iter_mut().enumerate() {
                                    oscillator_widget(ui, i, osc);
                                }
                            });
                        });
                    });
                }
            })
        });
    });
}

fn vibrator_widget(ui: &mut Ui, index: usize, vibe: &mut VibratorProps) {
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Vibe {index}: "));
        let label = if vibe.is_enabled { "Enabled" } else { "Enable" };
        let button =
            Button::new(RichText::new(label).color(Color32::WHITE)).fill(if vibe.is_enabled {
                palette::ACCENT_PURPLE
            } else {
                palette::BG_TERTIARY
            });
        if ui.add(button).clicked() {
            vibe.is_enabled = !vibe.is_enabled;
        }
        ui.label("Multiplier: ");
        let slider = ui.add(Slider::new(&mut vibe.multiplier, 0.0..=5.0));
        if slider.double_clicked() {
            vibe.multiplier = 1.0;
        }
        ui.label("Minimum: ");
        let slider = ui.add(Slider::new(&mut vibe.min, 0.0..=1.0).fixed_decimals(2));
        if slider.double_clicked() {
            vibe.min = 0.0;
        }
        ui.label("Maximum: ");
        let slider = ui.add(Slider::new(&mut vibe.max, 0.0..=1.0).fixed_decimals(2));
        if slider.double_clicked() {
            vibe.max = 1.0;
        }
    });
}

fn oscillator_widget(ui: &mut Ui, index: usize, osc: &mut OscillatorProps) {
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Oscillator {index}: "));
        let label = if osc.is_enabled { "Enabled" } else { "Enable" };
        let button =
            Button::new(RichText::new(label).color(Color32::WHITE)).fill(if osc.is_enabled {
                palette::ACCENT_PURPLE
            } else {
                palette::BG_TERTIARY
            });
        if ui.add(button).clicked() {
            osc.is_enabled = !osc.is_enabled;
        }
        ui.label("Multiplier: ");
        let slider = ui.add(Slider::new(&mut osc.multiplier, 0.0..=5.0));
        if slider.double_clicked() {
            osc.multiplier = 1.0;
        }
        ui.label("Minimum: ");
        let slider = ui.add(Slider::new(&mut osc.min, 0.0..=1.0).fixed_decimals(2));
        if slider.double_clicked() {
            osc.min = 0.0;
        }
        ui.label("Maximum: ");
        let slider = ui.add(Slider::new(&mut osc.max, 0.0..=1.0).fixed_decimals(2));
        if slider.double_clicked() {
            osc.max = 1.0;
        }
    });
}
