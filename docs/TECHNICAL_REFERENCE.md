# ChloeVibes — Technical Reference

Software version **1.5.0**. Product overview and install: [root README](../README.md).

This document is the long-form engineering reference (signal chain, protocols, parity, limitations, CI).

---

## General Data

| Item | Value |
|---|---|
| System type | Real-time audio-reactive haptic controller |
| Clients | Windows desktop (Rust, egui) and Android (Kotlin, Jetpack Compose) |
| Signal engine | Single fixed-order DSP chain, ported across both clients and parity-verified in CI |
| Frame rate | 60 Hz nominal (16 ms frame budget) |
| Spectral resolution | 2048-point FFT, 1024 usable bins, 23.4 Hz per bin at 48 kHz |
| Output interface | Lovense BLE over Nordic UART Service; Buttplug 9.0.9 client on desktop |
| Output resolution | Integer intensity 0 to 20 (21 discrete levels) |
| Software version | 1.5.0 |
| License | MIT |

---

## 1. General Description

ChloeVibes converts a live audio stream into a haptic drive signal in real time. The system captures system audio, performs spectral analysis, derives an amplitude envelope from spectral content and detected onsets, optionally applies a slow time-domain modulation layer, and transmits the result to a Bluetooth haptic device.

The system is delivered as two clients built on one shared signal engine:

- **Windows desktop.** Rust with `eframe`/`egui`. Captures system audio by WASAPI loopback. Drives devices through a Buttplug 9.0.9 client.
- **Android.** Kotlin with Jetpack Compose (Material3, dark). Captures the system output mix through the Android Visualizer API. Drives Lovense devices over Bluetooth Low Energy directly.

The Android engine is a direct port of the Rust engine (`src/audio.rs`). Output equivalence between the two clients is enforced by a continuous-integration parity test (Section 6). The clients are not permitted to diverge; CI fails on any deviation.

> **WARNING.** This system commands a physical haptic device against a human body. The desktop client implements a dead-man watchdog (output is commanded to stop if the processing pipeline stalls for 2 s), a panic-stop hook (devices are stopped before the process dies on a crash), and a verified stop-all control. These reduce, but do not eliminate, risk: the Android client has no watchdog in this version, and no software can stop a device if the process is killed outright. The operator retains full responsibility for output level and for stopping the device. See Section 9.

---

## 2. Theory of Operation: Signal Chain

Each audio frame is processed through a fixed-order chain. The order is invariant on both clients. No stage is reordered and no stage is skipped.

```
   SYSTEM AUDIO
        |
        v
   [ Spectral Analyzer ] ---> 2048-pt FFT, Hann window, 8 perceptual bands, centroid, flux
        |
        v
   [ Noise Gate ] ---------> hysteresis, optional auto-gate (25% open-time target)
        |
        v
   [ Beat Detector ] ------> adaptive flux onset detection, tempo tracking, onset prediction
        |
        v
   [ ADSR Envelope ] ------> attack/decay/sustain/release, velocity overshoot, frequency shaping
        |
        v
   [ Climax Engine ] ------> slow time-domain modulation, edge-and-deny (disabled by default)
        |
        v
   [ Output Map ] ---------> intensity 0 to 20, asymmetric slew
        |
        v
   LOVENSE DEVICE (BLE / Buttplug)
```

### 2.1 Spectral Analyzer

A 2048-point fast Fourier transform with a Hann window (symmetric variant) and 2/N magnitude normalization. The lower 1024 bins are retained, giving 23.4 Hz per bin at a 48 kHz sample rate. The desktop client uses `rustfft`. The Android client uses a hand-written radix-2 Cooley-Tukey transform on the microphone path and the Android Visualizer FFT output on the system-audio path. Both clients apply identical normalization.

The magnitude spectrum is reduced to eight perceptual bands across 20 Hz to 20 kHz. Band edges are identical on both clients: 20, 60, 250, 500, 2000, 4000, 6000, 12000, 20000 Hz. The band labels are Sub, Bass, Lo-Mid, Mid, Hi-Mid, Pres, Brill, Air.

The analyzer computes per-band RMS energy, spectral centroid (DC bin excluded), and half-wave-rectified spectral flux (the sum of positive bin-to-bin magnitude increases). The `rms_power` and `dominant_frequency` fields are reserved and are hardwired to 0.0 in the Rust engine.

### 2.2 Noise Gate

A hysteresis gate with threshold-proportional hysteresis and asymmetric smoothing: instantaneous open, smoothed close. An optional auto-gate maintains a 100-bin energy histogram, recalculated every 86 frames, and selects a threshold that holds the gate open approximately 25% of the time. The auto-gate result is blended with the manual threshold by a configurable amount.

