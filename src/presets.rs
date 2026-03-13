// ==========================================================================
// presets.rs — Named Preset Configurations
//
// Presets snap all signal-processing parameters to known-good values.
// Think of these like synth patches — each one is tuned for a specific
// use case. Users pick a preset as a starting point, then tweak.
//
// Naming convention follows synthesizer tradition:
//   - Short evocative name
//   - Description of what it feels like / what it's for
// ==========================================================================

use crate::audio::{FrequencyMode, TriggerMode};

/// A complete snapshot of all signal-processing settings.
/// Does NOT include connection/device settings — only the "patch."
#[derive(Clone, Debug)]
pub struct Preset {
    pub name: &'static str,
    pub description: &'static str,
    pub category: PresetCategory,

    // Volume / Input
    pub main_volume: f32,

    // Frequency
    pub frequency_mode: FrequencyMode,
    pub target_frequency: f32,

    // Gate (Noise Gate)
    pub gate_threshold: f32,
    pub auto_gate_amount: f32,
    pub gate_smoothing: f32,

    // Trigger
    pub trigger_mode: TriggerMode,
    pub binary_level: f32,
    pub hybrid_blend: f32,

    // ADSR Envelope
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub sustain_level: f32,
    pub release_ms: f32,
    pub attack_curve: f32,
    pub decay_curve: f32,
    pub release_curve: f32,

    // Output Range
    pub min_vibe: f32,
    pub max_vibe: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetCategory {
    /// Tracks beats/drums — short, punchy envelopes
    Percussion,
    /// Follows melody/vocals — smooth, sustained
    Musical,
    /// Bass-focused — heavy, slow
    Bass,
    /// Special effects and creative use cases
    Effect,
    /// Starting points / neutral
    Init,
}

impl PresetCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Percussion => "DRUMS",
            Self::Musical => "MUSICAL",
            Self::Bass => "BASS",
            Self::Effect => "FX",
            Self::Init => "INIT",
        }
    }

    pub fn all() -> &'static [PresetCategory] {
        &[
            PresetCategory::Init,
            PresetCategory::Percussion,
            PresetCategory::Musical,
            PresetCategory::Bass,
            PresetCategory::Effect,
        ]
    }
}

// ---------------------------------------------------------------------------
// Factory Presets
// ---------------------------------------------------------------------------

