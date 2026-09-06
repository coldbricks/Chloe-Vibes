# ChloeVibes — Technical Reference

Software version **1.6.1**. Product overview and install: [root README](../README.md).

This document is the long-form engineering reference (signal chain, protocols, parity, limitations, CI).

---

## General Data

| Item | Value |
|---|---|
| System type | Real-time audio-reactive haptic controller |
| Clients | Windows desktop (Rust, egui) and Android (Kotlin, Jetpack Compose) |
| Signal engine | Fixed-order DSP chain ported across clients; covered by synthetic cross-platform regression tests |
| Processing cadence | Capture, DSP output, UI, and transport have separate cadences; see Sections 2 and 7 |
| Spectral resolution | 2048-point FFT, 1024 usable bins, 23.4 Hz per bin at 48 kHz |
| Output interface | Lovense BLE UART services; Buttplug 9.0.9 client on desktop |
| Output resolution | Normalized DSP output; Lovense BLE commands use integer intensity 0 to 20 |
| Software version | 1.6.1 |
| License | MIT |

---

## 1. General Description

ChloeVibes converts a live audio stream into a haptic drive signal in real time. It captures the selected audio source, performs spectral analysis, derives an amplitude envelope from spectral content and detected onsets, optionally applies a slow time-domain modulation layer, and transmits the result to a connected haptic device.

The system is delivered as two clients built on one shared signal engine:

- **Windows desktop.** Rust with `eframe`/`egui`. Captures system audio by WASAPI loopback. Drives devices through a Buttplug 9.0.9 client.
- **Android.** Kotlin with Jetpack Compose (Material3, dark). Captures the system output mix through Android Visualizer, or microphone PCM when explicitly selected. Drives Lovense devices over Bluetooth Low Energy directly.

The Android engine is a port of the Rust engine (`src/audio.rs`). Continuous-integration parity tests compare selected DSP outputs for a shared synthetic PCM input (Section 6). Capture APIs, live scheduling, output smoothing, and device transport are separate client code and require separate verification.

Both clients request a device stop if the processing heartbeat stalls for 2 seconds. Android latches this stop until explicit resume. Desktop also attempts a stop from its panic hook and reports failures from Stop all devices. An accepted command is not a measurement of motor motion; connection loss or abrupt process termination can prevent a stop from reaching the device.

---

## 2. Theory of Operation: Signal Chain

Advanced processing uses the following fixed-order chain. Climax modulation can be disabled. Desktop's selectable RMS mode is a separate legacy path (Section 2.7).

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
   [ Output Map ] ---------> normalized intensity, asymmetric slew
        |
        v
   LOVENSE DEVICE (BLE / Buttplug)
