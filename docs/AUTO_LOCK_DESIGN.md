# AUTO-LOCK — Design (Phase 1)

One button that fits the signal chain to the playing material: the most
rhythmic drive band, the punchiest trigger shape, and an envelope whose decay
fits the tempo — maximum *contrast* within the user's ceiling. Produced by a
grounded design study (engine-surface extraction → two independent design
lenses → adversarial skeptic pass), 2026-07-03.

## Core principle

"Most pleasurable" is not maximum intensity. Vibrotactile receptors adapt to
constant stimulation; the engine already fights this (micro-pauses, Climax
anti-adaptation). Auto-Lock maximizes contrast: pick the band that carries the
rhythm, fit decay/release into the inter-onset interval so every hit fully
blooms and the trough between hits is preserved.

## Target waveform (field brief, 2026-07-03)

Think like a bass drum: **punch up instantly, one continuous musical decay
down, and the decay LANDS exactly where the next beat begins.** A sawtooth
metronome with an organic boom. Never a flat dead gap (mechanical), never a
truncated cut (jolting). Concretely: instant attack (fast path), exponential
decay (curve ~1.8) spanning ~78% of the perceptually folded beat, landing on
a low sustain floor (~0.08) that doubles as the retrigger-ready state, with
jitter margin before the next hit. Side effect exploited deliberately: the
onset detector fires on the subdivision grid (eighth notes), and off-beat
onsets arrive mid-Decay where the engine eats them — only the felt beat
relaunches the punch.

Band selection is punch-first, not rhythm-first: per band, the median
per-hit energy jump times hit reliability, divided by the between-hit floor.
The kick's huge jump out of a quiet floor beats a loud-but-ticky hi-hat and
a bassline-smeared band.

## Architecture

A **supervising estimator-controller** (`src/auto_lock.rs`), owned by the App
and ticked inside `update()`. It READS what the engine already publishes
(band energies, spectral flux, centroid, onset events, tempo confidence,
envelope output) and WRITES a whitelisted subset of existing `Settings`
fields through a slew-limited glide. The engine is untouched on both
platforms — Rust/Kotlin parity and the golden-CSV CI are preserved by
construction.

## Safety by construction

The write-struct simply lacks the fields Auto-Lock must never touch:
`main_volume`, `output_gain`, `min_vibe`, `max_vibe`, per-device multipliers,
all `climax_*`, `trim_ms` (user latency calibration), `gate_threshold` /
`auto_gate_amount` (writing the gate creates a feedback loop with the onset
veto). `binary_level` is seeded from the observed dynamic envelope p90 with a
0.55 punch floor and a hard 0.85 cap — the USER's ceiling (`max_vibe`,
`output_gain`, device multipliers) always binds downstream, so the consent
boundary is structural, not statistical.

## State machine

```
IDLE --press--> LISTENING (>=4s valid audio within a 15s budget;
                            frozen while using_rms_fallback)
      --enough signal--> COMMIT (enums immediately, floats glide 1-2s)
                          -> LOCKED (score shown on the button)
      --not lockable---> NO_LOCK (honest message, nothing written)
LOCKED --revert--> restore pre-lock snapshot (one press)
LOCKED --keep----> dissolve lock into normal settings (explicit consent)
LOCKED --any manual param change or preset click--> lock cancels itself
```

## Estimator (rolling ~8s, time-based, deduplicated frames)

| Feature | How | Drives |
|---|---|---|
| Per-band rhythmic salience | Half-wave-rectified per-band energy delta accumulated at onset times | `frequency_mode` + `target_frequency` (needs >=1.3x margin over 2nd band, else Full) |
| Median / IQR inter-onset interval | Onset timestamp diffs | `decay_ms`, `release_ms`, `output_slew_ms` (decay MUST fit inside the IOI — onsets during Decay are silently eaten; retrigger only fires from Sustain) |
| Crest factor (PRE-volume energy) | p95/p50 of gate-side energy | `trigger_mode`, `hybrid_blend`, `dynamic_curve`, `threshold_knee`, input smoothing |
| Silence ratio | Fraction of near-zero frames | lock-score penalty |
| Envelope output p90 | Observed dynamic path output | `binary_level` cap |
| Median spectral centroid | Engine-exact linear norm `(centroid-100)/4000` | pre-compensates the engine's frequency shaping of sustain/release |

Lock score = f(tempo confidence, salience margin, silence penalty). Shown as
"LOCKED NN%". Below threshold → NO_LOCK.

## Verdict-mandated phase-1 requirements (not optional)

1. **Persistence guard:** eframe auto-saves Settings. While a lock is active,
   `save()` must persist the PRE-LOCK snapshot values for whitelisted fields,
   so a crash/quit can never silently make a lock permanent.
2. **Preset race:** preset application is a complete snapshot write — it must
   hard-cancel the lock and its glide, and invalidate the revert snapshot.
3. **attack_ms honesty:** any attack < 50ms takes the engine's instant-peak
   fast path. Write 20ms once; do not pretend finer control exists.
4. **Centroid compensation must use the engine's exact linear formula**, not a
   log-scale normalization.
5. **Time-based rings, not frame-count** — update() cadence is not a
   guaranteed 60Hz.

## Explicitly out of phase 1

Continuous servo / auto re-lock (gated on the safety pass), per-band
autocorrelation salience, onset-boundary enum scheduling, FLOW probe
hysteresis, `trim_ms`, anything Climax, save-lock-as-preset (blocked: the
Rust `Preset` struct lacks six supervisor-written fields), Android port,
rolling-mean intensity guard (requires a shadow engine; unimplementable).

## Later phases

- **Phase 2:** re-lock on song-change detection (opt-in), FLOW fallback
  profile for unlockable material, Android port of the supervisor.
- **Phase 3:** preference learning — explicit thumbs up/down nudges the
  feature→parameter mapping weights. Requires the safety phase complete.
