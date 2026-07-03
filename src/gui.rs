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
    collections::{HashMap, HashSet, VecDeque},
    iter::from_fn,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
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
    auto_lock::{AutoLock, AutoLockState},
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

enum ConnectionState {
    Connecting,
    Connected,
    Error(String),
}

// ---------------------------------------------------------------------------
// Device structs (unchanged from original)
// ---------------------------------------------------------------------------

struct Device {
    name: String,
    props: Arc<Mutex<DeviceProps>>,
    task: tokio::task::JoinHandle<()>,
}

/// Values-only snapshot of a dropped device's tuning, restored on reconnect
/// within the grace window. Without this, a 2s RF blip rebuilt DeviceProps
/// from defaults — a multiplier the user lowered for comfort snapped back to
/// 1.0 on a running session.
struct SavedDeviceTuning {
    is_enabled: bool,
    multiplier: f32,
    min: f32,
    max: f32,
    vibrators: Vec<VibratorProps>,
    oscillators: Vec<OscillatorProps>,
}

impl Device {
    /// Abort the command and battery tasks. Must be called when the device
    /// leaves the map, otherwise a stale task keeps polling a dead handle
    /// and a reconnected device (same name) never gets a live task again.
    fn abort_tasks(&self) {
        self.task.abort();
        self.props
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .battery_state
            .task
            .abort();
    }
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

struct BatteryState {
    shared_level: SharedF32,
    task: tokio::task::JoinHandle<()>,
}

impl BatteryState {
    pub fn new(runtime: &Runtime, device: Arc<ButtplugClientDevice>) -> Self {
        let shared_level = SharedF32::new(0.0);
        let task = {
            let shared_level = shared_level.clone();
            runtime.spawn(battery_check_bg_task(device, shared_level))
        };
        Self { shared_level, task }
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
    let mut consecutive_errors: u32 = 0;
    loop {
        interval.tick().await;
        match device.battery_level().await {
            Ok(level) => {
                shared_level.store(level as f32);
                consecutive_errors = 0;
            }
            Err(_) => {
                consecutive_errors += 1;
                if consecutive_errors >= 5 {
                    shared_level.store(f32::NAN);
                    break;
                }
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
    client: Option<Arc<ButtplugClient>>,
    connection_state: ConnectionState,
    connection_task: Option<tokio::task::JoinHandle<Result<ButtplugClient, ButtplugClientError>>>,
    server_addr: Option<String>,
    server_name: String,
    capture_status: Arc<Mutex<String>>,
    // Keyed by Buttplug device INDEX (unique per connection); names collide
    // when two identical toys are connected.
    devices: HashMap<u32, Device>,
    // Tuning of recently dropped devices (by name -> (tuning, when)), so an
    // RF blip resumes the session with the user's comfort settings intact.
    // Pruned by RECONNECT_GRACE.
    recent_disconnects: HashMap<String, (SavedDeviceTuning, Instant)>,
    // Written every update() with app_now_ms(); device tasks stop output when
    // it goes stale (dead-man watchdog).
    pipeline_heartbeat: Arc<AtomicU64>,
    // Definitive result of the last scan start/stop op.
    scan_result: Arc<Mutex<Option<ScanOpResult>>>,
    // True while a scan start/stop op is in flight (button disabled).
    scan_op_in_flight: Arc<AtomicBool>,
    // Result of the last "Stop all devices" command; Some = stop FAILED.
    stop_all_error: Arc<Mutex<Option<String>>>,

    // Audio data from capture thread
    sound_power: SharedF32,            // Legacy: simple RMS power
    spectral_data: SharedSpectralData, // NEW: full spectral analysis

    // Processed output that devices read
    processed_output: SharedF32, // Final output after envelope/gate (primary motor)
    processed_output_2: SharedF32, // Secondary motor output from ClimaxEngine

    _capture_thread: JoinHandle<()>,
    use_advanced_shared: Arc<AtomicBool>,
    is_scanning: bool,
    show_settings: bool,

    // Processing state
    vibration_level: f32,
    motor2_level: f32,
    hold_start_time: Option<Instant>,

    // NEW: Signal processors (from ChloeVibes)
    gate: Gate,
    envelope: EnvelopeProcessor,
    beat_detector: BeatDetector,
    climax_engine: ClimaxEngine,

    // AUTO-LOCK supervisor (see docs/AUTO_LOCK_DESIGN.md)
    auto_lock: AutoLock,

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

    // Preset UI state
    selected_preset_category: PresetCategory,

    // Logo texture (optional — app works fine without it)
    logo_texture: Option<egui::TextureHandle>,

    settings: Settings,
}

// ---------------------------------------------------------------------------
// Color palette — dark theme with accent colors
// ---------------------------------------------------------------------------

#[allow(dead_code)] // design tokens -- not every tier is referenced yet
mod palette {
    use eframe::egui::Color32;

    // EFIS / pro-console palette: near-black "glass" surfaces where depth comes
    // from tone (not borders), and color is rationed -- one cyan for data/
    // "you set this", one green for "engaged & good", amber/red only at
    // caution/fault. The old ACCENT_* names are kept as aliases so every
    // existing call site inherits the new look with no churn.

    // --- Surfaces (faint navy-black, like a DiGiCo / SSL Live screen) ---
    pub const BG_PRIMARY: Color32 = Color32::from_rgb(11, 13, 17); // navy-black canvas
    pub const BG_SECONDARY: Color32 = Color32::from_rgb(24, 28, 34); // raised section panel
    pub const BG_TERTIARY: Color32 = Color32::from_rgb(40, 45, 53); // bezel grey (tactile bodies)
    pub const WELL: Color32 = Color32::from_rgb(8, 11, 16); // recessed navy inset behind meters/scopes
    pub const EDGE_HIGHLIGHT: Color32 = Color32::from_rgb(54, 62, 73); // top-edge bevel light

    // --- Accents (rationed; cool azure like a console selected channel) ---
    pub const CYAN: Color32 = Color32::from_rgb(56, 190, 235); // primary data accent / set-points
    pub const CYAN_DIM: Color32 = Color32::from_rgb(30, 108, 140);
    pub const GREEN: Color32 = Color32::from_rgb(54, 204, 110); // active / engaged / safe
    pub const AMBER: Color32 = Color32::from_rgb(255, 179, 30); // caution
    pub const RED: Color32 = Color32::from_rgb(255, 59, 48); // warning / clip / fault

    // --- Back-compat aliases (old primary purple -> cyan, etc.) ---
    pub const ACCENT_PURPLE: Color32 = CYAN;
    pub const ACCENT_PURPLE_DIM: Color32 = CYAN_DIM;
    pub const ACCENT_TEAL: Color32 = GREEN;
    pub const ACCENT_PINK: Color32 = RED;
    pub const ACCENT_AMBER: Color32 = AMBER;

    // --- Text tiers ---
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(232, 237, 242); // soft off-white
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(122, 130, 142); // captions / units
    pub const TEXT_DIM: Color32 = Color32::from_rgb(107, 116, 128); // disabled / ghost

    // --- Structure ---
    pub const HAIRLINE: Color32 = Color32::from_rgb(42, 48, 58); // 1px dividers / frames
    pub const GRID_LINE: Color32 = HAIRLINE;
}

// ---------------------------------------------------------------------------
// Visualization constants
// ---------------------------------------------------------------------------

const HISTORY_LEN: usize = 256;
const ADSR_PREVIEW_HEIGHT: f32 = 100.0;
const OUTPUT_HISTORY_HEIGHT: f32 = 80.0;

// ---------------------------------------------------------------------------
// Safety plumbing: pipeline heartbeat, dead-man watchdog, panic-stop
// ---------------------------------------------------------------------------

/// If the UI/pipeline thread stops producing fresh output for this long,
/// the per-device tasks command a stop rather than hold the last intensity
/// on a human body. Sized to sit well above normal frame hitches (window
/// drags, shader stalls) but far below "the app is actually hung".
const WATCHDOG_TIMEOUT_MS: u64 = 2_000;

/// How long a dropped device's enable state is remembered, so a brief BLE
/// blip resumes the session instead of coming back silently disabled.
const RECONNECT_GRACE: Duration = Duration::from_secs(60);

/// Milliseconds since app start on a monotonic clock. Shared time base for
/// the pipeline heartbeat and the device watchdogs.
fn app_now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

type PanicStopPair = (tokio::runtime::Handle, Arc<ButtplugClient>);

/// (was_start, error message if the scan op failed)
type ScanOpResult = (bool, Option<String>);

fn panic_stop_slot() -> &'static Mutex<Option<PanicStopPair>> {
    static SLOT: OnceLock<Mutex<Option<PanicStopPair>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Best-effort: command all devices to stop before the process dies from a
/// panic. Chained in FRONT of the existing crash-log hook; bounded so a dead
/// server cannot hang the crash path.
fn register_panic_stop(handle: tokio::runtime::Handle, client: Arc<ButtplugClient>) {
    static HOOK_INSTALLED: OnceLock<()> = OnceLock::new();
    *panic_stop_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some((handle, client));
    HOOK_INSTALLED.get_or_init(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // The capture thread catches and retries its own init panics
            // (device busy/unplugged); a recovered panic must not stop a
            // running session. Panics anywhere else are treated as fatal.
            let is_recovered_capture_panic = std::thread::current().name() == Some("capture");
            if !is_recovered_capture_panic {
                let _ = std::panic::catch_unwind(panic_stop_devices);
            }
            prev(info);
        }));
    });
}

fn panic_stop_devices() {
    let pair = panic_stop_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Some((handle, client)) = pair {
        let (tx, rx) = std::sync::mpsc::channel();
        handle.spawn(async move {
            let _ = client.stop_all_devices().await;
            let _ = tx.send(());
        });
        // Give the stop command a bounded window to reach the hardware.
        let _ = rx.recv_timeout(Duration::from_millis(1_500));
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
    use_advanced: Arc<AtomicBool>,
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

            // Legacy: low-pass filter + RMS (only when advanced processing is off)
            if !use_advanced.load(Ordering::Relaxed) {
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
            }

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

/// Install a condensed technical label font + a tabular mono readout font from
/// the Windows system fonts: Bahnschrift (DIN-style condensed, the cockpit/
/// console feel) for proportional text, Consolas for monospace readouts.
/// Falls back silently to egui defaults if a font file isn't present.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut changed = false;
    if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/bahnschrift.ttf") {
        fonts.font_data.insert(
            "bahnschrift".to_owned(),
            egui::FontData::from_owned(bytes).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "bahnschrift".to_owned());
        changed = true;
    }
    if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/consola.ttf") {
        fonts.font_data.insert(
            "consolas".to_owned(),
            egui::FontData::from_owned(bytes).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "consolas".to_owned());
        changed = true;
    }
    if changed {
        ctx.set_fonts(fonts);
    }
}