### 2.3 Beat Detector

Onset detection runs on spectral flux against an adaptive threshold computed as the mean plus a multiple of the standard deviation over a 43-frame window. Onsets are subject to a 55 ms refractory cooldown, bounding detection at approximately 270 BPM at sixteenth-note resolution. Tempo is tracked across the most recent 16 onset timestamps. The engine publishes a predicted next-onset time when tempo confidence exceeds 0.5. Downstream, each client pre-fires the drive command approximately 76 ms ahead of the predicted onset when tempo confidence exceeds 0.6, compensating for transmission and actuator latency.

### 2.4 ADSR Envelope Processor

A full Attack-Decay-Sustain-Release envelope with an independent power-curve exponent per stage. Velocity overshoot drives the attack target to a maximum of 1.2 (120%) on hard transients. Frequency-dependent shaping, keyed to spectral centroid, reduces sustain by up to 25% and extends release by up to 40% for low-centroid content. During sustain the processor applies a five-layer modulation on irrational frequency ratios (0.17 to 2.7 Hz, selected so the summed waveform does not repeat) and deterministic stochastic micro-pauses: true-zero intervals of 3 to 6 frames (48 to 96 ms) recurring every 2 to 8 seconds. The minimum retrigger interval is 20 ms.

### 2.5 Climax Engine

Final-stage slow time-domain modulation. Disabled by default. See Section 3.

### 2.6 Output Map

A single parity-locked stage (`map_output` in Rust, `mapOutput` in Kotlin). Below threshold the stage returns zero. Above threshold the shaped envelope is mapped into the device range [min, max], scaled by gain, and clamped. Both clients apply asymmetric output slew: 85 ms nominal, with the rising edge at approximately 30 ms (0.35 of the configured slew). This stage is verified value-for-value by the parity test (Section 6).

### 2.7 Algorithm Selection (Desktop)

The desktop client provides two processing algorithms, selectable at runtime. The Android client always uses the advanced algorithm.

- **Advanced FFT + ADSR.** The default. The full signal chain described in Sections 2.1 through 2.6.
- **Original Chloe Vibes (RMS).** The original loudness-follower algorithm, inherited from the project predecessor and retained as a selectable mode. The capture thread derives a single loudness value from a low-pass filter and full-band RMS. The pipeline is: RMS loudness, scaled by the volume control, optional hold-and-decay persistence (configurable hold delay and decay rate), then clamp to 0 to 1. The FFT band analysis, gate, beat detector, ADSR envelope, and Climax Engine are bypassed in this mode.

### 2.8 FIND BOOM / AUTO-LOCK (Desktop)

One-press automatic parameter fitting (UI label **FIND BOOM**). On activation the client listens to 4 to 15 seconds of the playing material and derives: the punchiest frequency band (largest per-hit energy jump over the quietest between-hit floor, times hit consistency), the beat interval (median and IQR of merged inter-onset intervals with perceptual octave folding into 70 to 180 BPM), the material's crest factor, and the median spectral centroid. It then writes a fitted parameter set — drive band, gate, trigger mode and curve, and an envelope whose decay fits inside the beat interval — through a 1.5 s glide, and reports a lock-quality score on the button. Unlockable material (ambient, speech) is reported honestly as NO LOCK and nothing is written.

The default product path and the fitted response both target a bass-drum waveform: instant peak, one continuous exponential decay spanning most of the beat (~78% of the folded interval at curve 1.8), landing on a near-zero floor exactly as the next beat fires. Off-beat subdivision onsets land mid-Decay where the engine absorbs them. Every press starts a fresh listen: only audio arriving after the press is judged, so a NO LOCK can be retried immediately on new material.

FIND BOOM is a supervisor above the signal chain, not a chain stage: it writes the same whitelisted parameter fields the sliders write and cannot touch volume, output gain, the output floor/ceiling, Climax, or timing trim. Its binary trigger level is seeded from observed output with a hard 0.85 cap, and the user's configured ceiling always binds downstream. Expert knobs (trigger shape, slew, curves) live in a collapsed OVERRIDE section; the main surface is presets + FIND BOOM. Any manual parameter change or preset selection cancels the lock; one press reverts to the exact pre-lock settings; an active lock is never persisted to storage without an explicit Keep. Design document: `docs/AUTO_LOCK_DESIGN.md`.

---

## 3. Climax Engine