```

### 2.1 Spectral Analyzer

A 2048-point fast Fourier transform with a Hann window (symmetric variant) and 2/N magnitude normalization is used for PCM input. The lower 1024 bins are retained, giving 23.4 Hz per bin at a 48 kHz sample rate. The desktop client uses `rustfft`. The Android client uses a hand-written radix-2 Cooley-Tukey transform on the microphone path. Its system-audio path receives packed FFT data from Android Visualizer, whose capture size and scaling differ from the PCM path.

The magnitude spectrum is reduced to eight perceptual bands across 20 Hz to 20 kHz. Band edges are identical on both clients: 20, 60, 250, 500, 2000, 4000, 6000, 12000, 20000 Hz. The band labels are Sub, Bass, Lo-Mid, Mid, Hi-Mid, Pres, Brill, Air.

The analyzer computes per-band RMS energy, spectral centroid (DC bin excluded), and half-wave-rectified spectral flux (the sum of positive bin-to-bin magnitude increases). The `rms_power` and `dominant_frequency` fields are reserved and are hardwired to 0.0 in the Rust engine.

On Windows, capture publishes a sequence number with each analyzed spectrum. Gate and beat statistics consume each publication once, including successive identical spectra. If no new audio packets arrive for 250 ms, the capture path clears retained audio and the host suppresses output. After 2 seconds without packets it reopens the playback source. The device-output loop also checks capture freshness, separately from the UI processing heartbeat.

Android exposes explicit System audio and Microphone sources; source selection is locked during capture. Missing system audio never silently enables the microphone. Input older than 120 ms produces zero output, and held frames do not rerun FFT, gate, or beat statistics. System audio is retried after 3 seconds without callbacks, up to three consecutive attempts; exhausted retries or a stalled microphone stop capture with an error. Microphone capture is refused or stopped during an active call. Source restarts and UI reset requests use one monotonic processing clock; the processing thread applies DSP resets.

### 2.2 Noise Gate

A hysteresis gate with threshold-proportional hysteresis and asymmetric smoothing: instantaneous open, smoothed close. An optional auto-gate maintains a 100-bin energy histogram, recalculated every 86 frames, and selects a threshold that holds the gate open approximately 25% of the time. The auto-gate result is blended with the manual threshold by a configurable amount.

### 2.3 Beat Detector

Onset detection runs on spectral flux against an adaptive threshold computed as the mean plus a multiple of the standard deviation over a 43-frame window. Onsets are subject to a 55 ms refractory cooldown, bounding detection at approximately 270 BPM at sixteenth-note resolution. Tempo is tracked across the most recent 16 onset timestamps. The engine publishes a predicted next-onset time when tempo confidence exceeds 0.5. Downstream, each client pre-fires the drive command approximately 50 ms ahead of the predicted onset when tempo confidence exceeds 0.6 and a real onset was seen within about two beats (recency guard). Without new onsets, tempo confidence decays so stale locks cannot ghost-fire into silence or the next track.

Confidence decays from the last real-onset baseline according to elapsed time, independently of how often the host polls, and clears after three missed beat periods. A confirmed pre-fire suppresses one matching real envelope attack, including a coincident gate-open edge; the real onset still contributes to tempo tracking and desktop tuner evidence.

### 2.4 ADSR Envelope Processor

A full Attack-Decay-Sustain-Release envelope with an independent power-curve exponent per stage. Velocity overshoot drives the attack target to a maximum of 1.2 (120%) on hard transients. Frequency-dependent shaping, keyed to spectral centroid, reduces sustain by up to 25% and extends release by up to 40% for low-centroid content. During sustain the processor applies five slow modulation components (approximately 0.17 to 2.7 Hz) and deterministic pseudo-random micro-pauses: zero-output intervals of 90 to 120 ms recurring every 2 to 8 seconds after the first pause. These events bypass output slew and Android peak-hold. Actual motor coast-down depends on the hardware. The minimum retrigger interval is 20 ms.

The first trigger can start at time zero. Stage changes use their actual elapsed-time boundaries, so a delayed host frame can advance through several completed stages without stretching the envelope. Reset also clears the remembered gate edge, allowing a newly opened gate to trigger normally.

### 2.5 Climax Engine

Final-stage slow time-domain modulation. Disabled by default. See Section 3.

### 2.6 Output Map

The shared mapping stage (`map_output` in Rust, `mapOutput` in Kotlin) returns zero for silence, an effectively zero signal, or non-finite inputs. Active output is mapped through the configured range, multiplied by gain, and capped at the configured maximum. A floor above the maximum is reduced to that maximum.

Each host then applies asymmetric output slew. The Bass Drum default is 42 ms on falling edges, with ordinary rises using 0.35 of the configured time. Large upward jumps use a faster rise, limited to 1–12 ms, and zero-output events bypass smoothing. These are command-shaping time constants, not measurements of motor latency. Desktop device and actuator multipliers/caps are a subsequent stage: check those controls as well as the shared output range. The golden test covers the shared mapping, not host slew or transport timing.

### 2.7 Algorithm Selection (Desktop)

The desktop client provides two processing algorithms, selectable at runtime. The Android client always uses the advanced algorithm.

- **Advanced FFT + ADSR.** The default. The full signal chain described in Sections 2.1 through 2.6.
- **Original Chloe Vibes (RMS).** The original loudness-follower algorithm, inherited from the project predecessor and retained as a selectable mode. The capture thread derives a single loudness value from a low-pass filter and full-band RMS. The pipeline is: RMS loudness, scaled by the volume control, optional hold-and-decay persistence (configurable hold delay and decay rate), then clamp to 0 to 1. The FFT band analysis, gate, beat detector, ADSR envelope, and Climax Engine are bypassed in this mode.

Changing algorithms resets the gate, beat detector, envelope, modulation state, output levels, and delay history. The advanced **Output delay** control adds 0–500 ms using timestamped history. It is a delay-only control; onset prediction is separate. Rests travel with the delayed phrase, while capture loss stops output and clears pending history.

### 2.8 FIND BOOM / AUTO-LOCK (Desktop)

One-press automatic parameter fitting (UI label **FIND BOOM**). On activation the client listens to 4 to 15 seconds of the playing material and derives: the punchiest frequency band (largest per-hit energy jump over the quietest between-hit floor, times hit consistency), the beat interval (median and IQR of merged inter-onset intervals with perceptual octave folding into 70 to 180 BPM), the material's crest factor, and the median spectral centroid. The gate threshold is calibrated in the newly selected frequency domain, so switching the drive band does not reuse a threshold from a different energy scale. It then writes a fitted parameter set — drive band, gate, trigger mode and curve, and an envelope whose decay fits inside the beat interval — through a 1.5 s glide, and reports a lock-quality score on the button. Unlockable material (ambient, speech) is reported honestly as NO LOCK and nothing is written.

The default product path and the fitted response target a bass-drum waveform: instant peak, curved decay spanning most of the beat (~78% of the folded interval at curve 1.8), then a low-sustain release. Onsets arriving mid-Decay are absorbed. The fit leaves timing margin for the next hit; it does not guarantee a physical peak on every beat. Every press starts a fresh listen using audio arriving after that press.

FIND BOOM is a supervisor above the signal chain. It adjusts frequency focus, gate threshold/smoothing, trigger shape, envelope, and slew; it sets auto-gate to zero and disables Climax for the boom response. It leaves volume, output gain, output floor/ceiling, device multipliers, Climax intensity/cycle parameters, and timing delay untouched. Binary trigger level comes from crest factor plus a kick-band boost, clamped to 0.70–0.88. The desktop groups expert settings in a collapsible full-controls section, keeping volume, presets, FIND BOOM, and output limits visible. Design document: [AUTO_LOCK_DESIGN.md](AUTO_LOCK_DESIGN.md).

The reversible snapshot includes every tuner-written field, including auto-gate, the Climax enable flag, and preset name. Revert restores that snapshot. Keep finishes an in-progress glide at its fitted target and releases the snapshot. Keep/Revert remain available when a subsequent listen fails or is cancelled. Preset selection cancels the tuner immediately; a manual change to a fitted parameter cancels it without undoing the edit. Until Keep or a manual takeover, autosave writes the pre-lock values for tuner-owned fields.

---

## 3. Climax Engine

The Climax Engine varies the drive signal over a configurable cycle. It is disabled in the default Bass Drum preset and enabled in selected factory presets, one-click profiles, or by manual control. The modulation describes generated commands, not a measured physiological effect.

### 3.1 Cycle Structure

Cycle length is 8 to 240 seconds, default 90 seconds, identical on both clients. Intensity ramps along one of three patterns:

- **Wave.** Smooth S-curve.
- **Stairs.** Quantized stepped climb.
- **Surge.** Front-loaded power curve.

The terminal portion of each cycle has a tease interval followed by a separate surge interval in the final 12%. Cycle maturity grows from 0 to 1 over six completed cycles, then grows more slowly to a cap of 1.2. This changes several modulation depths and timings.

### 3.2 Modulation Layers

The macro cycle combines several sources of variation:

| Layer | Function | Range |
|---|---|---|
| 5-oscillator micro-pulse | Five detuned sinusoids (detune 0.07 and 0.13) summed to a composite pulse | up to 7 Hz, 10 Hz during surge |
| Sub-harmonic flutter | Low-frequency resonance, deepening with maturity | 8% to 24% depth |
| Lorenz-attractor chaos | Deterministic chaotic oscillator (sigma 10, rho 28, beta 8/3); non-repeating | 6% to 18% depth |
| Breathing-rate modulation | Low-rate modulation at approximately 0.18 Hz | 6% to 16% depth |
| Stochastic micro-pauses | Zero-output intervals every 2 to 8 s (applied in the ADSR stage) | 90 to 120 ms, time-based |
| 5-layer sustain modulation | Irrational-ratio modulation (applied in the ADSR stage) | 0.17 to 2.7 Hz |

> **NOTE.** The micro-pauses and the five-layer sustain modulation are implemented in the EnvelopeProcessor (Section 2.4), one stage upstream of the Climax Engine.

### 3.3 Arousal Momentum and Edge-and-Deny

The internal variable `arousal_momentum` accumulates by 0.12 per completed cycle, capped at 0.75, and contributes to the output modulation. It decays while the gate is closed. The name denotes algorithm state; the software does not measure the user's arousal.

The edge-and-deny state machine monitors sustained high output and forces a reduction once the high-output dwell exceeds the trigger time. The parameters escalate with cycle maturity:

| Parameter | Initial | At maximum timing maturity |
|---|---|---|
| Nominal deny reduction | 70% | 95% |
| Deny duration | 0.7–1.2 s | 3.23–3.73 s |
| Trigger time (high-output dwell) | 6 s | 2.32 s |
| Added post-deny onset boost | +0.40 | +0.8025; accumulated boost capped at 0.85 |

When the deny envelope reaches a reduction of 30% or more, the engine emits a zero-output event for both motors. The nominal reduction therefore describes the transition shape, not a continuous low plateau during the hold.

On a device that reports a second actuator, the engine drives the secondary motor in a dynamic unison-to-anti-phase relationship: in phase at low output, increasingly out of phase as output rises.

### 3.4 Desktop One-Click Profiles

| Profile | Pattern | Intensity | Build-up | Notes |
|---|---|---|---|---|
| Edge | Wave | 0.62 | 130 s | Extended low-rate cycle |
| Overload | Surge | 0.88 | 75 s | Fast escalation |
| Punisher | Stairs | 1.0 | 55 s | Maximum intensity and modulation depth |

---

## 4. Preset Catalog

A preset groups input, trigger, envelope, output-range, and modulation settings. Presets are organized into five categories: INIT, DRUMS, MUSICAL, BASS, FX.

| Client | Factory presets | Climax-enabled |
|---|---|---|
| Windows (desktop) | 33 | 5 (Edge & Deny, Crescendo, Slow Tease, Ride the Beat, Break Me) |
| Android | 33 | 5 (same five; Deep 90 / Club 125 / Hard 140 are Climax-off boom variants) |

| Preset | Category | Description |
|---|---|---|
| **Bass Drum** | INIT | **Default.** Kick-only natural BOOM — instant peak, ~375 ms exp decay (125 BPM), near-zero floor. |
| Ride Intensity | INIT | Pad / continuous loudness follower (not the boom path). |
| Hi-Hat Tingle | FX | High-pass, treble-reactive. Present on both clients. |
| Slow Tease | FX | Slower Climax cycle, Wave pattern. |
| Ride the Beat | FX | Music-reactive Climax modulation, Surge pattern. |
| Break Me | FX | Faster Climax build, stronger modulation, dual-motor variation. |

Both catalog structures include threshold-knee and dynamic-curve parameters. Presets change input, envelope, output-range, and modulation settings; they are starting points for adjustment, not device calibration profiles.

---

## 5. Output Interface and Device Compatibility

### 5.1 Lovense Protocol

Commands are ASCII strings terminated with a semicolon, transmitted over BLE UART services. The Android client recognizes Nordic UART Service and several firmware-specific Lovense service/characteristic UUID sets, with a writable/notifiable service-pair fallback. Intensity is an integer from 0 to 20 (21 discrete levels). Single-motor: `Vibrate:N;`. Dual-motor: `Vibrate1:X;Vibrate2:Y;`.

### 5.2 Device Support

| Capability | Windows (desktop) | Android |
|---|---|---|
| Transport | Buttplug 9.0.9 client to Intiface (default); embedded server fallback | Direct BLE GATT over compatible UART services |
| Device reach | Devices exposing supported scalar vibration or oscillation actuators through Buttplug / Intiface | Lovense devices reachable over BLE |
| Single-motor | Supported | Supported |
| Dual-motor | Any multi-actuator scalar device reported by the server | Lovense Edge and Edge 2 only (see 5.3) |

### 5.3 Android Dual-Motor Limitation

Independent `Vibrate1`/`Vibrate2` control on Android is enabled only for the Lovense Edge and Edge 2 (DeviceType code `P`, covered by protocol fixtures). All other devices, including the Domi 2, use the single-motor command path. Fixtures verify command selection, not physical operation on every firmware revision.

### 5.4 Android BLE Connection Sequence

Unfiltered low-latency scan with a 15 s timeout. Connect over TRANSPORT_LE and request HIGH connection priority and a 185-byte MTU. Service discovery follows the MTU callback, or starts immediately if the MTU request cannot be started. A 5-second setup deadline disconnects an incomplete attempt. The device becomes Ready only after notification subscription is acknowledged by a successful CCCD descriptor-write callback.

Scanning stops before any connection attempt; the GATT client is closed on every disconnect path; unexpected link loss triggers automatic reconnection with exponential backoff (600 ms to 8 s, 6 attempts). On Android 12+ `BLUETOOTH_SCAN` uses `neverForLocation`.

Writes use one in-flight operation. Pending stops take priority, followed by device/battery queries and the newest vibration command. A queued vibration does not replace a pending stop. Failed callbacks leave the current/latest command eligible for retry; a write callback missing for 500 ms disconnects the link. Connection-generation checks reject stale scheduled work and callbacks. Ordinary writes are spaced at least 28 ms apart; stop writes use a 12 ms interval. Both motor channels participate in change and zero-output decisions.

Explicit disconnect rejects further positive commands and gives a queued stop up to 500 ms to complete before closing the GATT client. A successful stop-write callback closes it sooner. This is a bounded delivery attempt, not confirmation that the physical motor has stopped.

**Desktop device lifecycle.** Dropped devices are pruned; reconnect within 60 s resumes enable state and per-device tuning. Buttplug server health is checked every frame.

Desktop dispatch compares the final requested values for every vibration and oscillation actuator against the last acknowledged command. Failed sends remain pending for retry; an unsuccessful zero command is not recorded as a completed stop. Device and actuator control changes therefore participate in dispatch even when the main signal level is steady.

---

## 6. Cross-Platform Parity

| Stage | Windows (Rust) | Android (Kotlin) | Status |
|---|---|---|---|
| Spectral / FFT | `rustfft`, 2048-point PCM | radix-2 PCM / Visualizer FFT | Synthetic PCM path covered; Visualizer excluded |
| Gate | Proportional hysteresis, 25% auto-gate | Ported | Covered synthetic scenarios |
| Beat detector | Adaptive flux onset, tempo, prediction | Ported; 50 ms prediction window | Onset path covered; prediction has separate unit tests |
| ADSR | Full ADSR + shaping | Ported | Covered synthetic scenarios |
| Climax engine | 8–240 s, modulation layers | Ported | Covered synthetic scenarios |
| Output stage | `map_output` | `mapOutput` | Parity-tested |
| Dual-motor | Per-actuator index | Edge / Edge 2 only | Not equivalent |

`tests/parity.rs` and Android `ParityTest.kt` run 6 scenarios of 468 frames each and compare the selected output columns against a shared golden CSV within numerical tolerances. They exercise Dynamic/Binary/Hybrid triggers, Wave/Stairs/Surge modulation, a high-sustain case, motor 2, and range/gain mapping. These tests do not cover live capture APIs, UI scheduling, transport latency, or measured device response.

---

## 7. Specifications

| Parameter | Value |
|---|---|
| Signal chain | Spectral, Gate, Beat, ADSR, Climax, Output |
| FFT | 2048-point, Hann, 1024 bins, 23.4 Hz @ 48 kHz |
| Frame rate | Windows PCM analysis uses a 1024-frame hop (~46.9 Hz at 48 kHz); Android processing targets ~60 Hz, UI ~30 Hz |
| Predictive onset lead | 50 ms at tempo confidence > 0.6 (recency + decay) |
| Output resolution | Lovense 0–20 integer |
| Command pacing | Desktop loop: at most 50 Hz; Android ordinary BLE writes: at least 28 ms apart (~36 Hz), stop writes: 12 ms |
| Desktop stack | Rust, eframe/egui 0.33.3, Buttplug 9.0.9 |
| Desktop package | `chloe-vibes` 1.6.1 |
| Android stack | `com.ashairfoil.chloevibes` 1.6.1 (versionCode 8) |
| Android SDK | Minimum API 26 (Android 8.0); target / compile API 35 |
| Stop behavior | Both clients: 2 s pipeline watchdog; desktop: panic-stop and stop-error feedback; Android: stop latch |
| License | MIT |

---

## 8. Installation and Operation

See the [root README](../README.md) for downloads. Windows runs as a standalone executable. The Android download is a debug APK for sideloading; grant the requested audio and nearby-device permissions. Android Visualizer requires `RECORD_AUDIO` permission even when System audio is selected.

### 8.1 Connect and choose a response

On Windows, start Intiface Central and use its server at `ws://127.0.0.1:12345`, then connect and enable a device in ChloeVibes. If no server is listening at the default address, ChloeVibes can start an embedded Buttplug server. A custom address can be supplied with `--server-addr`. On Android, scan for the device and wait for Ready before sending output.