impl GuiApp {
    fn new(server_addr: Option<String>, ctx: &CreationContext) -> Self {
        install_fonts(&ctx.egui_ctx);
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
        let processed_output_2 = SharedF32::new(0.0);
        let capture_status = Arc::new(Mutex::new(String::from("audio: starting")));
        let capture_status2 = capture_status.clone();
        let pipeline_heartbeat = Arc::new(AtomicU64::new(app_now_ms()));

        // Debug diagnostic: log heartbeat age so pipeline stalls (minimize,
        // window drag, hangs) are measurable during development.
        #[cfg(debug_assertions)]
        {
            let hb = pipeline_heartbeat.clone();
            runtime.spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                loop {
                    interval.tick().await;
                    let age = app_now_ms().saturating_sub(hb.load(Ordering::Relaxed));
                    eprintln!("[hb-probe] pipeline heartbeat age: {age}ms");
                }
            });
        }

        let mut settings = ctx.storage.map(Settings::load).unwrap_or_default();
        settings.sanitize();
        // Recover from a stored preset name that no longer maps to a real preset:
        // legacy "Default" files, or a preset renamed/removed across versions.
        // An empty name means "custom" (user-edited) and must be left untouched.
        let stored_name = settings.current_preset_name.clone();
        let is_unknown_named =
            !stored_name.is_empty() && presets::find_preset(&stored_name).is_none();
        if stored_name.eq_ignore_ascii_case("Default") || is_unknown_named {
            if let Some(preset) = presets::find_preset("Ride Intensity") {
                settings.apply_preset(&preset);
            }
        }
        let low_pass_freq = settings.low_pass_freq.clone();
        let polling_rate_ms = settings.polling_rate_ms.clone();
        let use_polling_rate = settings.use_polling_rate.clone();
        let use_advanced_shared = Arc::new(AtomicBool::new(settings.use_advanced_processing));
        let use_advanced_capture = use_advanced_shared.clone();

        // Named so the panic-stop hook can recognize this thread's RECOVERED
        // panics (capture init is wrapped in catch_unwind + retry) and not
        // halt a running session for them.
        let _capture_thread = std::thread::Builder::new()
            .name("capture".to_string())
            .spawn(move || {
                capture_thread(
                    sound_power2,
                    spectral_data2,
                    low_pass_freq,
                    polling_rate_ms,
                    use_polling_rate,
                    use_advanced_capture,
                    capture_status2,
                )
            })
            .expect("failed to spawn audio capture thread");

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
            recent_disconnects: HashMap::new(),
            pipeline_heartbeat,
            scan_result: Arc::new(Mutex::new(None)),
            scan_op_in_flight: Arc::new(AtomicBool::new(false)),
            stop_all_error: Arc::new(Mutex::new(None)),
            sound_power,
            spectral_data,
            processed_output,
            processed_output_2,
            _capture_thread,
            use_advanced_shared,
            is_scanning: false,
            show_settings: false,
            settings,
            vibration_level: 0.0,
            motor2_level: 0.0,
            hold_start_time: None,

            // New processors
            gate: Gate::new(),
            envelope: EnvelopeProcessor::new(),
            beat_detector: BeatDetector::new(),
            climax_engine: ClimaxEngine::new(),
            auto_lock: AutoLock::new(),
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
            for device in self.devices.values() {
                let mut vibrators = Vec::new();
                let mut oscillators = Vec::new();
                let props = &device.props.lock().unwrap_or_else(|e| e.into_inner());
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
                    .insert(device.name.clone(), device_settings);
            }
        }
        // Persistence guard: never let auto-save persist an active Auto-Lock.
        // A crash or quit mid-lock must not silently replace the user's tuned
        // settings, so the pre-lock values are swapped in for the write.
        if let Some(snapshot) = self.auto_lock.pre_lock_snapshot() {
            let live = self.auto_lock.live_params(&self.settings);
            snapshot.apply(&mut self.settings);
            self.settings.save(storage);
            live.apply(&mut self.settings);
        } else {
            self.settings.save(storage);
        }
        storage.flush();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Sync advanced processing flag with capture thread
        self.use_advanced_shared
            .store(self.settings.use_advanced_processing, Ordering::Relaxed);

        // Apply the EFIS / pro-console theme: near-black glass, one cyan data
        // accent, green for engaged, hairline structure, low corner radius and
        // restrained hover so it reads as precise equipment, not a consumer app.
        let mut visuals = if self.settings.use_dark_mode {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        if self.settings.use_dark_mode {
            let cr = CornerRadius::same(3);
            let hairline = Stroke::new(1.0, palette::HAIRLINE);

            visuals.panel_fill = palette::BG_PRIMARY;
            visuals.window_fill = palette::BG_SECONDARY;
            visuals.extreme_bg_color = palette::WELL;
            visuals.faint_bg_color = palette::BG_SECONDARY;
            visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(56, 190, 235, 70);
            visuals.selection.stroke = Stroke::new(1.0, palette::CYAN);
            visuals.hyperlink_color = palette::CYAN;
            visuals.warn_fg_color = palette::AMBER;
            visuals.error_fg_color = palette::RED;
            visuals.window_corner_radius = CornerRadius::same(6);

            let w = &mut visuals.widgets;
            // Labels, separators, frames
            w.noninteractive.bg_fill = palette::BG_SECONDARY;
            w.noninteractive.bg_stroke = hairline;
            w.noninteractive.fg_stroke = Stroke::new(1.0, palette::TEXT_PRIMARY);
            w.noninteractive.corner_radius = cr;
            // Buttons / inputs at rest sit on bezel grey with a hairline edge
            w.inactive.bg_fill = palette::BG_TERTIARY;
            w.inactive.bg_stroke = hairline;
            w.inactive.fg_stroke = Stroke::new(1.0, palette::TEXT_PRIMARY);
            w.inactive.corner_radius = cr;
            // Hovered: restrained lift + cyan hairline, no bright flash
            w.hovered.bg_fill = palette::EDGE_HIGHLIGHT;
            w.hovered.bg_stroke = Stroke::new(1.0, palette::CYAN);
            w.hovered.fg_stroke = Stroke::new(1.0, palette::TEXT_PRIMARY);
            w.hovered.corner_radius = cr;
            w.hovered.expansion = 1.0;
            // Active / pressed: cyan body, dark text
            w.active.bg_fill = palette::CYAN;
            w.active.bg_stroke = Stroke::new(1.0, palette::CYAN);
            w.active.fg_stroke = Stroke::new(1.0, palette::BG_PRIMARY);
            w.active.corner_radius = cr;
            w.active.expansion = 1.0;
            // Open combo/menus
            w.open.bg_fill = palette::BG_TERTIARY;
            w.open.bg_stroke = Stroke::new(1.0, palette::CYAN);
            w.open.fg_stroke = Stroke::new(1.0, palette::TEXT_PRIMARY);
            w.open.corner_radius = cr;
        }
        ctx.set_visuals(visuals);
        ctx.style_mut(|s| {
            s.spacing.item_spacing = vec2(6.0, 4.0);
            s.spacing.button_padding = vec2(8.0, 4.0);
        });

        // --- Pipeline heartbeat (dead-man watchdog time base) ---
        self.pipeline_heartbeat
            .store(app_now_ms(), Ordering::Relaxed);

        // Engine clock, shared by the pipeline and the Auto-Lock UI.
        let current_time_ms = {
            static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
            let start = START.get_or_init(Instant::now);
            start.elapsed().as_secs_f32() * 1000.0
        };

        // --- Definitive scan start/stop results ---
        // is_scanning reflects what the server actually did, not what we
        // hoped: a failed START leaves it off, a failed STOP leaves it ON.
        if let Some((was_start, err)) = self
            .scan_result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            match err {
                None => self.is_scanning = was_start,
                Some(e) => {
                    self.is_scanning = !was_start;
                    let what = if was_start {
                        "Start scan failed"
                    } else {
                        "Stop scan failed"
                    };
                    self.connection_state = ConnectionState::Error(format!("{what}: {e}"));
                }
            }
        }

        // --- Server health check ---
        // A dead Intiface/websocket link used to leave the app claiming
        // "Connected" with dead output and no recovery UI short of restart.
        if matches!(self.connection_state, ConnectionState::Connected) {
            let alive = self.client.as_ref().map(|c| c.connected()).unwrap_or(false);
            if !alive {
                for (_, device) in self.devices.drain() {
                    device.abort_tasks();
                }
                self.client = None;
                self.is_scanning = false;
                self.connection_state =
                    ConnectionState::Error("Server connection lost".to_string());
            }
        }

        // --- Connection Handling ---
        if let Some(task) = self.connection_task.take() {
            if task.is_finished() {
                match self.runtime.block_on(task) {
                    Ok(Ok(client)) => {
                        let client = Arc::new(client);
                        self.server_name = client
                            .server_name()
                            .as_deref()
                            .unwrap_or("<unknown>")
                            .to_string();
                        register_panic_stop(self.runtime.handle().clone(), client.clone());
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
                        ConnectionState::Error(_) => {
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
                                // Never block the UI thread on a server
                                // round-trip: a slow/hung Intiface froze the
                                // whole app here. One op in flight at a time;
                                // is_scanning only changes when the server
                                // confirms (drained at frame start).
                                let scan_busy =
                                    self.scan_op_in_flight.load(Ordering::Relaxed);
                                if ui.add_enabled(!scan_busy, scan_btn).clicked() {
                                    let start = !self.is_scanning;
                                    self.scan_op_in_flight.store(true, Ordering::Relaxed);
                                    let client = client.clone();
                                    let slot = self.scan_result.clone();
                                    let in_flight = self.scan_op_in_flight.clone();
                                    self.runtime.spawn(async move {
                                        let res = if start {
                                            client.start_scanning().await
                                        } else {
                                            client.stop_scanning().await
                                        };
                                        *slot.lock().unwrap_or_else(|p| p.into_inner()) =
                                            Some((start, res.err().map(|e| e.to_string())));
                                        in_flight.store(false, Ordering::Relaxed);
                                    });
                                }
                            }
                        }
                        ConnectionState::Connecting => {}
                    }
                }

                match &self.connection_state {
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

                // AUTO-LOCK: fit the chain to the playing material
                // (docs/AUTO_LOCK_DESIGN.md). Advanced pipeline only.
                if self.settings.use_advanced_processing {
                    let al_fill = match self.auto_lock.state {
                        AutoLockState::Locked { .. } => palette::GREEN,
                        AutoLockState::Listening { .. } => palette::CYAN,
                        AutoLockState::NoLock { .. } => palette::AMBER,
                        AutoLockState::Idle => palette::BG_TERTIARY,
                    };
                    let al_btn = Button::new(
                        RichText::new(self.auto_lock.button_label(current_time_ms))
                            .color(Color32::WHITE),
                    )
                    .fill(al_fill);
                    if ui.add(al_btn).clicked() {
                        self.auto_lock.on_button(current_time_ms);
                    }
                    if self.auto_lock.is_locked() {
                        if ui.small_button("Revert").clicked() {
                            self.auto_lock.revert(&mut self.settings);
                        }
                        if ui.small_button("Keep").clicked() {
                            self.auto_lock.keep(&mut self.settings);
                        }
                    }
                }

                let stop_w = 120.0;
                ui.add_space(ui.available_width() - stop_w);

                let stop_btn = Button::new(
                    RichText::new("Stop all devices").color(Color32::BLACK),
                )
                .fill(Color32::from_rgb(240, 0, 0));
                if ui.add_sized([stop_w, 30.0], stop_btn).clicked() {
                    if let Some(client) = &self.client {
                        // Verified stop: the result is checked and a failure is
                        // shown in red. Disabling every device also makes the
                        // per-device tasks send zeros within one 20ms tick, so
                        // the hardware stops even if this RPC fails.
                        let client = client.clone();
                        let err_slot = self.stop_all_error.clone();
                        self.runtime.spawn(async move {
                            let res = client.stop_all_devices().await;
                            *err_slot.lock().unwrap_or_else(|p| p.into_inner()) = match res {
                                Ok(_) => None,
                                Err(e) => Some(format!(
                                    "STOP COMMAND FAILED: {e} — power the device off"
                                )),
                            };
                        });
                        for device in self.devices.values_mut() {
                            device.props.lock().unwrap_or_else(|e| e.into_inner()).is_enabled = false;
                        }
                    }
                }
            });

            let stop_error = self
                .stop_all_error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(msg) = stop_error {
                ui.colored_label(palette::RED, msg);
            }

            ui.separator();

            // ============================================================
            // AUDIO PROCESSING PIPELINE
            // ============================================================
            let delta_time = ctx.input(|x| x.stable_dt);
            let main_mul = self.settings.main_volume;

            if self.settings.use_advanced_processing {
                // ====== NEW PIPELINE (ChloeVibes-derived) ======

                // 1. Read spectral data from capture thread
                self.last_spectral = self.spectral_data.load();

                // 2. Extract energy based on frequency mode
                let spectral_energy = SpectralAnalyzer::extract_energy(
                    &self.last_spectral,
                    self.settings.frequency_mode,
                    self.settings.target_frequency,
                );
                let spectral_energy = sanitize_unit(spectral_energy);
                let legacy_energy = sanitize_unit(self.sound_power.load());
                let spectral_rms = sanitize_unit(self.last_spectral.rms_power);
                let spectral_total = sanitize_unit(
                    self.last_spectral.band_energies.iter().copied().sum::<f32>()
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
                // Raw energy (pre-volume) for the gate so the threshold
                // slider maps cleanly regardless of volume setting.
                let raw_energy_for_gate = sanitize_unit(normalized_input);
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
                let (detected_onset, onset_strength) =
                    self.beat_detector.process(self.last_spectral.spectral_flux, current_time_ms);

                // Predictive onset: when tempo is locked, pre-trigger ~2 device
                // write intervals early so the attack command arrives on-beat
                // instead of late. False positives feel like syncopation;
                // late delivery feels like lag.
                let is_onset = if !detected_onset
                    && self.beat_detector.tempo_confidence > 0.6
                {
                    let predicted = self.beat_detector.predicted_next_onset_ms;
                    if predicted > 0.0 {
                        let lead_time_ms = 76.0; // ~2 BLE/device write intervals
                        let time_to_predicted = predicted - current_time_ms;
                        time_to_predicted >= 0.0
                            && time_to_predicted <= lead_time_ms
                            && self.gate_is_open
                    } else {
                        false
                    }
                } else {
                    detected_onset
                };

                let onset_ok = is_onset
                    && onset_strength > 1.02
                    && energy > self.settings.gate_threshold * 0.40;

                // 5. Gate (uses raw pre-volume energy so threshold is
                // volume-independent, matching Android behavior)
                self.gate_is_open = self.gate.process(
                    raw_energy_for_gate,
                    self.settings.gate_threshold,
                    self.settings.auto_gate_amount,
                    self.settings.gate_smoothing,
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
                    self.last_spectral.spectral_centroid,
                );

                // 7. Optional climax modulation layer
                let shaped_output = self.climax_engine.process(
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
                self.climax_phase = if self.settings.climax_mode_enabled {
                    self.climax_engine.phase_progress(
                        current_time_ms,
                        self.settings.climax_build_up_ms,
                    )
                } else {
                    0.0
                };

                // 8. Apply output range (min_vibe / max_vibe) + output gain via
                // the shared, parity-tested output mapping. is_silent forces a
                // true zero (energy negligible, gate shut, envelope idle) so a
                // brief silence pumps back to rest; the climax engine is
                // intentionally NOT reset so minutes of build-up survive.
                let is_silent = energy < 0.005
                    && !self.gate_is_open
                    && self.envelope.state == audio::EnvelopeState::Idle;
                let final_intensity = audio::map_output(
                    shaped_output,
                    self.settings.min_vibe,
                    self.settings.max_vibe,
                    self.settings.output_gain,
                    is_silent,
                );

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
                let output_alpha = if trimmed_intensity >= self.vibration_level {
                    smoothing_alpha(delta_time, output_up_ms)
                } else {
                    smoothing_alpha(delta_time, output_down_ms)
                };
                self.vibration_level +=
                    (trimmed_intensity - self.vibration_level) * output_alpha;
                self.vibration_level = self.vibration_level.clamp(0.0, 1.0);

                // AUTO-LOCK supervisor: observes this frame, manages the
                // listen/commit/glide state machine. Writes only whitelisted
                // Settings fields (docs/AUTO_LOCK_DESIGN.md).
                self.auto_lock.tick(
                    current_time_ms,
                    &self.last_spectral,
                    raw_energy_for_gate,
                    onset_ok,
                    envelope_output,
                    using_rms_fallback,
                    self.beat_detector.tempo_confidence,
                    &mut self.settings,
                );
            } else {
                // ====== LEGACY PIPELINE (original Chloe Vibes) ======
                // Auto-Lock only supervises the advanced pipeline.
                if !matches!(self.auto_lock.state, AutoLockState::Idle) {
                    self.auto_lock.cancel();
                }
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

            // Secondary motor (motor 2) is driven by the HIGH-frequency content
            // of the live audio. Motor 1 carries the full body vibe; on a
            // dual-motor device this puts bass/body on motor 1 and treble/detail
            // on motor 2 -- tuned by the audio itself, not the climax engine.
            // NOTE: motor2 scales the already-mapped motor1 level directly (no
            // second map_output), so both motors live in the same output space.
            let is_silent = self.raw_energy < 0.005
                && !self.gate_is_open
                && self.envelope.state == audio::EnvelopeState::Idle;
            let motor2_target = if is_silent {
                0.0
            } else {
                // Treble fraction = high bands (Hi-Mid..Air) / total band energy.
                // Bands 0..4 = Sub/Bass/Lo-Mid/Mid, bands 4..8 = Hi-Mid/Pres/Brill/Air.
                let be = &self.last_spectral.band_energies;
                let low_e = be[0] + be[1] + be[2] + be[3];
                let high_e = be[4] + be[5] + be[6] + be[7];
                let total = low_e + high_e;
                let treble_frac = if total > 1e-9 { high_e / total } else { 0.0 };
                // Gain so even modest treble lifts motor 2; clamp keeps it sane.
                let treble_weight = (treble_frac * 3.0).clamp(0.0, 1.0);
                (self.vibration_level * treble_weight).clamp(0.0, 1.0)
            };
            // Apply same slew smoothing as motor1 so both motors have matching latency
            let m2_up_ms = (self.settings.output_slew_ms * 0.35).max(1.0);
            let m2_down_ms = self.settings.output_slew_ms.max(1.0);
            let motor2_alpha = if motor2_target >= self.motor2_level {
                smoothing_alpha(delta_time, m2_up_ms)
            } else {
                smoothing_alpha(delta_time, m2_down_ms)
            };
            self.motor2_level += (motor2_target - self.motor2_level) * motor2_alpha;
            self.motor2_level = self.motor2_level.clamp(0.0, 1.0);
            self.processed_output_2.store(self.motor2_level);

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

                // Recessed well behind the meter
                painter.rect_filled(rect, 3.0, palette::WELL);

                // Filled portion -- calibrated meter zones (safe/caution/clip)
                let fill_width = rect.width() * self.vibration_level;
                if fill_width > 0.5 {
                    let fill_rect = Rect::from_min_size(
                        rect.min,
                        vec2(fill_width, rect.height()),
                    );
                    let color = if self.vibration_level > 0.85 {
                        palette::RED
                    } else if self.vibration_level > 0.6 {
                        palette::AMBER
                    } else {
                        palette::GREEN
                    };
                    painter.rect_filled(fill_rect, 3.0, color);
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
                "RMS loudness -> volume scaling -> optional hold/decay persistence -> clamp 0..1 -> device output."
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

                    let slider = ui.add(
                        Slider::new(&mut self.settings.output_gain, 0.0..=2.0)
                            .fixed_decimals(2)
                            .text("Output Gain"),
                    );
                    let slider = slider.on_hover_text(
                        "Final-stage multiplier applied after range mapping.\n\
                         1.0 = unity. Turn up for more headroom, down for safety.",
                    );
                    if slider.changed() {
                        mark_custom(&mut self.settings);
                    }
                    if slider.double_clicked() {
                        self.settings.output_gain = defaults::OUTPUT_GAIN;
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
                        "Linear scaling: 200% = 2x.",
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
                let bp_devices = client.devices();

                // Prune devices that dropped. Without this, entries lived
                // forever: on reconnect no new command task was spawned, and
                // the old task kept polling a dead handle — the device
                // reappeared in the list but never vibrated again. Keyed by
                // Buttplug device INDEX: names collide when two identical
                // toys are connected.
                let live: HashSet<u32> = bp_devices.iter().map(|d| d.index()).collect();
                let gone: Vec<u32> = self
                    .devices
                    .keys()
                    .filter(|idx| !live.contains(idx))
                    .copied()
                    .collect();
                for idx in gone {
                    if let Some(device) = self.devices.remove(&idx) {
                        let saved = {
                            let props =
                                device.props.lock().unwrap_or_else(|e| e.into_inner());
                            SavedDeviceTuning {
                                is_enabled: props.is_enabled,
                                multiplier: props.multiplier,
                                min: props.min,
                                max: props.max,
                                vibrators: props.vibrators.clone(),
                                oscillators: props.oscillators.clone(),
                            }
                        };
                        device.abort_tasks();
                        self.recent_disconnects
                            .insert(device.name.clone(), (saved, Instant::now()));
                    }
                }
                self.recent_disconnects
                    .retain(|_, (_, at)| at.elapsed() < RECONNECT_GRACE);

                for bp_device in bp_devices {
                    let device_index = bp_device.index();
                    let device_name = bp_device.name().to_string();
                    if !self.devices.contains_key(&device_index) {
                        let props = Arc::new(Mutex::new(DeviceProps::new(
                            &self.runtime,
                            bp_device.clone(),
                            &self.settings,
                        )));

                        // Resume the session after a brief RF blip with the
                        // user's tuning intact — enable state AND comfort
                        // settings (multiplier/min/max/per-motor), so output
                        // never jumps past what they had dialed in.
                        if let Some((saved, _)) =
                            self.recent_disconnects.remove(&device_name)
                        {
                            let mut p = props.lock().unwrap_or_else(|e| e.into_inner());
                            p.is_enabled = saved.is_enabled;
                            p.multiplier = saved.multiplier;
                            p.min = saved.min;
                            p.max = saved.max;
                            if saved.vibrators.len() == p.vibrators.len() {
                                p.vibrators = saved.vibrators;
                            }
                            if saved.oscillators.len() == p.oscillators.len() {
                                p.oscillators = saved.oscillators;
                            }
                        }

                        // Spawn per-device task.
                        // Reads processed_output (after envelope/gate).
                        // RATE LIMITED: Only sends BT commands when intensity
                        // actually changes, at max 50Hz. Prevents Intiface
                        // crash/lag from BT packet flooding.
                        let task = self.runtime.spawn({
                            let bp_device = bp_device.clone();
                            let props = props.clone();
                            let processed = self.processed_output.clone();
                            let processed2 = self.processed_output_2.clone();
                            let heartbeat = self.pipeline_heartbeat.clone();
                            async move {
                                // Rate limiter state (from Gemini/ChloeVibes approach)
                                let mut last_sent: f64 = -1.0;
                                let mut last_sent_m2: f64 = -1.0;
                                // 0.5% dead-band: the Domi 2 has decent granularity.
                                // Finer resolution captures subtle dynamics that
                                // make the difference between "buzzing" and "alive".
                                let resolution: f64 = 0.005;
                                let mut consecutive_errors: u32 = 0;
                                // Enable-state signature: a toggle must force
                                // a send even when the computed level is
                                // steady, otherwise disabling a device (or
                                // the stop-all fallback flipping is_enabled)
                                // sends nothing and the motor keeps running.
                                let mut last_enable_sig: u64 = u64::MAX;

                                loop {
                                    // Dead-man watchdog: if the pipeline stops
                                    // producing fresh output (UI hang, panic
                                    // mid-frame), do not hold the last intensity
                                    // on a human body — stop and idle until the
                                    // heartbeat resumes.
                                    let heartbeat_age = app_now_ms()
                                        .saturating_sub(heartbeat.load(Ordering::Relaxed));
                                    if heartbeat_age > WATCHDOG_TIMEOUT_MS {
                                        if last_sent > 0.0 || last_sent_m2 > 0.0 {
                                            eprintln!(
                                                "Pipeline heartbeat stale ({heartbeat_age}ms); stopping device"
                                            );
                                            // Only mark stopped on SUCCESS —
                                            // a failed stop must be retried
                                            // on the next 250ms pass, not
                                            // forgotten while the motor runs.
                                            match bp_device.stop().await {
                                                Ok(_) => {
                                                    last_sent = 0.0;
                                                    last_sent_m2 = 0.0;
                                                }
                                                Err(e) => eprintln!(
                                                    "Watchdog stop failed (will retry): {e}"
                                                ),
                                            }
                                        }
                                        tokio::time::sleep(Duration::from_millis(250)).await;
                                        continue;
                                    }

                                    let now = tokio::time::Instant::now();
                                    let vibration_level = processed.load();
                                    let vibration_level_2 = processed2.load();
                                    let mut should_send = false;
                                    let mut vibrate_cmd = None;
                                    let mut oscillate_cmd = None;
                                    {
                                        let guard = props.lock().unwrap_or_else(|e| e.into_inner());
                                        // A disabled device drives zero, so a
                                        // toggle at a steady level registers
                                        // as a change (and a hard stop).
                                        let speed = if guard.is_enabled {
                                            guard.calculate_output(vibration_level)
                                        } else {
                                            0.0
                                        };
                                        let speed_f64 = speed as f64;

                                        // === RATE LIMITER ===
                                        // Only send if:
                                        //   a) intensity changed by more than 0.5%, OR
                                        //   b) this is a hard stop (going to zero), OR
                                        //   c) any enable toggle changed
                                        let speed2 = if guard.is_enabled {
                                            guard.calculate_output(vibration_level_2)
                                        } else {
                                            0.0
                                        };
                                        let speed2_f64 = speed2 as f64;
                                        let change = (speed_f64 - last_sent).abs();
                                        let change_m2 = (speed2_f64 - last_sent_m2).abs();
                                        let is_hard_stop = speed_f64 < 0.005
                                            && last_sent >= 0.005;
                                        let mut enable_sig: u64 = guard.is_enabled as u64;
                                        for v in &guard.vibrators {
                                            enable_sig = (enable_sig << 1) | v.is_enabled as u64;
                                        }
                                        for o in &guard.oscillators {
                                            enable_sig = (enable_sig << 1) | o.is_enabled as u64;
                                        }

                                        if change >= resolution
                                            || change_m2 >= resolution
                                            || is_hard_stop
                                            || enable_sig != last_enable_sig
                                        {
                                            should_send = true;
                                            last_sent = speed_f64;
                                            last_sent_m2 = speed2_f64;
                                            last_enable_sig = enable_sig;
                                        }

                                        if should_send {
                                            if !guard.vibrators.is_empty() {
                                                vibrate_cmd = Some(
                                                    ScalarValueCommand::ScalarValueVec(
                                                        guard
                                                            .vibrators
                                                            .iter()
                                                            .enumerate()
                                                            .map(|(idx, v)| {
                                                                if v.is_enabled
                                                                    && guard
                                                                        .is_enabled
                                                                {
                                                                    // Motor 0 = primary signal.
                                                                    // Motors 1+ = secondary motor
                                                                    // signal from ClimaxEngine
                                                                    // dual-motor phasing.
                                                                    let src = if idx == 0 {
                                                                        speed
                                                                    } else {
                                                                        guard.calculate_output(
                                                                            vibration_level_2,
                                                                        )
                                                                    };
                                                                    (src
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
                                        let mut had_error = false;
                                        if let Some(cmd) = vibrate_cmd {
                                            if let Err(e) =
                                                bp_device.vibrate(&cmd).await
                                            {
                                                eprintln!(
                                                    "Vibrate error: {e}"
                                                );
                                                had_error = true;
                                            }
                                        }
                                        if let Some(cmd) = oscillate_cmd {
                                            if let Err(e) =
                                                bp_device.oscillate(&cmd).await
                                            {
                                                eprintln!(
                                                    "Oscillate error: {e}"
                                                );
                                                had_error = true;
                                            }
                                        }
                                        if had_error {
                                            consecutive_errors += 1;
                                            if consecutive_errors >= 10 {
                                                // Do NOT exit: an exited task
                                                // means a device that looks
                                                // connected but never vibrates.
                                                // Back off; if the device is
                                                // really gone the client drops
                                                // it and the UI thread aborts
                                                // this task.
                                                eprintln!("10+ consecutive BLE errors; backing off");
                                                tokio::time::sleep(Duration::from_secs(1)).await;
                                            }
                                        } else {
                                            consecutive_errors = 0;
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
                            name: device_name.clone(),
                            props,
                            task,
                        };
                        self.devices.insert(device_index, device);
                    }
                    let device =
                        self.devices.get_mut(&device_index).unwrap();
                    device_widget(
                        ui,
                        bp_device,
                        &mut device.props.lock().unwrap_or_else(|e| e.into_inner()),
                        self.vibration_level,
                        &self.runtime,
                    );
                }
            }
        });

        settings_window_widget(ctx, &mut self.show_settings, &mut self.settings);
        ctx.request_repaint_after(Duration::from_millis(16));
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
    // Recessed scope well + hairline frame
    painter.rect_filled(rect, 4.0, palette::WELL);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, palette::HAIRLINE),
        StrokeKind::Inside,
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

    // Faint fill under the curve -- properly translucent (not additive), so a
    // high sustain no longer floods the panel with a bright block.
    for i in 0..points.len().saturating_sub(1) {
        let p0 = points[i];
        let p1 = points[i + 1];
        let quad = vec![pos2(p0.x, origin.y), p0, p1, pos2(p1.x, origin.y)];
        painter.add(Shape::convex_polygon(
            quad,
            Color32::from_rgba_unmultiplied(56, 190, 235, 16),
            Stroke::NONE,
        ));
    }

    // Single restrained azure curve, no glow.
    if points.len() >= 2 {
        painter.add(Shape::line(points, Stroke::new(1.5, palette::CYAN)));
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
    painter.rect_filled(rect, 4.0, palette::WELL);
    let border_color = if gate_is_open {
        palette::CYAN
    } else {
        palette::HAIRLINE
    };
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, border_color),
        StrokeKind::Inside,
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
        palette::CYAN
    } else {
        Color32::from_rgba_unmultiplied(56, 190, 235, 110)
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

        painter.add(Shape::convex_polygon(
            vec![
                pos2(x0, bottom),
                pos2(x0, y0),
                pos2(x1, y1),
                pos2(x1, bottom),
            ],
            Color32::from_rgba_unmultiplied(90, 100, 115, 26),
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
        painter.add(Shape::line(line_points, Stroke::new(1.5, palette::GREEN)));
    }
}

/// Draw prettier spectrum frequency bars.
fn draw_spectrum_bars(
    painter: &egui::Painter,
    rect: Rect,
    band_energies: &[f32],
    gate_is_open: bool,
) {
    painter.rect_filled(rect, 4.0, palette::WELL);
    let border_color = if gate_is_open {
        palette::CYAN
    } else {
        palette::HAIRLINE
    };
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, border_color),
        StrokeKind::Inside,
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

#[derive(Clone)]
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

#[derive(Clone)]
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
                    // Logarithmic: linearly, the whole useful zone (0-2x) sat
                    // in the first 10% of travel of the 0-20x range.
                    let slider =
                        ui.add(Slider::new(&mut props.multiplier, 0.0..=20.0).logarithmic(true));
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
        let slider = ui.add(Slider::new(&mut vibe.multiplier, 0.0..=5.0).logarithmic(true));
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
        let slider = ui.add(Slider::new(&mut osc.multiplier, 0.0..=5.0).logarithmic(true));
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
