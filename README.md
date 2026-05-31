<p align="center">
  <img src="assets/logo.png" alt="ChloeVibes neon logo" width="920" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/MUSIC-MADE_PHYSICAL-ff2ea6?style=for-the-badge&labelColor=111118" alt="Music Made Physical" />
  <img src="https://img.shields.io/badge/ADSR-ENGINE-23f6ff?style=for-the-badge&labelColor=111118" alt="ADSR Engine" />
  <img src="https://img.shields.io/badge/CLIMAX-ENGINE-fff240?style=for-the-badge&labelColor=111118" alt="Climax Engine" />
  <img src="https://img.shields.io/badge/PARITY-LOCKED-72ff00?style=for-the-badge&labelColor=111118" alt="Parity Locked" />
  <img src="https://img.shields.io/badge/BLE-OUTPUT-ff6a00?style=for-the-badge&labelColor=111118" alt="BLE Output" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Windows-Rust_·_egui-23f6ff?style=for-the-badge&logo=windows&logoColor=23f6ff&labelColor=08060d" alt="Windows · Rust + egui" />
  <img src="https://img.shields.io/badge/Android-Kotlin_·_Compose-72ff00?style=for-the-badge&logo=android&logoColor=72ff00&labelColor=08060d" alt="Android · Kotlin + Compose" />
  <img src="https://img.shields.io/badge/Lovense-BLE-ff2ea6?style=for-the-badge&labelColor=08060d" alt="Lovense BLE" />
  <img src="https://img.shields.io/badge/Buttplug.io-9.0.9-fff240?style=for-the-badge&labelColor=08060d" alt="Buttplug.io 9.0.9" />
  <img src="https://img.shields.io/badge/Version-1.1.0-7f5cff?style=for-the-badge&labelColor=08060d" alt="Version 1.1.0" />
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-ff6a00?style=for-the-badge&labelColor=08060d" alt="MIT License" /></a>
</p>

<p align="center">
  <strong>Music, made physical.</strong><br>
  A spectral haptics engine that turns whatever you're listening to into sensation you can feel — in real time, beat for beat.
</p>

<p align="center">
  <em>system audio → FFT → gate → beat → ADSR → climax engine → haptic output</em>
</p>

---

## Feel first

Put on a track. Any track.

The bass drops and you feel it *arrive* — not a beat late, but on the kick, because the engine saw it coming and fired the attack roughly 76 ms early to land it exactly on time. A vocal swells and the sensation swells under it. A hi-hat ticks and you get a bright, weightless flicker up top. The room goes quiet and so does everything else — cleanly, no smear, no tail.

This is not a level meter wired to a motor. ChloeVibes listens the way a synthesizer would: it splits the spectrum into eight perceptual bands, tracks brightness and onset energy frame by frame, shapes every hit through a full ADSR envelope with velocity overshoot, and renders the result as motion. Bass holds. Treble taps off. The whole thing breathes.

And when you want it to stop being kind, there's the part that thinks in minutes instead of milliseconds.

> **The Climax Engine** lives at the end of the chain and quietly rewrites the rules underneath you. Across multi-minute *build → tease → surge* cycles it stacks chaos, sub-harmonic flutter, and a breathing-rate pulse so your nervous system never settles into a pattern it can tune out. It **edges**: drives you up, then pulls back to near-silence right as you crest — and each time, the denial runs a little deeper, holds a little longer, and triggers a little sooner. Then it lets go. It is off by default. You turn it on when you mean it.

<p align="center">
  <img src="assets/screenshot-windows.png" alt="ChloeVibes on Windows — glass-cockpit console with live spectrum, ADSR preview, and output scope" width="760" /><br>
  <em>Windows — the glass-cockpit console: live 8-band spectrum, ADSR envelope preview, energy/output scope, and a connected Lovense device.</em>
</p>