Play audio, select **Bass Drum** or another preset, and adjust volume and output limits. Windows **FIND BOOM** fits the current track; **Revert** restores its earlier settings and **Keep** accepts the fit. Expand the full controls for frequency focus, gate, trigger shape, envelope timing, curves, and Climax settings. See Sections 2.8 and 4 for tuning behavior and preset scope.

Android source selection is explicit: choose **System audio** or **Microphone** before starting capture, and stop capture to change sources. System audio availability depends on the playback route and Android audio implementation.

Use **Stop all devices** to end output. Its software stop request and delivery limits are described in Sections 1 and 5.

### Direct envelope editing

Both clients show a large ADSR curve before the expert controls. Drag **A** horizontally for attack, **D** horizontally for decay and vertically for sustain, **S** vertically for sustain, and **R** horizontally for release. A gesture keeps its starting time scale so the curve stays under the pointer. A preset or slider change refits the view; Android also has **Fit view**, and Windows supports a double-click on empty graph space. The sustain segment illustrates a held level, not a timed hold.

Edits select Custom and update the same parameters as the detailed sliders. On Windows they also cancel an active FIND BOOM fit without reverting the edit. Output limits and gain remain separate. Windows connection and audio-status rows reserve their height, and the main content scrolls when needed; Android audio hints reserve two lines and long device labels truncate.

