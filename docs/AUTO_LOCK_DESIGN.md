# FIND BOOM — Automatic response fitting

Design reference for ChloeVibes **1.6.0**. The desktop implementation is
[`src/auto_lock.rs`](../src/auto_lock.rs); its internal name is AUTO-LOCK.
See the [technical reference](TECHNICAL_REFERENCE.md) for the shared signal
engine and output controls.

One button fits the signal chain to the playing material: a rhythmic drive
band, trigger shape, and an envelope whose decay fits the detected tempo.

## Core principle

Auto-Lock targets contrast between hits and troughs. It selects a drive band
and fits an envelope to the observed onset interval. The score describes
audio features and timing confidence, not measured pleasure or physical force.

## Target waveform

The fitted response follows a bass-drum shape: an immediate command peak,
a curved decay spanning about 78% of the perceptually folded beat interval,
and a low sustain level around 0.08. The decay curve is about 1.8, with
timing margin before the next expected hit. Onsets arriving during Decay
are absorbed, which can prevent detected subdivisions from retriggering
the envelope. Actual timing depends on capture, detected rhythm, envelope
state, and hardware response.

Band selection combines the median per-hit energy jump, hit reliability,
and a bounded contrast factor `hit / (hit + 3 * floor + epsilon)`. The floor
is mean positive energy change between onset-aligned windows. This gives
absolute hit size more weight than a tiny transient with an almost zero floor.

## Architecture

The App owns the supervisor and calls it inside `update()`. It reads
published band energies, centroid, onset events, tempo confidence, and
pre-volume energy, then updates a selected set of existing `Settings`
fields through a 1.5-second glide. The supervisor is desktop-only and does
not add a stage to the shared Rust/Kotlin DSP chain. The synthetic parity
tests cover the engine, not this parameter-fitting controller.

## Parameter limits and rollback

The fitted fields are frequency focus, trigger parameters, envelope timing
and curves, output slew, gate threshold, and gate smoothing. A fit sets
auto-gate to zero and disables Climax to isolate the boom response. It does
not adjust `main_volume`, `output_gain`, `min_vibe`, `max_vibe`, device
multipliers, Climax intensity/cycle parameters, or timing delay.

`binary_level` is derived from crest factor plus a kick-band boost and
clamped to 0.70–0.88. It is not fitted to measured motor strength. Output
mapping and per-device limits apply after the fitted envelope.

## State machine

```
IDLE --press--> LISTENING (>=4s valid audio within a 15s budget;
                            RMS fallback frames do not count as valid audio)
      --enough signal--> COMMIT (enums immediately, floats glide 1.5s)
                          -> LOCKED (score shown on the button)
      --not lockable---> NO_LOCK (honest message, nothing written)
LOCKED --revert--> restore pre-lock snapshot (one press)
LOCKED --keep----> dissolve lock into normal settings (explicit consent)
LOCKED --manual fitted-parameter change or preset click--> lock cancels itself
```

The snapshot includes auto-gate, the Climax enable flag, and preset name as
well as the fitted numeric parameters. Revert restores those values; Keep
applies the final fitted target even if the glide is still running. Revert
and Keep remain available while retrying, after a failed retry, or after
cancelling that retry. An initial cancelled listen has no changes to revert.
Manual takeover leaves the current edited values in place and ends the
autosave snapshot guard.

## Estimator (rolling ~8s, time-based, deduplicated frames)

The host supplies a fresh-capture flag; identical newly captured spectra
still count, while repeated UI reads do not. Valid listening time counts
adjacent valid frames at most 100 ms apart and excludes time before the
current listen. Missing callbacks cannot count as a long stretch of music.

| Feature | How | Drives |
|---|---|---|
| Per-band punch | Median onset-aligned positive energy jump × reliability × bounded contrast | `frequency_mode` + `target_frequency`; a >=1.3× lead selects the winning band, otherwise crest/low-band evidence can select bass, else Full |
| Median / IQR inter-onset interval | Onset timestamp diffs | `decay_ms`, `release_ms`, `output_slew_ms`; decay fits within the folded interval to leave a retrigger window |
| Crest factor (PRE-volume energy) | p95/p50 of gate-side energy | `trigger_mode`, `hybrid_blend`, `dynamic_curve`, `binary_level` |
| Silence ratio | Fraction of near-zero frames | lock-score penalty |
| Hit/trough energy | Median pre-volume energy in and between onset-aligned windows | `gate_threshold` and gate smoothing |
| Median spectral centroid | Engine-exact linear norm `(centroid-100)/4000` | pre-compensates the engine's frequency shaping of sustain/release |

Lock score combines tempo confidence, salience margin, silence ratio, and
crest factor. Shown as "BOOM NN%"; it is an estimator score rather than a
percentage of correct beats. Below threshold → NO_LOCK.

## Implementation requirements

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

## Current scope

FIND BOOM is a desktop-only, one-shot tuner. It does not automatically
re-lock when a song changes, calibrate timing delay, fit Climax parameters,
save custom presets, or learn preferences. Those capabilities are possible
extensions, not features of this implementation.