pub fn factory_presets() -> Vec<Preset> {
    vec![
        // === INIT ===
        Preset {
            name: "Ride Intensity",
            description: "Neutral follower - smooth, non-rhythmic response that rides loudness",
            category: PresetCategory::Init,
            main_volume: 1.15,
            frequency_mode: FrequencyMode::Full,
            target_frequency: 200.0,
            gate_threshold: 0.07,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.22,
            trigger_mode: TriggerMode::Dynamic,
            binary_level: 0.8,
            hybrid_blend: 0.5,
            attack_ms: 30.0,
            decay_ms: 160.0,
            sustain_level: 0.9,
            release_ms: 320.0,
            attack_curve: 1.0,
            decay_curve: 1.0,
            release_curve: 1.15,
            min_vibe: 0.0,
            max_vibe: 1.0,
        },
        Preset {
            name: "Transparent",
            description: "Minimal processing — raw audio level drives output",
            category: PresetCategory::Init,
            main_volume: 1.5,
            frequency_mode: FrequencyMode::Full,
            target_frequency: 200.0,
            gate_threshold: 0.05,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.0,
            trigger_mode: TriggerMode::Dynamic,
            binary_level: 0.8,
            hybrid_blend: 0.5,
            attack_ms: 1.0,
            decay_ms: 10.0,
            sustain_level: 0.9,
            release_ms: 50.0,
            attack_curve: 1.0,
            decay_curve: 1.0,
            release_curve: 1.0,
            min_vibe: 0.0,
            max_vibe: 1.0,
        },
        // === PERCUSSION ===
        Preset {
            name: "Drum Hit",
            description: "Tight punch — snappy attack, fast decay, minimal sustain",
            category: PresetCategory::Percussion,
            main_volume: 1.2,
            frequency_mode: FrequencyMode::Full,
            target_frequency: 200.0,
            gate_threshold: 0.20,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.0,
            trigger_mode: TriggerMode::Dynamic,
            binary_level: 0.8,
            hybrid_blend: 0.5,
            attack_ms: 1.0,
            decay_ms: 40.0,
            sustain_level: 0.2,
            release_ms: 60.0,
            attack_curve: 0.3,
            decay_curve: 2.0,
            release_curve: 2.5,
            min_vibe: 0.0,
            max_vibe: 1.0,
        },
        Preset {
            name: "Kick Follow",
            description: "Locks to the kick drum — bass-only, binary pulse",
            category: PresetCategory::Percussion,
            main_volume: 1.5,
            frequency_mode: FrequencyMode::LowPass,
            target_frequency: 120.0,
            gate_threshold: 0.25,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.0,
            trigger_mode: TriggerMode::Binary,
            binary_level: 0.9,
            hybrid_blend: 0.5,
            attack_ms: 2.0,
            decay_ms: 30.0,
            sustain_level: 0.1,
            release_ms: 80.0,
            attack_curve: 0.3,
            decay_curve: 2.5,
            release_curve: 3.0,
            min_vibe: 0.0,
            max_vibe: 1.0,
        },
        Preset {
            name: "Staccato Pulse",
            description: "Sharp on/off — like a trance gate effect",
            category: PresetCategory::Percussion,
            main_volume: 1.0,
            frequency_mode: FrequencyMode::Full,
            target_frequency: 200.0,
            gate_threshold: 0.18,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.0,
            trigger_mode: TriggerMode::Binary,
            binary_level: 1.0,
            hybrid_blend: 0.5,
            attack_ms: 0.5,
            decay_ms: 5.0,
            sustain_level: 0.95,
            release_ms: 30.0,
            attack_curve: 0.2,
            decay_curve: 1.0,
            release_curve: 3.0,
            min_vibe: 0.0,
            max_vibe: 1.0,
        },
        // === MUSICAL ===
        Preset {
            name: "Slow Swell",
            description: "Gradual build like a string section — long attack, full sustain",
            category: PresetCategory::Musical,
            main_volume: 1.0,
            frequency_mode: FrequencyMode::Full,
            target_frequency: 200.0,
            gate_threshold: 0.10,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.3,
            trigger_mode: TriggerMode::Dynamic,
            binary_level: 0.8,
            hybrid_blend: 0.5,
            attack_ms: 200.0,
            decay_ms: 100.0,
            sustain_level: 0.85,
            release_ms: 500.0,
            attack_curve: 0.7,
            decay_curve: 1.0,
            release_curve: 1.5,
            min_vibe: 0.05,
            max_vibe: 0.85,
        },
        Preset {
            name: "Vocal Ride",
            description: "Tracks vocal energy — mid-range focus, smooth dynamics",
            category: PresetCategory::Musical,
            main_volume: 1.3,
            frequency_mode: FrequencyMode::BandPass,
            target_frequency: 1000.0,
            gate_threshold: 0.12,
            auto_gate_amount: 0.3,
            gate_smoothing: 0.2,
            trigger_mode: TriggerMode::Hybrid,
            binary_level: 0.5,
            hybrid_blend: 0.3,
            attack_ms: 15.0,
            decay_ms: 60.0,
            sustain_level: 0.7,
            release_ms: 200.0,
            attack_curve: 0.5,
            decay_curve: 1.2,
            release_curve: 1.8,
            min_vibe: 0.05,
            max_vibe: 0.9,
        },
        Preset {
            name: "Pluck",
            description: "Quick strike, medium ring-out — like a guitar pluck",
            category: PresetCategory::Musical,
            main_volume: 1.0,
            frequency_mode: FrequencyMode::Full,
            target_frequency: 200.0,
            gate_threshold: 0.15,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.0,
            trigger_mode: TriggerMode::Dynamic,
            binary_level: 0.8,
            hybrid_blend: 0.5,
            attack_ms: 2.0,
            decay_ms: 120.0,
            sustain_level: 0.3,
            release_ms: 300.0,
            attack_curve: 0.3,
            decay_curve: 1.8,
            release_curve: 2.5,
            min_vibe: 0.0,
            max_vibe: 1.0,
        },
        // === BASS ===
        Preset {
            name: "Sub Throb",
            description: "Deep bass tracking — heavy, slow, floor-shaking",
            category: PresetCategory::Bass,
            main_volume: 2.0,
            frequency_mode: FrequencyMode::LowPass,
            target_frequency: 80.0,
            gate_threshold: 0.12,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.1,
            trigger_mode: TriggerMode::Dynamic,
            binary_level: 0.8,
            hybrid_blend: 0.5,
            attack_ms: 8.0,
            decay_ms: 150.0,
            sustain_level: 0.75,
            release_ms: 400.0,
            attack_curve: 0.4,
            decay_curve: 1.2,
            release_curve: 1.5,
            min_vibe: 0.1,
            max_vibe: 1.0,
        },
        Preset {
            name: "Wobble Bass",
            description: "EDM-style — binary bass pulse with max output",
            category: PresetCategory::Bass,
            main_volume: 2.5,
            frequency_mode: FrequencyMode::LowPass,
            target_frequency: 150.0,
            gate_threshold: 0.15,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.0,
            trigger_mode: TriggerMode::Hybrid,
            binary_level: 0.85,
            hybrid_blend: 0.6,
            attack_ms: 3.0,
            decay_ms: 60.0,
            sustain_level: 0.5,
            release_ms: 120.0,
            attack_curve: 0.3,
            decay_curve: 1.5,
            release_curve: 2.0,
            min_vibe: 0.0,
            max_vibe: 1.0,
        },
        // === EFFECTS ===
        Preset {
            name: "Ambient Wash",
            description: "Always-on gentle hum that swells with the music",
            category: PresetCategory::Effect,
            main_volume: 0.8,
            frequency_mode: FrequencyMode::Full,
            target_frequency: 200.0,
            gate_threshold: 0.03,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.5,
            trigger_mode: TriggerMode::Dynamic,
            binary_level: 0.8,
            hybrid_blend: 0.5,
            attack_ms: 300.0,
            decay_ms: 200.0,
            sustain_level: 0.9,
            release_ms: 1500.0,
            attack_curve: 0.6,
            decay_curve: 1.0,
            release_curve: 1.0,
            min_vibe: 0.15,
            max_vibe: 0.6,
        },
        Preset {
            name: "Heartbeat",
            description: "Rhythmic pulse — consistent intensity, dramatic on/off",
            category: PresetCategory::Effect,
            main_volume: 1.0,
            frequency_mode: FrequencyMode::Full,
            target_frequency: 200.0,
            gate_threshold: 0.20,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.0,
            trigger_mode: TriggerMode::Binary,
            binary_level: 0.7,
            hybrid_blend: 0.5,
            attack_ms: 10.0,
            decay_ms: 50.0,
            sustain_level: 0.6,
            release_ms: 100.0,
            attack_curve: 0.8,
            decay_curve: 2.0,
            release_curve: 2.5,
            min_vibe: 0.0,
            max_vibe: 0.75,
        },
        Preset {
            name: "Hi-Hat Tingle",
            description: "High frequency only — sparkly, delicate, treble-reactive",
            category: PresetCategory::Effect,
            main_volume: 2.0,
            frequency_mode: FrequencyMode::HighPass,
            target_frequency: 4000.0,
            gate_threshold: 0.10,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.0,
            trigger_mode: TriggerMode::Dynamic,
            binary_level: 0.8,
            hybrid_blend: 0.5,
            attack_ms: 1.0,
            decay_ms: 25.0,
            sustain_level: 0.15,
            release_ms: 40.0,
            attack_curve: 0.2,
            decay_curve: 2.5,
            release_curve: 3.0,
            min_vibe: 0.0,
            max_vibe: 0.7,
        },
        // === DOMI 2 OPTIMIZED ===
        Preset {
            name: "Domi Bass Lock",
            description: "Domi 2 optimized — locks to bass/kick, punchy hybrid pulses with sustain body",
            category: PresetCategory::Bass,
            main_volume: 1.90,
            frequency_mode: FrequencyMode::LowPass,
            target_frequency: 140.0,
            gate_threshold: 0.14,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.06,
            trigger_mode: TriggerMode::Hybrid,
            binary_level: 0.78,
            hybrid_blend: 0.48,
            attack_ms: 2.0,
            decay_ms: 55.0,
            sustain_level: 0.52,
            release_ms: 85.0,
            attack_curve: 0.35,
            decay_curve: 1.6,
            release_curve: 1.9,
            min_vibe: 0.05,
            max_vibe: 1.0,
        },
        Preset {
            name: "Domi Immerse",
            description: "Domi 2 full-range — smooth dynamic following with generous sustain and body",
            category: PresetCategory::Musical,
            main_volume: 1.60,
            frequency_mode: FrequencyMode::Full,
            target_frequency: 200.0,
            gate_threshold: 0.08,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.12,
            trigger_mode: TriggerMode::Dynamic,
            binary_level: 0.8,
            hybrid_blend: 0.5,
            attack_ms: 12.0,
            decay_ms: 110.0,
            sustain_level: 0.78,
            release_ms: 280.0,
            attack_curve: 0.55,
            decay_curve: 1.2,
            release_curve: 1.4,
            min_vibe: 0.06,
            max_vibe: 0.92,
        },
        Preset {
            name: "Domi Edge",
            description: "Domi 2 edging — tight bass pulses, low sustain, dramatic contrast",
            category: PresetCategory::Percussion,
            main_volume: 2.10,
            frequency_mode: FrequencyMode::LowPass,
            target_frequency: 120.0,
            gate_threshold: 0.20,
            auto_gate_amount: 0.0,
            gate_smoothing: 0.0,
            trigger_mode: TriggerMode::Hybrid,
            binary_level: 0.92,
            hybrid_blend: 0.55,
            attack_ms: 1.0,
            decay_ms: 35.0,
            sustain_level: 0.28,
            release_ms: 50.0,
            attack_curve: 0.25,
            decay_curve: 2.2,
            release_curve: 2.8,
            min_vibe: 0.0,
            max_vibe: 1.0,
        },
    ]
}

/// Get a preset by name (case-insensitive)
pub fn find_preset(name: &str) -> Option<Preset> {
    factory_presets()
        .into_iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
}

/// Get all presets in a given category
pub fn presets_in_category(category: PresetCategory) -> Vec<Preset> {
    factory_presets()
        .into_iter()
        .filter(|p| p.category == category)
        .collect()
}