### 8.2 Audio status and timing

If Windows reports no playback, start audio and check the Windows playback device. A missing capture stream holds output at zero while the app attempts recovery. On Android, Waiting for audio means the selected source has not supplied a recent frame; it does not activate the other source.

Windows **Output delay** adds 0–500 ms to both motor channels when haptics arrive before audio. It cannot make late haptics arrive earlier. The beat predictor is a separate feature and does not measure or automatically calibrate Bluetooth latency.

### 8.3 Saved settings and diagnostics

Desktop settings use eframe persistence. Saved numeric settings are validated before use, including non-finite values, output ranges, delay, and device/actuator limits. A temporary FIND BOOM fit retains the pre-fit values for autosave until Keep or manual takeover.

Windows diagnostic files are stored in `%APPDATA%\chloe-vibes` (or the temporary directory if `APPDATA` is unavailable): `session.log` records startup, heartbeat, and clean exit; `session.prev.log` preserves the previous unclean session; `crash.log` records panic details and rotates to `crash.log.old` when large. These files describe process behavior, not measured device motion.

### 8.4 Run from source

```sh
cargo run --release
cargo run --release -- --server-addr ws://127.0.0.1:12345
cd android && ./gradlew assembleDebug
```

---

## 9. Limitations

- Stop commands depend on a working process and connection; they do not confirm that a motor physically stopped.
- No automatic comfort calibration; output limits are user-selected command limits.
- FIND BOOM is desktop-only and one-shot (press again to re-lock).
- Android dual-motor: Edge / Edge 2 only.
- The DSP golden covers synthetic PCM scenarios; live Visualizer capture, scheduling, Bluetooth timing, and physical output require separate checks.
- `rms_power` / `dominant_frequency` hardwired 0.0 in Rust.

