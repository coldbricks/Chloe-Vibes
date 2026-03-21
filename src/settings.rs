// ==========================================================================
// settings.rs — Persistent Settings
// Extended with spectral analysis, ADSR envelope, gate, and trigger settings
// ported from ChloeVibes.
// ==========================================================================

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use eframe::{get_value, set_value, Storage};

use crate::audio::{ClimaxPattern, FrequencyMode, TriggerMode};
use crate::util::SharedF32;

// ---------------------------------------------------------------------------
// Per-device settings (unchanged from original)
// ---------------------------------------------------------------------------

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct VibratorSettings {
    pub is_enabled: bool,
    pub multiplier: f32,
    pub min: f32,
    pub max: f32,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct OscillatorSettings {
    pub is_enabled: bool,
    pub multiplier: f32,
    pub min: f32,
    pub max: f32,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DeviceSettings {
    pub is_enabled: bool,
    pub multiplier: f32,
    pub min: f32,
    pub max: f32,
    pub vibrators: Vec<VibratorSettings>,
    #[serde(default)]
    pub oscillators: Vec<OscillatorSettings>,
}

// ---------------------------------------------------------------------------
// Main settings struct
// ---------------------------------------------------------------------------

pub struct Settings {
    // === Original settings ===
    pub main_volume: f32,
    pub low_pass_freq: SharedF32,
    pub use_dark_mode: bool,
    pub start_scanning_on_startup: bool,
    pub polling_rate_ms: SharedF32,
    pub use_polling_rate: Arc<AtomicBool>,
    pub device_settings: HashMap<String, DeviceSettings>,
    pub save_device_settings: bool,

    // === NEW: Signal Processing (from ChloeVibes) ===

    // -- Trigger Mode --
    /// How audio energy is converted to trigger magnitude
    pub trigger_mode: TriggerMode,
    /// Output level for Binary mode (0.0 - 1.0)
    pub binary_level: f32,
    /// Blend between dynamic and binary in Hybrid mode (0.0 = dynamic, 1.0 = binary)
    pub hybrid_blend: f32,
    /// Softness around threshold (higher = less all-or-nothing).
    pub threshold_knee: f32,
    /// Dynamic transfer curve (1.0 = linear, >1 expands range, <1 compresses).
    pub dynamic_curve: f32,
    /// Input smoothing rise time (ms). Lower = snappier.
    pub input_rise_ms: f32,
    /// Input smoothing fall time (ms). Lower = less trailing.
    pub input_fall_ms: f32,
    /// Final output slew time (ms). Lower = tighter rhythm.
    pub output_slew_ms: f32,
    /// Timing trim in milliseconds (positive = delay haptics, negative = advance).
    pub trim_ms: f32,

    // -- Frequency Mode --
    /// Which part of the spectrum to analyze
    pub frequency_mode: FrequencyMode,
    /// Target frequency for bandpass/lowpass/highpass modes (Hz)
    pub target_frequency: f32,

    // -- ADSR Envelope --
    /// Attack time in milliseconds (how fast vibration ramps up)
    pub attack_ms: f32,
    /// Decay time in milliseconds (how fast it drops from peak to sustain)
    pub decay_ms: f32,
    /// Sustain level (0.0 - 1.0, what fraction of peak to hold)
    pub sustain_level: f32,
    /// Release time in milliseconds (how fast it fades after sound stops)
    pub release_ms: f32,
    /// Attack curve shape (1.0 = linear, <1 = log, >1 = exp)
    pub attack_curve: f32,
    /// Decay curve shape
    pub decay_curve: f32,
    /// Release curve shape
    pub release_curve: f32,

    // -- Gate --
    /// Energy threshold for gate to open (0.0 - 1.0)
    pub gate_threshold: f32,
    /// How much the auto-gate contributes (0.0 = manual only, 1.0 = fully auto)
    pub auto_gate_amount: f32,
    /// Gate signal smoothing (0.0 = instant, 1.0 = very gradual)
    pub gate_smoothing: f32,

    // -- Output Range --
    /// Minimum vibration output (floor). Device won't go below this when active.
    pub min_vibe: f32,
    /// Maximum vibration output (ceiling).
    pub max_vibe: f32,

    // -- Climax Engine --
    /// Enables time-based build/tease/surge modulation layer.
    pub climax_mode_enabled: bool,
    /// Overall strength of the climax modulation.
    pub climax_intensity: f32,
    /// Duration of one full build cycle in milliseconds.
    pub climax_build_up_ms: f32,
    /// Fraction of cycle used for tease behavior near the end.
    pub climax_tease_ratio: f32,
    /// Depth of the tease dip.
    pub climax_tease_drop: f32,
    /// End-of-cycle surge boost amount.
    pub climax_surge_boost: f32,
    /// Depth of fast micro-pulse modulation.
    pub climax_pulse_depth: f32,
    /// Shape of long-cycle escalation.
    pub climax_pattern: ClimaxPattern,

    // -- Legacy persistence (kept for backward compat, but ADSR replaces it) --
    pub enable_persistence: bool,
    pub hold_delay_ms: f32,
    pub decay_rate_per_sec: f32,

    // -- Use new processing pipeline --
    /// When true, uses the new spectral analysis + ADSR pipeline.
    /// When false, falls back to the original simple processing.
    pub use_advanced_processing: bool,

    // -- Current preset tracking --
    /// Name of the last applied preset (empty string = custom)
    pub current_preset_name: String,
}

impl Settings {
    /// Apply a preset's values to all signal-processing parameters.
    /// Does NOT touch connection/device/UI settings.
    pub fn apply_preset(&mut self, preset: &crate::presets::Preset) {
        self.main_volume = preset.main_volume;
        self.frequency_mode = preset.frequency_mode;
        self.target_frequency = preset.target_frequency;
        self.gate_threshold = preset.gate_threshold;
        self.auto_gate_amount = preset.auto_gate_amount;
        self.gate_smoothing = preset.gate_smoothing;
        self.trigger_mode = preset.trigger_mode;
        self.binary_level = preset.binary_level;
        self.hybrid_blend = preset.hybrid_blend;
        self.threshold_knee = defaults::THRESHOLD_KNEE;
        self.dynamic_curve = defaults::DYNAMIC_CURVE;
        self.input_rise_ms = defaults::INPUT_RISE_MS;
        self.input_fall_ms = defaults::INPUT_FALL_MS;
        self.output_slew_ms = defaults::OUTPUT_SLEW_MS;
        self.trim_ms = defaults::TRIM_MS;
        self.attack_ms = preset.attack_ms;
        self.decay_ms = preset.decay_ms;
        self.sustain_level = preset.sustain_level;
        self.release_ms = preset.release_ms;
        self.attack_curve = preset.attack_curve;
        self.decay_curve = preset.decay_curve;
        self.release_curve = preset.release_curve;
        self.min_vibe = preset.min_vibe;
        self.max_vibe = preset.max_vibe;
        self.climax_mode_enabled = preset.climax_enabled;
        self.climax_intensity = preset.climax_intensity;
        self.climax_build_up_ms = preset.climax_build_up_ms;
        self.climax_tease_ratio = preset.climax_tease_ratio;
        self.climax_tease_drop = preset.climax_tease_drop;
        self.climax_surge_boost = preset.climax_surge_boost;
        self.climax_pulse_depth = preset.climax_pulse_depth;
        self.climax_pattern = preset.climax_pattern;
        self.current_preset_name = preset.name.to_string();
    }
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

pub mod defaults {
    use crate::audio::ClimaxPattern;

    // Original
    pub const MAIN_VOLUME: f32 = 1.15;
    pub const LOW_PASS_FREQ: f32 = 20_000.0;
    pub const DARK_MODE: bool = true;
    pub const START_SCANNING_ON_STARTUP: bool = false;
    pub const POLLING_RATE_MS: f32 = 20.0;
    pub const USE_POLLING_RATE: bool = false;

    // Trigger
    pub const BINARY_LEVEL: f32 = 0.8;
    pub const HYBRID_BLEND: f32 = 0.5;
    pub const THRESHOLD_KNEE: f32 = 0.22;
    pub const DYNAMIC_CURVE: f32 = 1.0;
    pub const INPUT_RISE_MS: f32 = 36.0;
    pub const INPUT_FALL_MS: f32 = 150.0;
    pub const OUTPUT_SLEW_MS: f32 = 85.0;
    pub const TRIM_MS: f32 = 0.0;

    // Frequency
    pub const TARGET_FREQUENCY: f32 = 200.0;

    // ADSR - neutral defaults for smooth intensity following
    pub const ATTACK_MS: f32 = 30.0;
    pub const DECAY_MS: f32 = 160.0;
    pub const SUSTAIN_LEVEL: f32 = 0.9;
    pub const RELEASE_MS: f32 = 320.0;
    pub const ATTACK_CURVE: f32 = 1.0; // Linear attack
    pub const DECAY_CURVE: f32 = 1.0; // Linear decay
    pub const RELEASE_CURVE: f32 = 1.15; // Slightly eased release tail

    // Gate
    pub const GATE_THRESHOLD: f32 = 0.07;
    pub const AUTO_GATE_AMOUNT: f32 = 0.0;
    pub const GATE_SMOOTHING: f32 = 0.22;

    // Output range
    pub const MIN_VIBE: f32 = 0.0;
    pub const MAX_VIBE: f32 = 1.0;

    // Climax engine
    pub const CLIMAX_MODE_ENABLED: bool = false;
    pub const CLIMAX_INTENSITY: f32 = 0.7;
    pub const CLIMAX_BUILD_UP_MS: f32 = 90_000.0;
    pub const CLIMAX_TEASE_RATIO: f32 = 0.18;
    pub const CLIMAX_TEASE_DROP: f32 = 0.35;
    pub const CLIMAX_SURGE_BOOST: f32 = 0.5;
    pub const CLIMAX_PULSE_DEPTH: f32 = 0.18;
    pub const CLIMAX_PATTERN: ClimaxPattern = ClimaxPattern::Wave;

    // Legacy persistence
    pub const ENABLE_PERSISTENCE: bool = false;
    pub const HOLD_DELAY_MS: f32 = 0.0;
    pub const DECAY_RATE_PER_SEC: f32 = 2.0;

    // Pipeline toggle
    pub const USE_ADVANCED_PROCESSING: bool = true;
}

// ---------------------------------------------------------------------------
// Persistence key names
// ---------------------------------------------------------------------------

mod names {
    // Original
    pub const MAIN_VOLUME: &str = "main_volume";
    pub const LOW_PASS_FREQ: &str = "low_pass_freq";
    pub const DARK_MODE: &str = "dark_mode";
    pub const START_SCANNING_ON_STARTUP: &str = "start_scanning_on_startup";
    pub const POLLING_RATE_MS: &str = "polling_rate_ms";
    pub const USE_POLLING_RATE: &str = "use_polling_rate";
    pub const DEVICE_SETTINGS: &str = "device_settings";
    pub const SAVE_DEVICE_SETTINGS: &str = "save_device_settings";

    // New
    pub const TRIGGER_MODE: &str = "trigger_mode";
    pub const BINARY_LEVEL: &str = "binary_level";
    pub const HYBRID_BLEND: &str = "hybrid_blend";
    pub const THRESHOLD_KNEE: &str = "threshold_knee";
    pub const DYNAMIC_CURVE: &str = "dynamic_curve";
    pub const INPUT_RISE_MS: &str = "input_rise_ms";
    pub const INPUT_FALL_MS: &str = "input_fall_ms";
    pub const OUTPUT_SLEW_MS: &str = "output_slew_ms";
    pub const TRIM_MS: &str = "trim_ms";
    pub const FREQUENCY_MODE: &str = "frequency_mode";
    pub const TARGET_FREQUENCY: &str = "target_frequency";
    pub const ATTACK_MS: &str = "attack_ms";
    pub const DECAY_MS: &str = "decay_ms";
    pub const SUSTAIN_LEVEL: &str = "sustain_level";
    pub const RELEASE_MS: &str = "release_ms";
    pub const ATTACK_CURVE: &str = "attack_curve";
    pub const DECAY_CURVE: &str = "decay_curve";
    pub const RELEASE_CURVE: &str = "release_curve";
    pub const GATE_THRESHOLD: &str = "gate_threshold";
    pub const AUTO_GATE_AMOUNT: &str = "auto_gate_amount";
    pub const GATE_SMOOTHING: &str = "gate_smoothing";
    pub const MIN_VIBE: &str = "min_vibe";
    pub const MAX_VIBE: &str = "max_vibe";
    pub const CLIMAX_MODE_ENABLED: &str = "climax_mode_enabled";
    pub const CLIMAX_INTENSITY: &str = "climax_intensity";
    pub const CLIMAX_BUILD_UP_MS: &str = "climax_build_up_ms";
    pub const CLIMAX_TEASE_RATIO: &str = "climax_tease_ratio";
    pub const CLIMAX_TEASE_DROP: &str = "climax_tease_drop";
    pub const CLIMAX_SURGE_BOOST: &str = "climax_surge_boost";
    pub const CLIMAX_PULSE_DEPTH: &str = "climax_pulse_depth";
    pub const CLIMAX_PATTERN: &str = "climax_pattern";
    pub const ENABLE_PERSISTENCE: &str = "enable_persistence";
    pub const HOLD_DELAY_MS: &str = "hold_delay_ms";
    pub const DECAY_RATE_PER_SEC: &str = "decay_rate_per_sec";
    pub const USE_ADVANCED_PROCESSING: &str = "use_advanced_processing";
    pub const CURRENT_PRESET_NAME: &str = "current_preset_name";
}

// ---------------------------------------------------------------------------
// Default impl
// ---------------------------------------------------------------------------

impl Default for Settings {
    fn default() -> Self {
        Self {
            main_volume: defaults::MAIN_VOLUME,
            low_pass_freq: SharedF32::new(defaults::LOW_PASS_FREQ),
            use_dark_mode: defaults::DARK_MODE,
            start_scanning_on_startup: defaults::START_SCANNING_ON_STARTUP,
            polling_rate_ms: SharedF32::new(defaults::POLLING_RATE_MS),
            use_polling_rate: Arc::new(AtomicBool::new(defaults::USE_POLLING_RATE)),
            device_settings: HashMap::new(),
            save_device_settings: false,

            trigger_mode: TriggerMode::Dynamic,
            binary_level: defaults::BINARY_LEVEL,
            hybrid_blend: defaults::HYBRID_BLEND,
            threshold_knee: defaults::THRESHOLD_KNEE,
            dynamic_curve: defaults::DYNAMIC_CURVE,
            input_rise_ms: defaults::INPUT_RISE_MS,
            input_fall_ms: defaults::INPUT_FALL_MS,
            output_slew_ms: defaults::OUTPUT_SLEW_MS,
            trim_ms: defaults::TRIM_MS,

            frequency_mode: FrequencyMode::Full,
            target_frequency: defaults::TARGET_FREQUENCY,

            attack_ms: defaults::ATTACK_MS,
            decay_ms: defaults::DECAY_MS,
            sustain_level: defaults::SUSTAIN_LEVEL,
            release_ms: defaults::RELEASE_MS,
            attack_curve: defaults::ATTACK_CURVE,
            decay_curve: defaults::DECAY_CURVE,
            release_curve: defaults::RELEASE_CURVE,

            gate_threshold: defaults::GATE_THRESHOLD,
            auto_gate_amount: defaults::AUTO_GATE_AMOUNT,
            gate_smoothing: defaults::GATE_SMOOTHING,

            min_vibe: defaults::MIN_VIBE,
            max_vibe: defaults::MAX_VIBE,

            climax_mode_enabled: defaults::CLIMAX_MODE_ENABLED,
            climax_intensity: defaults::CLIMAX_INTENSITY,
            climax_build_up_ms: defaults::CLIMAX_BUILD_UP_MS,
            climax_tease_ratio: defaults::CLIMAX_TEASE_RATIO,
            climax_tease_drop: defaults::CLIMAX_TEASE_DROP,
            climax_surge_boost: defaults::CLIMAX_SURGE_BOOST,
            climax_pulse_depth: defaults::CLIMAX_PULSE_DEPTH,
            climax_pattern: defaults::CLIMAX_PATTERN,

            enable_persistence: defaults::ENABLE_PERSISTENCE,
            hold_delay_ms: defaults::HOLD_DELAY_MS,
            decay_rate_per_sec: defaults::DECAY_RATE_PER_SEC,

            use_advanced_processing: defaults::USE_ADVANCED_PROCESSING,
            current_preset_name: String::from("Ride Intensity"),
        }
    }
}

// ---------------------------------------------------------------------------
// Load / Save
// ---------------------------------------------------------------------------

impl Settings {
    pub fn load(storage: &dyn Storage) -> Self {
        let main_volume = get_value(storage, names::MAIN_VOLUME).unwrap_or(defaults::MAIN_VOLUME);
        let low_pass_freq =
            get_value(storage, names::LOW_PASS_FREQ).unwrap_or(defaults::LOW_PASS_FREQ);
        let use_dark_mode = get_value(storage, names::DARK_MODE).unwrap_or(defaults::DARK_MODE);
        let start_scanning_on_startup = get_value(storage, names::START_SCANNING_ON_STARTUP)
            .unwrap_or(defaults::START_SCANNING_ON_STARTUP);
        let polling_rate_ms =
            get_value(storage, names::POLLING_RATE_MS).unwrap_or(defaults::POLLING_RATE_MS);
        let use_polling_rate =
            get_value(storage, names::USE_POLLING_RATE).unwrap_or(defaults::USE_POLLING_RATE);
        let device_settings: HashMap<String, DeviceSettings> =
            get_value(storage, names::DEVICE_SETTINGS).unwrap_or_default();
        let save_device_settings = get_value(storage, names::SAVE_DEVICE_SETTINGS).unwrap_or(false);

        // New settings
        let trigger_mode = get_value(storage, names::TRIGGER_MODE).unwrap_or(TriggerMode::Dynamic);
        let binary_level =
            get_value(storage, names::BINARY_LEVEL).unwrap_or(defaults::BINARY_LEVEL);
        let hybrid_blend =
            get_value(storage, names::HYBRID_BLEND).unwrap_or(defaults::HYBRID_BLEND);
        let threshold_knee =
            get_value(storage, names::THRESHOLD_KNEE).unwrap_or(defaults::THRESHOLD_KNEE);
        let dynamic_curve =
            get_value(storage, names::DYNAMIC_CURVE).unwrap_or(defaults::DYNAMIC_CURVE);
        let input_rise_ms =
            get_value(storage, names::INPUT_RISE_MS).unwrap_or(defaults::INPUT_RISE_MS);
        let input_fall_ms =
            get_value(storage, names::INPUT_FALL_MS).unwrap_or(defaults::INPUT_FALL_MS);
        let output_slew_ms =
            get_value(storage, names::OUTPUT_SLEW_MS).unwrap_or(defaults::OUTPUT_SLEW_MS);
        let trim_ms = get_value(storage, names::TRIM_MS).unwrap_or(defaults::TRIM_MS);
        let frequency_mode =
            get_value(storage, names::FREQUENCY_MODE).unwrap_or(FrequencyMode::Full);
        let target_frequency =
            get_value(storage, names::TARGET_FREQUENCY).unwrap_or(defaults::TARGET_FREQUENCY);

        let attack_ms = get_value(storage, names::ATTACK_MS).unwrap_or(defaults::ATTACK_MS);
        let decay_ms = get_value(storage, names::DECAY_MS).unwrap_or(defaults::DECAY_MS);
        let sustain_level =
            get_value(storage, names::SUSTAIN_LEVEL).unwrap_or(defaults::SUSTAIN_LEVEL);
        let release_ms = get_value(storage, names::RELEASE_MS).unwrap_or(defaults::RELEASE_MS);
        let attack_curve =
            get_value(storage, names::ATTACK_CURVE).unwrap_or(defaults::ATTACK_CURVE);
        let decay_curve = get_value(storage, names::DECAY_CURVE).unwrap_or(defaults::DECAY_CURVE);
        let release_curve =
            get_value(storage, names::RELEASE_CURVE).unwrap_or(defaults::RELEASE_CURVE);

        let gate_threshold =
            get_value(storage, names::GATE_THRESHOLD).unwrap_or(defaults::GATE_THRESHOLD);
        let auto_gate_amount =
            get_value(storage, names::AUTO_GATE_AMOUNT).unwrap_or(defaults::AUTO_GATE_AMOUNT);
        let gate_smoothing =
            get_value(storage, names::GATE_SMOOTHING).unwrap_or(defaults::GATE_SMOOTHING);

        let min_vibe = get_value(storage, names::MIN_VIBE).unwrap_or(defaults::MIN_VIBE);
        let max_vibe = get_value(storage, names::MAX_VIBE).unwrap_or(defaults::MAX_VIBE);
        let climax_mode_enabled =
            get_value(storage, names::CLIMAX_MODE_ENABLED).unwrap_or(defaults::CLIMAX_MODE_ENABLED);
        let climax_intensity =
            get_value(storage, names::CLIMAX_INTENSITY).unwrap_or(defaults::CLIMAX_INTENSITY);
        let climax_build_up_ms =
            get_value(storage, names::CLIMAX_BUILD_UP_MS).unwrap_or(defaults::CLIMAX_BUILD_UP_MS);
        let climax_tease_ratio =
            get_value(storage, names::CLIMAX_TEASE_RATIO).unwrap_or(defaults::CLIMAX_TEASE_RATIO);
        let climax_tease_drop =
            get_value(storage, names::CLIMAX_TEASE_DROP).unwrap_or(defaults::CLIMAX_TEASE_DROP);
        let climax_surge_boost =
            get_value(storage, names::CLIMAX_SURGE_BOOST).unwrap_or(defaults::CLIMAX_SURGE_BOOST);
        let climax_pulse_depth =
            get_value(storage, names::CLIMAX_PULSE_DEPTH).unwrap_or(defaults::CLIMAX_PULSE_DEPTH);
        let climax_pattern =
            get_value(storage, names::CLIMAX_PATTERN).unwrap_or(defaults::CLIMAX_PATTERN);

        let enable_persistence =
            get_value(storage, names::ENABLE_PERSISTENCE).unwrap_or(defaults::ENABLE_PERSISTENCE);
        let hold_delay_ms =
            get_value(storage, names::HOLD_DELAY_MS).unwrap_or(defaults::HOLD_DELAY_MS);
        let decay_rate_per_sec =
            get_value(storage, names::DECAY_RATE_PER_SEC).unwrap_or(defaults::DECAY_RATE_PER_SEC);

        let use_advanced_processing = get_value(storage, names::USE_ADVANCED_PROCESSING)
            .unwrap_or(defaults::USE_ADVANCED_PROCESSING);

        let current_preset_name: String = get_value(storage, names::CURRENT_PRESET_NAME)
            .unwrap_or_else(|| String::from("Ride Intensity"));

        Self {
            main_volume,
            low_pass_freq: SharedF32::new(low_pass_freq),
            use_dark_mode,
            start_scanning_on_startup,
            polling_rate_ms: SharedF32::new(polling_rate_ms),
            use_polling_rate: Arc::new(AtomicBool::new(use_polling_rate)),
            device_settings,
            save_device_settings,

            trigger_mode,
            binary_level,
            hybrid_blend,
            threshold_knee,
            dynamic_curve,
            input_rise_ms,
            input_fall_ms,
            output_slew_ms,
            trim_ms,
            frequency_mode,
            target_frequency,
            attack_ms,
            decay_ms,
            sustain_level,
            release_ms,
            attack_curve,
            decay_curve,
            release_curve,
            gate_threshold,
            auto_gate_amount,
            gate_smoothing,
            min_vibe,
            max_vibe,
            climax_mode_enabled,
            climax_intensity,
            climax_build_up_ms,
            climax_tease_ratio,
            climax_tease_drop,
            climax_surge_boost,
            climax_pulse_depth,
            climax_pattern,
            enable_persistence,
            hold_delay_ms,
            decay_rate_per_sec,
            use_advanced_processing,
            current_preset_name,
        }
    }

    pub fn save(&self, storage: &mut dyn Storage) {
        // Original
        set_value(storage, names::MAIN_VOLUME, &self.main_volume);
        set_value(storage, names::LOW_PASS_FREQ, &self.low_pass_freq.load());
        set_value(storage, names::DARK_MODE, &self.use_dark_mode);
        set_value(
            storage,
            names::START_SCANNING_ON_STARTUP,
            &self.start_scanning_on_startup,
        );
        set_value(
            storage,
            names::POLLING_RATE_MS,
            &self.polling_rate_ms.load(),
        );
        set_value(
            storage,
            names::USE_POLLING_RATE,
            &self.use_polling_rate.load(Ordering::Relaxed),
        );
        set_value(
            storage,
            names::SAVE_DEVICE_SETTINGS,
            &self.save_device_settings,
        );
        if self.save_device_settings {
            set_value(storage, names::DEVICE_SETTINGS, &self.device_settings);
        }

        // New
        set_value(storage, names::TRIGGER_MODE, &self.trigger_mode);
        set_value(storage, names::BINARY_LEVEL, &self.binary_level);
        set_value(storage, names::HYBRID_BLEND, &self.hybrid_blend);
        set_value(storage, names::THRESHOLD_KNEE, &self.threshold_knee);
        set_value(storage, names::DYNAMIC_CURVE, &self.dynamic_curve);
        set_value(storage, names::INPUT_RISE_MS, &self.input_rise_ms);
        set_value(storage, names::INPUT_FALL_MS, &self.input_fall_ms);
        set_value(storage, names::OUTPUT_SLEW_MS, &self.output_slew_ms);
        set_value(storage, names::TRIM_MS, &self.trim_ms);
        set_value(storage, names::FREQUENCY_MODE, &self.frequency_mode);
        set_value(storage, names::TARGET_FREQUENCY, &self.target_frequency);
        set_value(storage, names::ATTACK_MS, &self.attack_ms);
        set_value(storage, names::DECAY_MS, &self.decay_ms);
        set_value(storage, names::SUSTAIN_LEVEL, &self.sustain_level);
        set_value(storage, names::RELEASE_MS, &self.release_ms);
        set_value(storage, names::ATTACK_CURVE, &self.attack_curve);
        set_value(storage, names::DECAY_CURVE, &self.decay_curve);
        set_value(storage, names::RELEASE_CURVE, &self.release_curve);
        set_value(storage, names::GATE_THRESHOLD, &self.gate_threshold);
        set_value(storage, names::AUTO_GATE_AMOUNT, &self.auto_gate_amount);
        set_value(storage, names::GATE_SMOOTHING, &self.gate_smoothing);
        set_value(storage, names::MIN_VIBE, &self.min_vibe);
        set_value(storage, names::MAX_VIBE, &self.max_vibe);
        set_value(
            storage,
            names::CLIMAX_MODE_ENABLED,
            &self.climax_mode_enabled,
        );
        set_value(storage, names::CLIMAX_INTENSITY, &self.climax_intensity);
        set_value(storage, names::CLIMAX_BUILD_UP_MS, &self.climax_build_up_ms);
        set_value(storage, names::CLIMAX_TEASE_RATIO, &self.climax_tease_ratio);
        set_value(storage, names::CLIMAX_TEASE_DROP, &self.climax_tease_drop);
        set_value(storage, names::CLIMAX_SURGE_BOOST, &self.climax_surge_boost);
        set_value(storage, names::CLIMAX_PULSE_DEPTH, &self.climax_pulse_depth);
        set_value(storage, names::CLIMAX_PATTERN, &self.climax_pattern);
        set_value(storage, names::ENABLE_PERSISTENCE, &self.enable_persistence);
        set_value(storage, names::HOLD_DELAY_MS, &self.hold_delay_ms);
        set_value(storage, names::DECAY_RATE_PER_SEC, &self.decay_rate_per_sec);
        set_value(
            storage,
            names::USE_ADVANCED_PROCESSING,
            &self.use_advanced_processing,
        );
        set_value(
            storage,
            names::CURRENT_PRESET_NAME,
            &self.current_preset_name,
        );
    }
}