The Climax Engine applies slow time-domain modulation over multi-minute cycles to delay neural adaptation to a sustained drive signal. It is disabled by default in every preset and is engaged only by an experience preset, a one-click profile, or manual control.

### 3.1 Cycle Structure

Cycle length is 8 to 240 seconds, default 90 seconds, identical on both clients. Intensity ramps along one of three patterns:

- **Wave.** Smooth S-curve.
- **Stairs.** Quantized stepped climb.
- **Surge.** Front-loaded power curve.

In the terminal fraction of each cycle the engine either teases (sharp reduction followed by a slow rebuild) or surges to peak on an accelerating curve. Behavior escalates over the first six completed cycles (cycle maturity 0 to 1): tease depth and surge magnitude both increase with maturity.

### 3.2 Anti-Adaptation Layers

Six modulators are summed over the macro cycle to maintain an aperiodic drive signal:

| Layer | Function | Range |
|---|---|---|
| 5-oscillator micro-pulse | Five detuned sinusoids (detune 0.07 and 0.13) summed to a composite pulse | up to 7 Hz, 10 Hz during surge |
| Sub-harmonic flutter | Low-frequency resonance, deepening with maturity | 8% to 24% depth |
| Lorenz-attractor chaos | Deterministic chaotic oscillator (sigma 10, rho 28, beta 8/3); non-repeating | 6% to 18% depth |
| Breathing-rate modulation | Low-rate modulation at approximately 0.18 Hz | 6% to 16% depth |
| Stochastic micro-pauses | True-zero intervals, 48 to 96 ms, every 2 to 8 s (applied in the ADSR stage) | 3 to 6 frames |
| 5-layer sustain modulation | Irrational-ratio modulation (applied in the ADSR stage) | 0.17 to 2.7 Hz |

> **NOTE.** The micro-pauses and the five-layer sustain modulation are implemented in the EnvelopeProcessor (Section 2.4), one stage upstream of the Climax Engine. They are listed here as part of the anti-adaptation behavior.

### 3.3 Arousal Momentum and Edge-and-Deny

Arousal momentum accumulates by 0.12 per completed cycle, hard-capped at 0.75, and feeds peak gain to a maximum of 3.8x at full ramp. Momentum decays only during silence; an active session does not relax until the audio stops.

The edge-and-deny state machine monitors sustained high output and forces a reduction once the high-output dwell exceeds the trigger time. The parameters escalate with cycle maturity:

| Parameter | Early | Mature |
|---|---|---|
| Deny depth (reduction) | 60% | 90% |
| Deny duration | 0.6 s | 2.4 s |
| Trigger time (high-output dwell) | 6 s | 3 s |
| Post-deny overshoot | +0.30 | +0.55 (cap 0.65) |

On a device that reports a second actuator, the engine drives the secondary motor in a dynamic unison-to-anti-phase relationship: in phase at low output, increasingly out of phase as output rises.

### 3.4 Desktop One-Click Profiles

| Profile | Pattern | Intensity | Build-up | Notes |
|---|---|---|---|---|
| Edge | Wave | 0.62 | 130 s | Extended low-rate cycle |
| Overload | Surge | 0.88 | 75 s | Fast escalation |
| Punisher | Stairs | 1.0 | 55 s | Maximum intensity and modulation depth |

---

## 4. Preset Catalog

A preset is a complete snapshot of every signal parameter. Presets are organized into five categories: INIT, DRUMS, MUSICAL, BASS, FX.

| Client | Factory presets | Climax-enabled |
|---|---|---|
| Windows (desktop) | 30 | 3 (Slow Tease, Ride the Beat, Break Me) |
| Android | 33 | 3 (same three; Chloe macros are climax-off boom variants) |

| Preset | Category | Description |
|---|---|---|
| **Bass Drum** | INIT | **Default.** Kick-only natural BOOM — instant peak, ~375 ms exp decay (125 BPM), near-zero floor. |
| Ride Intensity | INIT | Pad / continuous loudness follower (not the boom path). |
| Hi-Hat Tingle | FX | High-pass, treble-reactive. Present on both clients. |
| Slow Tease | Experience | 120 s edging cycle, Wave pattern. |
| Ride the Beat | Experience | 60 s music-locked escalation, Surge pattern. |
| Break Me | Experience | 45 s build to maximum intensity, deepest pulse, dual-motor anti-phase. |

> **NOTE.** The `threshold_knee` and `dynamic_curve` fields exist in the Android `Preset` structure but not yet in the desktop catalog structure.

---

## 5. Output Interface and Device Compatibility

### 5.1 Lovense Protocol