---

## 10. Build, Test, and CI

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
cargo test --locked --all-targets
cargo test -p audio-capture --locked
cd android && ./gradlew testDebugUnitTest testReleaseUnitTest assembleDebug assembleRelease
```

The [main CI workflow](../.github/workflows/ci.yml) runs for pushes to `main` / `master`, pull requests targeting those branches, and manual dispatch. It checks Rust formatting, Clippy across all targets, the release build, and all-target tests including parity. Cargo builds and tests use the committed dependency lockfile. Android debug and release unit tests, debug APK assembly, and optimized release APK assembly run in a separate job. Windows also tests the vendored capture backend. The workflow has read-only repository-content permission and uploads build/test artifacts; it does not publish GitHub releases.

The [Android debug workflow](../.github/workflows/android-debug-apk.yml) is manually dispatched and produces test artifacts only. CI debug APKs may use a different signing certificate and are not automatically promoted to downloadable releases.

Published Android APKs use the project's preserved signing identity. For a given release, the standard debug download and its debug aliases receive identical APK bytes, keeping both contents and signing certificate consistent. The separately named `ChloeVibes-android-release.apk` is an optimized, non-debuggable build signed with that same identity. It can update the standard APK, but its bytes differ. The signing identity is a preserved Android debug certificate, not a Play Store signing identity. Use GitHub release assets for distribution; workflow artifacts are for testing.

---

## 11. Modification constraints

1. Signal-chain order is invariant on both clients.
2. Engines are one specification in two implementations — change both; parity enforces it.