<p align="center">
  <img src="assets/screenshot-android.jpg" alt="ChloeVibes on Android — full signal-chain controls" width="280" />
  &nbsp;&nbsp;&nbsp;&nbsp;
  <img src="assets/screenshot-android-climax.jpg" alt="ChloeVibes on Android — Climax Engine with Wave / Stairs / Surge patterns" width="280" /><br>
  <em>Android — full signal-chain controls (left) and the Climax Engine with Wave / Stairs / Surge patterns (right).</em>
</p>

---

## What it is

ChloeVibes is an audio-reactive haptic engine that ships as **two clients wrapped around one shared signal engine**:

- **Windows desktop** — Rust + `eframe`/`egui`, capturing system audio over WASAPI loopback, driving devices through a [Buttplug.io](https://buttplug.io) client.
- **Android** — Kotlin + Jetpack Compose (Material3, dark), tapping the system-audio output mix and speaking Lovense BLE directly.

The Android engine is a **direct, parity-tested port** of the Rust engine (`src/audio.rs`). These are not two implementations that happen to agree — a CI-enforced golden dataset asserts, frame by frame, that they produce *identical* output. The two ports are not allowed to drift, and CI fails the moment they do.

> A note on register: this is adult haptic software. The copy stays composed on purpose. The engineering can do the talking.

---

## How it works — the signal chain

Every frame of audio walks the same fixed path on both platforms. It is never reordered, and no stage is ever skipped.

```
                              SIGNAL CHAIN  ·  ~60 Hz  ·  16 ms frames
  ┌──────────┐  ┌───────────┐  ┌────────┐  ┌───────────┐  ┌────────────────┐  ┌─────────────┐  ┌──────────┐
  │  System  │─▶│ Spectral  │─▶│  Noise │─▶│   Beat    │─▶│      ADSR      │─▶│   Climax    │─▶│  Output  │
  │  Audio   │  │ Analyzer  │  │  Gate  │  │ Detector  │  │ EnvelopeProc.  │  │   Engine    │  │   Map    │
  └──────────┘  └───────────┘  └────────┘  └───────────┘  └────────────────┘  └─────────────┘  └────┬─────┘
   WASAPI /      2048-pt FFT    hysteresis   adaptive       full ADSR +          6 anti-            │
   Visualizer    8 bands +      + auto-gate   flux onset     velocity            adaptation         ▼
   (+ mic        centroid +     (~25% open)   + tempo +      overshoot +         layers,        Lovense / Buttplug
    fallback)    flux                         ~76 ms lead    freq shaping        edge & deny    (intensity 0–20)
                                                                                 (off by default)
```

**1 · Spectral Analyzer.** A 2048-point, Hann-windowed FFT — `2/N` magnitude normalization, 1024 usable bins, ~23.4 Hz per bin at 48 kHz. The spectrum is divided into **8 perceptual bands** spanning 20 Hz–20 kHz — Sub / Bass / Lo-Mid / Mid / Hi-Mid / Pres / Brill / Air — on edges that are byte-identical across platforms (`[20, 60, 250, 500, 2000, 4000, 6000, 12000, 20000]`). From there it derives **per-band RMS energy**, **spectral centroid** (brightness; the DC bin is skipped so silence doesn't read as dark), and **half-wave-rectified spectral flux** (onset energy — it counts the spectrum getting *louder*, not quieter). Rust uses `rustfft`; Android uses a hand-written radix-2 Cooley-Tukey FFT for the microphone path and Android's Visualizer FFT for the system-audio path. Both normalize identically.

**2 · Noise Gate.** A hysteresis gate with *proportional* hysteresis (the open/close gap scales with the threshold) and asymmetric smoothing: it opens instantly and closes gently, so transients punch through while silence settles without chatter. An optional histogram-based **auto-gate** continuously retargets the threshold to keep the gate open roughly a quarter of the time, whatever the ambient floor.

**3 · Beat Detector.** Adaptive-threshold onset detection on spectral flux (mean + *k*·σ over a 43-frame window), with tempo tracking across the last 16 onsets and a 55 ms cooldown (a ~270 BPM 16th-note ceiling). It publishes a *predicted next onset*. Downstream, once tempo confidence clears 0.6, the pipeline pre-fires roughly **76 ms early** — so the attack lands *on* the beat instead of chasing it.

**4 · ADSR EnvelopeProcessor.** A full Attack–Decay–Sustain–Release envelope with independent power-curve exponents per stage, **velocity overshoot up to 120%** on hard transients, and centroid-driven *frequency-dependent shaping*: bass sustains louder and releases up to 40% longer; treble holds 25% less and taps off. Through the quiet of a sustain it runs a **five-layer modulation on irrational frequency ratios** (~0.17–2.7 Hz, chosen so the combined waveform never exactly repeats) plus deterministic **micro-pauses** — true-zero drops of 3–6 frames (~48–96 ms) every 2–8 seconds, so nerve endings reset and stay sensitive. Minimum retrigger interval is 20 ms, matched to motor spin-up.

**5 · Climax Engine.** The final stage — off by default. See below.

**6 · Output Map.** A single parity-locked stage (`map_output` in Rust, `mapOutput` in Kotlin): below threshold it returns silence; above it, the shaped envelope is mapped into the device's `[min, max]` range, gained, and clamped. Both platforms add asymmetric output slew (default 85 ms; ~30 ms on the rise) so motion reads punchy upward and natural on the way down. **This is the exact stage the cross-platform golden verifies, value for value.**

---

## The Climax Engine

<p align="center"><em>A slow time-domain modulation layer engineered to defeat neural adaptation — so the sensation never goes numb.</em></p>

The Climax Engine is the last thing in the chain. Where the ADSR shapes each individual hit, this reshapes the *whole arc* over multi-minute **build → tease → surge** cycles. The premise is simple, and a little ruthless: a nervous system filters out anything steady, so nothing here is ever allowed to be steady.

It is **off by default in every preset.** It runs only when you opt in — through an experience preset, a one-click profile, or by hand.

<details>
<summary><strong>How it builds — cycle structure</strong></summary>

<br>

Each cycle runs **8–240 seconds** (default **90 s**, identical on both engines), ramping along one of three shapes:

- **Wave** — a smooth S-curve swell.
- **Stairs** — a quantized, stepped climb.
- **Surge** — a fast-rising power curve that front-loads the build.

In the closing fraction of every cycle it either **teases** — a sharp cliff down, then a slow rebuild over the tail — or **surges** to peak on an accelerating curve. The first cycles are gentle. As the session *matures* across its first six completed cycles, the teases bite harder and the surges climb higher.

</details>

<details>
<summary><strong>Six anti-adaptation layers</strong></summary>

<br>

Over the macro cycle, six modulators keep the signal aperiodic so the body can't learn it:

| Layer | What it does | Range |
|---|---|---|
| **5-oscillator micro-pulse** | Five detuned sines (detunes 0.07 & 0.13) summed into a shimmering pulse the ear can't average out | up to 7 Hz, 10 Hz during surge |
| **Sub-harmonic flutter** | Low-frequency resonance that deepens as the cycle climbs | 8% → 24% depth |
| **Lorenz-attractor chaos** | A genuine chaotic oscillator (σ=10, ρ=28, β=8⁄3) — deterministic, yet never repeating | 6% → 18% depth |
| **Breathing-rate modulation** | A slow ~0.18 Hz pulse tuned to the rhythm of arousal breathing | 6% → 16% depth |
| **Stochastic micro-pauses** | True-zero drops of ~48–96 ms, every 2–8 s *(in the ADSR stage)* | 3–6 frames |
| **5-layer sustain modulation** | Irrational-ratio shimmer so a sustain is never flat *(in the ADSR stage)* | ~0.17–2.7 Hz |

The micro-pauses and sustain modulation physically live in the **EnvelopeProcessor**, one stage upstream — same anti-adaptation philosophy, applied a little earlier in the chain.

</details>

<details>
<summary><strong>Arousal momentum &amp; the edge-and-deny state machine</strong></summary>

<br>

**Arousal momentum** accumulates across completed cycles (+0.12 each, hard-capped at 0.75) and feeds back into gain — lifting peak output up to **~3.8×** at full ramp. It only relaxes during silence, so an active session *ratchets* upward and won't settle until the music stops.

The **edge-and-deny state machine** watches for sustained high output and, once you've held there long enough, triggers a **deny** — collapsing output toward zero, then releasing with a deliberate overshoot. As the session matures, the cruelty escalates on every axis:

| | Early | Mature |
|---|---|---|
| **Deny depth** | 60% reduction | 90% reduction |
| **Deny duration** | ~0.6 s | ~2.4 s |
| **Time to trigger** | 6 s of high output | 3 s |
| **Post-deny overshoot** | +0.30 | +0.55 *(cap 0.65)* |

On capable hardware it also drives a **second motor** in dynamic unison/anti-phase — the motors move together at low intensity and pull apart as output climbs, so the sensation travels.

</details>

<details>
<summary><strong>One-click profiles (desktop)</strong></summary>

<br>

The desktop console ships three one-click Climax profiles beside the manual sliders:

| Profile | Pattern | Intensity | Build-up | Character |
|---|---|---|---|---|
| **Edge** | Wave | 0.62 | 130 s | Long, patient edging |
| **Overload** | Surge | 0.88 | 75 s | Aggressive, fast escalation |
| **Punisher** | Stairs | 1.0 | 55 s | No restraint, maximum chaos |

</details>

---

## Presets

A curated factory library across five categories — **INIT · DRUMS · MUSICAL · BASS · FX** — covering everything from a transparent loudness follower to full experience cycles. Each preset is a complete snapshot of every signal parameter, like a synth patch: pick one, then tune.

| Platform | Factory presets | Climax-enabled |
|---|---|---|
| **Windows (desktop)** | **29** | 3 — *Slow Tease · Ride the Beat · Break Me* |
| **Android** | **32** | 5 — the above + *Chloe Medium · Chloe Ultimate* |

Desktop breakdown: INIT 2 · DRUMS 5 · MUSICAL 6 · BASS 6 · FX 10. Android ships the same 29 and folds in the three **Chloe** rhythm profiles (Loose / Medium / Ultimate) as Bass-category catalog presets; on desktop those same three are available as one-click rhythm-profile buttons rather than catalog entries — which is the whole of the 29-vs-32 difference.

**A few worth knowing:**

- **Ride Intensity** *(INIT)* — neutral loudness follower; the launch default.
- **Hi-Hat Tingle** *(FX)* — high-pass only; sparkly, delicate, treble-reactive. *(On both platforms.)*
- **Slow Tease** *(experience)* — a 120 s edging cycle that parks you on the brink.
- **Ride the Beat** *(experience)* — a 60 s music-locked escalation that syncs chaos and sub-harmonic resonance to the rhythm.
- **Break Me** *(experience)* — the unkind one: a 45 s build to maximum intensity, deepest pulse, dual-motor anti-phase.

> **Open parity gap (tracked, not hidden):** the `threshold_knee` and `dynamic_curve` fields exist in Android's `Preset` struct but not yet in the Rust catalog's, and the Chloe profiles are catalog presets on Android while remaining one-click buttons on desktop. Bringing the Rust catalog to full parity is on the list.

---

## Get started

### Windows

ChloeVibes desktop is a standards-based [Buttplug.io](https://buttplug.io) **client**.

1. **Run a Buttplug server.** By default the app connects to an external server at `ws://127.0.0.1:12345`; the easy path is [Intiface Central](https://intiface.com/central/). *(If none is reachable, it falls back to an embedded in-process server with seven communication managers — btleplug BLE, websocket-server, serialport, Lovense Connect, Lovense HID dongle, Lovense serial dongle, and XInput.)*
2. **Build and run:**
   ```sh
   cargo run --release
   ```
   Or point it at a specific server:
   ```sh
   cargo run --release -- --server-addr ws://127.0.0.1:12345
   ```
3. **Connect, scan, pick a preset, start the music.** The 8-band spectrum, ADSR preview, and output scope come alive as audio flows. Settings persist between sessions; closing the window stops every device.

### Android

1. **Install the APK** from a tagged GitHub Release, or build one:
   ```sh
   cd android && ./gradlew assembleDebug
   ```
2. **Grant permissions.** ChloeVibes taps your phone's **system-audio output mix** through Android's Visualizer API — which still requires `RECORD_AUDIO`, even though it reads the output mix, not the microphone. Bluetooth scan/connect permissions are requested as needed. If the Visualizer stalls or goes silent, the app falls back to the microphone automatically.
3. **Scan, connect, pick a preset, play.**

Requires **Android 8.0+ (API 26)**; targets API 35.

---

## Supported devices

ChloeVibes drives **Lovense BLE** toys, with honest differences between platforms:

| Capability | Windows (desktop) | Android |
|---|---|---|
| **Transport** | Buttplug 9.0.9 client → Intiface (default), embedded-server fallback | Direct Android BLE GATT over Nordic UART Service |
| **Device reach** | Anything your Buttplug/Intiface server supports | Lovense toys reachable over BLE |
| **Single-motor** | ✅ | ✅ |
| **Dual-motor** | ✅ Any multi-actuator scalar device the server reports (driven per-actuator by index) | ⚠️ **Edge / Edge 2 only** — see below |

**About Android dual-motor — read this.** Independent `Vibrate1` / `Vibrate2` control is currently reliable **only on the Lovense Edge / Edge 2** (DeviceType code `P`, fixture-verified). Every other toy — including the **Domi 2**, this project's own primary test device — is driven as a single motor *by design*. Detection matches the device's reported `DeviceType` code against a deliberate one-entry whitelist; an unknown code stays single-motor, because guessing "dual" wrong can silence a single-motor toy entirely. This is a conservative choice, not an oversight.

> **Lovense protocol.** ASCII commands terminated with `;` over the Nordic UART Service. Intensity is an **integer 0–20 (21 discrete levels)** — not 0–100, not 0–255. Single motor: `Vibrate:N;`. Dual motor: `Vibrate1:X;Vibrate2:Y;`.

**Not yet implemented — know before you rely on it.** There is **no auto-reconnect** after a BLE/RF dropout, and **no automatic safety/panic-stop.** The only stop controls are the manual *"Stop all devices"* button and the desktop's stop-on-close. Both are on the roadmap.

---

## Architecture &amp; parity

One engine, two clients, provably in sync. The Kotlin engine is a direct port of the Rust engine, and a **CI-enforced parity golden** asserts they match frame for frame.

| Stage | Windows (Rust) | Android (Kotlin) | In sync |
|---|---|---|---|
| Spectral / FFT | `rustfft`, 2048-pt | radix-2 Cooley-Tukey + Visualizer FFT — same size, window, normalization | ✅ |
| Gate | Proportional hysteresis, ~25% auto-gate | `Gate.kt` — ported | ✅ |
| Beat detector | Adaptive flux onset + tempo + predicted onset | Ported; ~76 ms pre-fire at conf > 0.6 | ✅ |
| ADSR | Full ADSR + overshoot + freq shaping + 5-layer sustain + micro-pauses | Ported `EnvelopeProcessor` | ✅ |
| Climax engine | 8–240 s cycles, 6 layers, edge-and-deny | `ClimaxEngine.kt` — same range and patterns | ✅ |
| **Output stage** | `map_output` + asymmetric slew | `mapOutput` + configurable slew | ✅ *(parity-tested)* |
| Audio capture | WASAPI loopback (dedicated OS thread) | Visualizer system-audio + mic fallback | ⚠️ different APIs, same goal |
| Preset catalog | 29 presets, 5 categories | 32 presets (+3 Chloe, +2 fields) | ⚠️ tracked gap |
| Dual-motor | General, per-actuator index | Edge / Edge 2 whitelist | ⚠️ not equivalent |
| Device transport | Buttplug / Intiface | Raw Lovense BLE | ⚠️ different stacks |

**The parity contract.** A Rust harness (`tests/parity.rs`) runs the full chain over deterministic synthetic PCM and writes a golden CSV — **6 scenarios × 468 frames = 2,808 rows.** Kotlin's `ParityTest.kt` regenerates the same PCM, runs the ported chain, and asserts every output column matches within epsilon (1e-4 on the Rust side, 1e-3 on Kotlin to absorb floating-point op-ordering). **CI fails on any drift.** The six scenarios cover all three trigger modes (Dynamic / Binary / Hybrid) and all three climax patterns (Wave / Stairs / Surge), plus the high-sustain clamp region. Parity here isn't a nice-to-have; it's the contract that lets one engine live two lives.

---

## Tech details

| | |
|---|---|
| **Engine** | One shared signal engine; fixed chain Spectral → Gate → Beat → ADSR → Climax → Output |
| **FFT** | 2048-point, Hann-windowed; ~23.4 Hz/bin at 48 kHz; 1024 usable bins |
| **Bands** | 8 perceptual (Sub / Bass / Lo-Mid / Mid / Hi-Mid / Pres / Brill / Air), 20 Hz–20 kHz |
| **Processing rate** | ~60 Hz (16 ms frame budget); Android UI ~30 Hz |
| **Climax cycle** | 8–240 s (default 90 s); peak arousal gain up to ~3.8× |
| **Output** | Lovense intensity 0–20 integer (21 levels) |
| **BLE command rate** | Desktop 50 Hz (20 ms, 0.5% dead-band); Android ~33 Hz (30 ms min interval) |
| **Desktop** | Rust, `eframe`/`egui` 0.33.3, Buttplug 9.0.9, `rustfft`, WASAPI loopback; glass-cockpit (EFIS/audio-console) GUI |
| **Android** | Kotlin, Jetpack Compose (Material3 dark), package `com.ashairfoil.chloevibes`, **v1.1.0** (versionCode 2), minSdk 26 / target 35, Java/Kotlin 17 |
| **Android BLE** | Unfiltered LOW_LATENCY scan (15 s), TRANSPORT_LE, HIGH priority, 185-byte MTU, then `discoverServices()` from `onMtuChanged` — the ordering fix that resolved GATT-status-19 dropouts. Verified on Galaxy S23 Ultra + Edge 2. |
| **Testing** | 61 Rust tests (cargo runner count; 36 distinct `#[test]` functions) + clippy `-D warnings` + `fmt --check`; Kotlin unit + parity tests |
| **CI/CD** | GitHub Actions — Rust on windows-latest (fmt, clippy, build, test, dedicated parity test) + Android on ubuntu-latest (Kotlin tests, `assembleDebug`, APK artifact, Release on `v*` tags); plus a rolling debug-APK build on every `master` push |

> **Version note.** The shipping product version is **1.1.0** — the value carried by both the `VERSION` file and the Android build. The Rust desktop crate's internal `Cargo.toml` currently lags at `0.5.0` and is slated to be bumped to match; a known internal discrepancy, not a separate release.

---

## Contributing

Contributions are welcome. A few ground rules keep the project honest:

1. **The signal-chain order is invariant.** Spectral → Gate → Beat → ADSR → Climax → Output — never reordered, never skipped, on either platform.
2. **Parity is a contract.** Change one engine, change the other to match. The golden will catch you in CI if you don't — which is exactly its job.
3. **CI stays green.** Rust: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, and the dedicated parity test. Android: `./gradlew testDebugUnitTest` before the APK build.
4. **Stay honest.** A feature that works on one platform is documented as platform-specific. The open gaps — the Chloe-preset catalog split, the Android dual-motor scope, the `rms_power` / `dominant_frequency` fields hardwired to `0.0` in Rust — are tracked, not papered over.

```sh
# Desktop
cargo test
cargo test --test parity

# Android
cd android && ./gradlew testDebugUnitTest
```

---

## License

**MIT** — Copyright © 2024 coldbricks. See [LICENSE](LICENSE).

<p align="center">
  <sub>Music, made physical.</sub>
</p>