Commands are ASCII strings terminated with a semicolon, transmitted over the Nordic UART Service. Intensity is an integer from 0 to 20 (21 discrete levels). Single-motor: `Vibrate:N;`. Dual-motor: `Vibrate1:X;Vibrate2:Y;`.

### 5.2 Device Support

| Capability | Windows (desktop) | Android |
|---|---|---|
| Transport | Buttplug 9.0.9 client to Intiface (default); embedded server fallback | Direct BLE GATT over Nordic UART Service |
| Device reach | Any device supported by the connected Buttplug or Intiface server | Lovense devices reachable over BLE |
| Single-motor | Supported | Supported |
| Dual-motor | Any multi-actuator scalar device reported by the server | Lovense Edge and Edge 2 only (see 5.3) |

### 5.3 Android Dual-Motor Limitation

Independent `Vibrate1`/`Vibrate2` control on Android is confirmed only for the Lovense Edge and Edge 2 (DeviceType code `P`, fixture-verified). All other devices, including the Domi 2, are driven as a single motor by design.

### 5.4 Android BLE Connection Sequence

Unfiltered low-latency scan with a 15 s timeout. Connect over TRANSPORT_LE. Request HIGH connection priority. Request a 185-byte MTU. Call `discoverServices()` only from the `onMtuChanged` callback. Scanning stops before any connection attempt; the GATT client is closed on every disconnect path; unexpected link loss triggers automatic reconnection with exponential backoff (600 ms to 8 s, 6 attempts). On Android 12+ `BLUETOOTH_SCAN` uses `neverForLocation`.

**Desktop device lifecycle.** Dropped devices are pruned; reconnect within 60 s resumes enable state and per-device tuning. Buttplug server health is checked every frame.

---

## 6. Cross-Platform Parity

| Stage | Windows (Rust) | Android (Kotlin) | Status |
|---|---|---|---|
| Spectral / FFT | `rustfft`, 2048-point | radix-2 / Visualizer FFT | Equivalent |
| Gate | Proportional hysteresis, 25% auto-gate | Ported | Equivalent |
| Beat detector | Adaptive flux onset, tempo, prediction | Ported; 76 ms pre-fire | Equivalent |
| ADSR | Full ADSR + shaping | Ported | Equivalent |
| Climax engine | 8–240 s, 6 layers | Ported | Equivalent |
| Output stage | `map_output` | `mapOutput` | Parity-tested |
| Dual-motor | Per-actuator index | Edge / Edge 2 only | Not equivalent |

`tests/parity.rs` + Android `ParityTest.kt` — 6 scenarios × 468 frames. CI fails on drift.

---

## 7. Specifications

| Parameter | Value |
|---|---|
| Signal chain | Spectral, Gate, Beat, ADSR, Climax, Output |
| FFT | 2048-point, Hann, 1024 bins, 23.4 Hz @ 48 kHz |
| Frame rate | 60 Hz nominal; Android UI ~30 Hz |
| Predictive onset lead | 76 ms at tempo confidence > 0.6 |
| Output resolution | Lovense 0–20 integer |
| BLE command rate | Desktop 50 Hz; Android 33 Hz |
| Desktop stack | Rust, eframe/egui 0.33.3, Buttplug 9.0.9 |
| Android stack | `com.ashairfoil.chloevibes` 1.5.0 (versionCode 5) |
| Safety (desktop) | Watchdog 2 s, panic-stop, verified stop-all, session.log |
| License | MIT |

---

## 8. Installation and Operation

See the [root README](../README.md) for product install. From source:

```sh
cargo run --release
cargo run --release -- --server-addr ws://127.0.0.1:12345
cd android && ./gradlew assembleDebug
```

---

## 9. Limitations

- Safety coverage is partial (Android has no dead-man watchdog yet).
- No automatic intensity limiter — operator owns the ceiling.
- FIND BOOM is desktop-only and one-shot (press again to re-lock).
- Android dual-motor: Edge / Edge 2 only.
- Catalog parity gap on desktop for Chloe presets / knee fields.
- `rms_power` / `dominant_frequency` hardwired 0.0 in Rust.

---

## 10. Build, Test, and CI

```sh
cargo build --release
cargo test
cargo test --test parity
cd android && ./gradlew assembleDebug testDebugUnitTest
```

CI: `fmt` · `clippy -D warnings` · release build · parity · Android unit tests. Rolling APK workflow publishes `android-latest` only (not the dual-platform Latest).

---

## 11. Modification constraints

1. Signal-chain order is invariant on both clients.
2. Engines are one specification in two implementations — change both; parity enforces it.
