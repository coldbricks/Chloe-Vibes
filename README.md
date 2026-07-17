# ChloeVibes

### Audio-reactive haptics for **Windows** and **Android**

**System audio → calibrated force** in real time. One signal engine, two clients, parity-locked in CI — not a slider toy bolted onto a BLE chat.

| | |
|--|--|
| **Version** | [**v1.5.0**](https://github.com/coldbricks/Chloe-Vibes/releases/latest) |
| **Clients** | Windows (Rust · egui) · Android (Kotlin · Compose) |
| **Devices** | Lovense BLE · Buttplug / Intiface (desktop) |
| **Sister app** | [**ChloeVR**](https://github.com/coldbricks/ChloeVR) — XR cinema + stage (shares this engine’s beat DNA) |
| **Author** | [Ash Airfoil](https://github.com/coldbricks) |

[![Release](https://img.shields.io/github/v/release/coldbricks/Chloe-Vibes?style=flat-square)](https://github.com/coldbricks/Chloe-Vibes/releases)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/Windows%20%7C%20Android-haptics-informational?style=flat-square)](https://github.com/coldbricks/Chloe-Vibes/releases)

<p align="center">
  <img src="assets/logo.png" alt="ChloeVibes wordmark" width="720" />
</p>

---

## What it does

```
┌──────────────────────┐     ┌──────────────────────┐
│     WINDOWS          │     │       ANDROID        │
│  WASAPI loopback     │     │  Visualizer / mic     │
│  Buttplug · Intiface │     │  Direct Lovense BLE   │
│  FIND BOOM tuner     │     │  Same engine (port)   │
└──────────┬───────────┘     └──────────┬───────────┘
           │                            │
           └──────────┬─────────────────┘
                      v
              fixed signal chain
         FFT → gate → beat → ADSR → out
                      v
                 device 0…20
```

Play music. ChloeVibes listens to the system mix, finds the punch (kick, hat, pad…), and drives a haptic device with a **musical envelope** — not a raw loudness follower unless you ask for one.

**Default path = Bass Drum boom:** instant peak, long musical decay, near-zero floor. Press **FIND BOOM** once to auto-tune band / gate / trigger / envelope to whatever is playing.

> **Safety.** This commands a physical device on a body. Desktop has a dead-man watchdog, panic-stop, and verified stop-all. Android does not yet have a watchdog. You own the ceiling. Red **Stop all devices** is always the right move.

---

## Screenshots (app UI only)

Docs show the **console UI** only — no device photography, no adult media. The product is adult haptic software; the surfaces below are pro-audio style controls.

<p align="center">
  <img src="assets/demo.gif" alt="Desktop console meters tracking live audio" width="820" /><br>
  <em>Desktop console — output level and envelope tracking live system audio (Advanced FFT + ADSR).</em>
</p>

<p align="center">
  <img src="assets/screenshot-windows.png" alt="Windows desktop console" width="820" /><br>
  <em>Windows — transport, gate, spectrum, envelope, presets, device panel.</em>
</p>

<p align="center">
  <img src="assets/screenshot-android.jpg" alt="Android signal chain" width="280" />
  &nbsp;&nbsp;&nbsp;&nbsp;
  <img src="assets/screenshot-android-climax.jpg" alt="Android Climax controls" width="280" /><br>
  <em>Android — signal chain (left) and optional Climax engine (right).</em>
</p>

---

## Install (prebuilt)

1. Open the [**latest release**](https://github.com/coldbricks/Chloe-Vibes/releases/latest)
2. Download:
   - **Windows:** `ChloeVibes-windows-x64.exe`
   - **Android:** `ChloeVibes-android.apk` (debug, sideload)
3. **Windows:** run the exe. Prefer [Intiface Central](https://intiface.com/central/) on `ws://127.0.0.1:12345`; if nothing is listening, the app starts an embedded Buttplug server.
4. **Android:** sideload the APK, grant mic / nearby devices as prompted (Visualizer needs `RECORD_AUDIO` even for system audio).

Stable rolling links (same binaries as the versioned Latest when kept in sync):

| Platform | URL |
|----------|-----|
| Windows | `https://github.com/coldbricks/Chloe-Vibes/releases/latest/download/ChloeVibes-windows-x64.exe` |
| Android | `https://github.com/coldbricks/Chloe-Vibes/releases/latest/download/ChloeVibes-android.apk` |

---

## Quick start

| Step | What to do |
|------|------------|
| 1 | Connect the device (scan → enable) |
| 2 | Play a track with a clear kick |
| 3 | Press **FIND BOOM** (desktop) or pick **Bass Drum** / a BOOM tempo macro |
| 4 | Raise volume / multiplier only as far as you mean to |
| 5 | **Stop all devices** when done |

Expert knobs (trigger shape, slew, curves) live under **OVERRIDE** — the main surface is presets + FIND BOOM.

---

## What’s new in v1.5.0

| Area | Change |
|------|--------|
| **FIND BOOM** | Max-dynamic bass-drum tuner (UI name for AUTO-LOCK) |
| **Bass Drum default** | Kick-only natural BOOM path |
| **Domi hang fix** | Pluck/boom release to true zero; device rest floors; motors actually stop |
| **UI declutter** | Expert controls collapsed; auto-scan on connect |
| **Crash hardening** | WASAPI capture panic recovery; session heartbeat log |

---

## Signal chain (invariant)

Order never changes on either client:

```
SYSTEM AUDIO
    → Spectral (2048-pt FFT, 8 bands)
    → Gate (hysteresis · optional auto)
    → Beat (onset · tempo · 76 ms pre-fire)
    → ADSR (A/D/S/R · curves · overshoot)
    → Climax (optional · off by default)
    → Output map (0…20 · asymmetric slew)
    → BLE / Buttplug
```

Desktop can switch to **Original Chloe Vibes (RMS)** loudness mode. Android always runs the advanced chain.

Deep dive (parity, protocols, limitations, build): [`docs/TECHNICAL_REFERENCE.md`](docs/TECHNICAL_REFERENCE.md)  
Design notes: [`docs/AUTO_LOCK_DESIGN.md`](docs/AUTO_LOCK_DESIGN.md) · [`docs/TEMPORAL_ARCHITECTURE.md`](docs/TEMPORAL_ARCHITECTURE.md)

---

## Device support

| | Windows | Android |
|--|---------|---------|
| Transport | Buttplug 9 → Intiface (or embedded) | Direct Lovense BLE (NUS) |
| Reach | Anything the server exposes | Lovense over BLE |
| Dual-motor | Per actuator index | **Edge / Edge 2 only** (fixture-verified) |

---

## Build from source

```bash
# Desktop
cargo build --release
cargo test
cargo test --test parity

# Android
cd android && ./gradlew assembleDebug testDebugUnitTest
```

| | |
|--|--|
| **Desktop package** | `chloe-vibes` (Cargo) |
| **Android applicationId** | `com.ashairfoil.chloevibes` |
| **Android SDK** | min 26 · target / compile 35 |
| **Version** | 1.5.0 · versionCode 5 |

CI: `fmt` · `clippy -D warnings` · release build · Rust + Kotlin parity golden.

---

## Architecture

```
Windows                         Android
───────                         ───────
eframe / egui console           Jetpack Compose UI
WASAPI capture thread           Visualizer / mic
src/audio.rs  ──────── parity ─→ Kotlin engine port
src/auto_lock.rs (FIND BOOM)    (presets mirrored)
Buttplug client                 BleDeviceManager
```

---

## Releases

| Version | Notes |
|---------|--------|
| **[v1.5.0](https://github.com/coldbricks/Chloe-Vibes/releases/tag/v1.5.0)** | FIND BOOM · Domi hang · UI declutter · crash hardening |
| [v1.4.0](https://github.com/coldbricks/Chloe-Vibes/releases/tag/v1.4.0) | AUTO-LOCK beat lock + punch |
| [v1.3.0](https://github.com/coldbricks/Chloe-Vibes/releases/tag/v1.3.0) | Reliability · safety · first AUTO-LOCK |

Always prefer the [**latest release**](https://github.com/coldbricks/Chloe-Vibes/releases/latest).

---

## Status

Actively developed on real hardware (Domi / Edge). Desktop is the FIND BOOM + safety reference; Android tracks the same engine via parity CI.

---

## Credits

**Ash Airfoil** — product, signal taste, hardware QA  
Stack: **Rust** · **egui** · **Buttplug** · **Kotlin Compose** · **Lovense BLE**

---

## Privacy & content

ChloeVibes is a **local control surface**. It does not host media or device catalogs.  
You provide the audio and the hardware. Adult-oriented product; repository screenshots are UI-only.
