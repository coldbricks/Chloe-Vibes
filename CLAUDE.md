# CRITICAL PROJECT DIRECTIVES

These rules are non-negotiable. Violations require stopping and asking the user.

- NEVER commit, push, deploy, or run destructive commands without explicit user approval
- NEVER create files unless the task strictly requires it — prefer editing existing files
- NEVER guess at architecture — read the code first, form a model, then act
- When an error occurs, read the FULL message. Trace the cause. Do not blame the platform or say "can't be done" without exhaustive investigation

# Project Identity

- **Name:** ChloeVibes
- **Type:** Audio-reactive haptic controller — Android app (Kotlin/Compose) + Windows desktop (Rust/egui)
- **Language/Stack:** Kotlin + Jetpack Compose (Android), Rust + eframe/egui (desktop)
- **Build System:** Gradle 9.0.0 / Kotlin 2.1.0 (Android), Cargo (Rust desktop)
- **Target Platform:** Android 8.0+ (API 26) targeting API 35; Windows x86_64 (Rust desktop)
- **Repo Root:** (use current working directory)
- **Primary Branch:** master

# Key File Map

Consult these before searching blindly. Paths are relative to repo root.

| Purpose | Path |
|---|---|
| Android entry point | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/MainActivity.kt` |
| Audio capture | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/audio/AudioCaptureManager.kt` |
| Spectral analysis | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/audio/SpectralAnalyzer.kt` |
| Noise gate | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/audio/Gate.kt` |
| Beat detection | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/audio/BeatDetector.kt` |
| ADSR envelope | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/audio/EnvelopeProcessor.kt` |
| Climax modulation | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/audio/ClimaxEngine.kt` |
| Presets | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/audio/Presets.kt` |
| BLE device manager | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/device/BleDeviceManager.kt` |
| Lovense protocol | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/device/LovenseProtocol.kt` |
| Main UI screen | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/ui/MainScreen.kt` |
| Theme | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/ui/Theme.kt` |
| Android manifest | `android/app/src/main/AndroidManifest.xml` |
| App build config | `android/app/build.gradle.kts` |
| Root build config | `android/build.gradle.kts` |
| Rust entry point | `src/main.rs` |
| Rust signal engine | `src/audio.rs` |
| Rust GUI + pipeline | `src/gui.rs` |
| Rust presets | `src/presets.rs` |
| Rust settings | `src/settings.rs` |
| Rust utilities | `src/util.rs` |
| FIND BOOM supervisor (desktop) | `src/auto_lock.rs` |
| FIND BOOM supervisor (Android) | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/audio/AutoLockSupervisor.kt` |
| Desktop device dispatch + stop schedule | `src/device_dispatch.rs` (the per-device task loop lives in `src/gui.rs`) |
| Desktop timing test + groove loops | `src/timing_test.rs` |
| Tap tempo | `src/tap_tempo.rs`, `android/app/src/main/kotlin/com/ashairfoil/chloevibes/audio/TapTempo.kt` |
| ADSR drag editor | `src/adsr_editor.rs`, `android/app/src/main/kotlin/com/ashairfoil/chloevibes/ui/EnvelopeEditor.kt` |
| Android app state, dead-man watchdog, companion sessions | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/ChloeVibesApplication.kt` |
| Android BLE write queue | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/device/BleCommandQueue.kt` |
| Finally companion bridge (Android) | `android/app/src/main/kotlin/com/ashairfoil/chloevibes/FinallyHapticBridgeService.kt` |
| Rust/Kotlin parity | `tests/parity.rs`, `tests/parity_golden.csv`, `android/app/src/test/kotlin/com/ashairfoil/chloevibes/audio/ParityTest.kt` |
| Reference + design docs | `docs/TECHNICAL_REFERENCE.md`, `docs/TEMPORAL_ARCHITECTURE.md`, `docs/AUTO_LOCK_DESIGN.md`, `docs/WINDOWS_AUDIO.md`, `docs/waves/` |
| Rust build config | `Cargo.toml` |
| Tests | `cargo test --locked --all-targets` (unit + parity golden), `cd android && ./gradlew testDebugUnitTest`, plus real-hardware feel checks |
| CI/CD | GitHub Actions (.github/workflows/ci.yml) — Rust build/test/lint + Android build |
| Generated (DO NOT EDIT) | `android/build/`, `android/app/build/`, `android/.gradle/`, `target/` |

# Build and Run

```
# Android — build debug APK
cd android && ./gradlew assembleDebug

# Android — install to connected device
adb install -r android/app/build/outputs/apk/debug/app-debug.apk

# Android — clean
cd android && ./gradlew clean

# Rust desktop — build
cargo build --release

# Rust desktop — run
cargo run --release
```

Always run the build after making changes. If the build fails, fix it before moving on. Never hand the user a broken build.

# Architecture Invariants

These are load-bearing design decisions. Do not refactor away from them without explicit approval.

1. Signal chain order is fixed: SpectralAnalyzer -> Gate -> BeatDetector -> EnvelopeProcessor -> ClimaxEngine -> Output. Never reorder or skip stages.
2. Audio processing runs on a dedicated thread at ~60Hz. UI reads state via volatile fields. Never do signal processing on the main/UI thread. (Android follows this. Desktop as built: the capture thread runs the FFT on a fixed 1024-frame hop, ~47 Hz; Gate through output run inside egui `update()`, with gate/beat ticking once per fresh spectral frame. The 2 s dead-man watchdog stops devices if `update()` stalls.)
3. The Kotlin signal engine is a direct port of the Rust engine (`src/audio.rs`). When modifying one, consider whether the other needs the same change to stay in sync.
4. BLE commands go through LovenseProtocol for command formatting and BleDeviceManager for transmission. Never write raw BLE commands outside this path.
5. All Lovense commands are ASCII strings terminated with semicolons, sent via Nordic UART Service (NUS). The intensity range is 0-20, not 0-100 or 0-255.
6. Presets are immutable snapshots of all signal processing parameters. When adding a parameter, it must be included in the Preset data class and all existing presets updated.

# Coding Standards

## Style Rules
- 4-space indentation, no tabs (Kotlin). Standard rustfmt conventions (Rust).
- Preserve existing formatting in files you edit — match the style already present.
- No wildcard imports in Kotlin.
- No trailing summaries or recap comments.

## Naming Conventions
- Files: PascalCase.kt (Kotlin), snake_case.rs (Rust)
- Variables: camelCase (Kotlin), snake_case (Rust), constants: SCREAMING_SNAKE (both)
- Boolean vars prefixed: is, has, should, can
- Signal processing parameters match across Kotlin and Rust (e.g., `attackMs`, `gateThreshold`)

## Patterns to Use
- Volatile fields for cross-thread state sharing (Android). Arc<Mutex<>> or SharedF32 for Rust.
- Sealed classes/enums for state machines (EnvelopeState, ConnectionState, TriggerMode, FrequencyMode)
- Data classes for parameter bundles (SpectralData, Preset)

## Patterns to Avoid
- No coroutines in the signal processing path — raw threads with sleep loops for deterministic timing
- No GlobalScope
- No BLE writes faster than the device can handle — throttle commands to prevent command spam
- No blocking calls on the Android main thread

# Workflow Directives

## Before Writing Code
1. Read the relevant source files. Use Glob to find them, Read to understand them. Do not guess structure.
2. Trace the data flow from audio input through the signal chain to BLE output.
3. Identify every file that will need changes before making the first edit.
4. If more than 5 files need changes, state the plan and wait for approval.

## While Writing Code
- Make the smallest correct change. Do not refactor adjacent code unless asked.
- Preserve existing formatting in files you edit — match the style already present.
- When using Edit, provide enough context in `old_string` to be unambiguous. If the match is not unique, widen the context.
- After editing, re-read the changed region to verify correctness.

## After Writing Code
- Run the build command. If it fails, fix immediately.
- Use `git diff` to review all changes before reporting completion.

## Debugging Protocol
1. Reproduce the issue — find the exact error message or behavior.
2. Read the FULL error output. Every line. Stack traces, logcat, everything.
3. Form a hypothesis about the root cause.
4. Verify the hypothesis by reading the relevant code path.
5. Fix the root cause, not the symptom.
6. Confirm the fix resolves the issue without regressions.

# Quality Gates

These checks must pass before reporting a task as complete.

- [ ] Code compiles / builds without errors
- [ ] No new warnings introduced
- [ ] No hardcoded secrets, paths, or credentials in committed code
- [ ] `git diff` reviewed — no accidental changes, debug prints, or commented-out code
- [ ] If UI was changed — describe the visual change so the user can verify on device
- [ ] If signal processing was changed — describe the expected audible/haptic behavior difference

# Tool Usage Refinements

## Bash
- Use project-specific commands from the "Build and Run" section above. Do not invent build commands.
- For long-running commands (Gradle builds), use `run_in_background` and check output when complete.
- Set JAVA_HOME appropriately for your platform when running Gradle commands.

## Grep and Glob
- When searching this project, start with the Key File Map above before doing broad searches.
- Exclude `android/build/`, `android/app/build/`, `android/.gradle/`, `target/` from searches.
- For Kotlin files use glob `**/*.kt`. For Rust files use glob `**/*.rs`.

## Read
- Config files (`build.gradle.kts`, `Cargo.toml`, `AndroidManifest.xml`) are short — read them fully.
- Source files regularly exceed 500 lines (MainScreen.kt ~1830, AudioCaptureManager.kt ~1120, BleDeviceManager.kt ~1070, gui.rs ~4350, audio.rs ~3240) — use offset/limit.

## Agent
- Use for multi-step investigations that require exploring unknown parts of the codebase.
- Do NOT use for tasks where the file locations are already known.

# Environment-Specific Notes

- Development on Windows 11 (also previously on Kali Linux)
- Device connected via ADB is a Samsung Galaxy S23 Ultra — Visualizer API behavior and BLE timing may differ from emulator/other devices
- Android builds also run on Meta Quest (v1.7.0 added silent-capture recovery and a landscape window)
- Lovense Domi 2 (single motor) is the primary test device for haptic output
- No emulator — all testing is on real hardware
- Gradle wrapper is in `android/` subdirectory, not repo root

# Domain-Specific Knowledge

- **Visualizer API** taps system audio output (not mic input) — requires `RECORD_AUDIO` permission and an active audio session ID. Returns FFT magnitude data, not raw PCM.
- **Lovense intensity** is 0-20 (integer), mapped to hardware PWM. The `Vibrate:N;` command sets single-motor intensity. `Vibrate1:X;` and `Vibrate2:Y;` control dual motors independently. Android sends dual commands only to DeviceType `P` (Edge / Edge 2); every other model, including the single-motor Domi 2, gets `Vibrate:N;`.
- **Nordic UART Service (NUS)** is the BLE GATT service Lovense devices use. TX characteristic UUID: `6e400002-...`, RX: `6e400003-...`. Commands are ASCII with `;` terminator.
- **Spectral flux** is the frame-to-frame change in FFT magnitude — used for onset/beat detection. High flux = transient (drum hit, note attack).
- **ADSR envelope** shapes the haptic response to each beat: Attack (ramp up), Decay (pull back), Sustain (hold), Release (fade out). Curve exponents control the shape of each stage.
- **ClimaxEngine** adds slow modulation over the audio-reactive signal — prevents neural adaptation by varying intensity patterns over 8-240 second cycles (default 90 s). Uses Lorenz attractor chaos, micro-oscillator detuning, and sub-harmonic flutter. Off in the default Bass Drum preset.
- **Gate threshold** is compared against pre-volume capture energy after `normalize_capture_energy` / `normalizeCaptureEnergy` (x6, ^0.65, clamped 0-1) on both clients. The auto-gate adapts to ambient noise level.
- **Processing rate:** desktop FFT runs on a 1024-frame hop (~47 Hz at 48 kHz), envelope/output at repaint rate, device dispatch at most 50 Hz (20 ms). Android processing ~60 Hz, UI ~30 Hz, ordinary BLE writes at least 28 ms apart (~36 Hz), stop writes 12 ms.

# Active Work Context

Update this section as work progresses. It survives compaction because CLAUDE.md is re-injected each turn.

**Current state (2026-09-30):** v1.7.0 is the latest release (tag `v1.7.0` = master `bf7448b`, 2026-09-17). No active task. Dave, 2026-09-30, on his Domi 2: "very tight as it is... its good as is." Do not change feel (envelope, slew, prefire, rest floors/curves) without a reported felt problem and a Domi A/B.
**Blocked on:** nothing.
**Recent changes:**
- 2026-09-17 (v1.7.0): Android FIND BOOM (`AutoLockSupervisor.kt`, 1.5 s glide, Revert/Keep); perceptual octave folding + beat-grid phase alignment in both BeatDetectors; band-targeted flux in Low/Band/High Pass modes (Full-mode golden unchanged); Meta Quest / Android silent-capture recovery + landscape window; CI drops the broken setup-android action (`bf7448b`).
- 2026-09-06 (v1.6.0-v1.6.2): envelope stages advance on elapsed time across phase boundaries; Windows output delay follows elapsed time (default 0); Android BLE queue coalesces intensity while keeping stop/control priority; direct ADSR drag editing on both clients (v1.6.1); tap tempo, groove timing loops and faster silence recovery (v1.6.2).
- 2026-08-08 (v1.5.1, Grok WAVE-002 "peak-feel"): prefire strength floor + one-shot latch, desktop Gate before Beat, silence-class through `map_output`, Edge & Deny ships with Climax on, Android 2 s dead-man + sticky Stop. Residuals as of that date are in `docs/waves/WAVE-002-VERIFY.md` (not re-verified since).
- 2026-07-17 (v1.5.0): version bump across VERSION / Cargo / Android versionCode 5; clippy-safe f32 Stroke literals for rustc float-literal-f32-fallback; product release cut (Windows + Android) + rolling `latest` refresh.
- 2026-07-13 (post-1.4 master → 1.5): UI declutter (expert knobs into OVERRIDE; trim forced 0; auto-scan on connect); crash hardening (WASAPI capture panic catch/retry, 80ms buffer, panic-safe Drop, eframe error log); 1s session heartbeat to `%APPDATA%\\chloe-vibes\\session.log` + unclean prev log + panic stamps.
- 2026-07-13: Domi hang fix — boom/pluck envelopes release to true zero (no residual-gate re-arm), device-class rest floors + power curves, strip placebo Rise/Fall, hard-snap silence past slew/dither so motors actually stop.
- 2026-07-10: Bass Drum default + FIND BOOM max-dynamic tuner (UI name for AUTO-LOCK); Chloe tempo macros Deep 90 / Club 125 / Hard 140; Android presets mirrored.
- 2026-07-03 pm (beat-lock + punch, v1.4.0): offline harness (auto_lock.rs ignored test `analyze_real_wav`, env CHLOE_WAV) proved on real 125 BPM material: detector tracks the eighth-note grid (octave error) and the per-poll capture cadence inflated onset jitter 6x. Fixes: perceptual octave folding [333-857ms] before envelope fitting; capture thread analyzes on a fixed 1024-frame hop (~47Hz); gate+detector tick once per FRESH spectral frame (UI was reprocessing duplicates at ~240fps). Auto-Lock re-spec per field brief "think like a bass drum": punch-first band metric (median per-hit jump x reliability x bounded contrast bonus vs between-hit floor), decay 0.78x folded beat w/ curve 1.8 landing on 0.08 sustain floor exactly at the next beat (off-beat eighths eaten mid-Decay by design), binary_level punch policy (p90*1.25, floor 0.55, cap 0.85 — user ceiling binds downstream), slew 0.10x beat. Retry fix: every press starts a FRESH listen (listen_from_ms) — NO LOCK was instantly re-judging the stale ring (dead button). User A/B'd on hardware: "perfect".
- 2026-07-03 pm (research): docs/TEMPORAL_ARCHITECTURE.md — deep-research verdict on the temporal stack. Headline findings: input rise/fall sliders are DEAD in the motor path (feed only the UI meter + motor2 silence check); the hidden 0.35 slew rise ratio owns the real attack; release lies ~2x; micro-pauses never reach the motor and are frame-counted not ms-counted; tempo confidence never decays. Phased fix program in the doc (Phase 0 next: composite Response readout, delete dead sliders from UI+LockParams, confidence decay + pre-fire recency guard in both BeatDetectors).
- 2026-07-03 (AUTO-LOCK phase 1, desktop): new src/auto_lock.rs supervising estimator-controller + docs/AUTO_LOCK_DESIGN.md. One button: listens 4-15s -> per-band onset-salience picks the drive band, IOI stats fit decay/release to tempo (engine-exact centroid pre-compensation), crest factor picks trigger shape; commits via 1.5s glide; LOCKED score + Revert/Keep in top bar. Whitelist-only writes (never volume/gain/floor/ceiling/gate/climax/trim); binary_level capped at observed p90; manual touch or preset click cancels; save() persists pre-lock snapshot so a crash can't make a lock permanent. 9 unit tests.
- 2026-07-03 (adversarial review fixes, 10 confirmed findings): desktop — rate limiter now enable-aware (disable at steady level actually sends zeros), watchdog stop retries on failure, prune preserves full device tuning (multiplier snap-back fixed), devices keyed by Buttplug index not name, scan toggle serialized w/ definitive is_scanning, panic-stop skips capture thread's recovered panics; Android — attemptReconnect handles sync connectGatt failure (no more stuck-Connecting), stopScan moved into connectInternal (reconnect path had lost the GATT-133 guard), stale-GATT identity guard on all callbacks, device picker sorts by stable keys not live RSSI.
- 2026-07-03 (desktop): Device lifecycle rework — prune dropped devices + abort their tasks (fixes stale-task-on-reconnect: device reappeared but never vibrated again), 60s enable-state grace so an RF blip resumes the session; dead-man watchdog (2s pipeline-heartbeat timeout -> device stop; probe-verified update() keeps running while minimized, ages <15ms); panic-stop hook chained before crash.log hook; verified stop-all with red failure banner; Intiface health check (dead server -> Error state + reconnect button); scan calls no longer block the UI thread; per-device multiplier sliders now logarithmic; fixed lying "exponential volume" tooltips; Cargo.toml 0.5.0 -> 1.1.0.
- 2026-07-03 (Android): BLE reliability — close GATT client on every disconnect path (fixes ~32-client leak = "must restart app to connect"), stop scan before connect (GATT 133), auto-reconnect w/ exponential backoff (600ms->8s, 6 attempts), service-discovery failure now drops link cleanly; neverForLocation scan (no GPS needed on 12+); device picker shows friendly Lovense model names sorted by RSSI; Volume/Gain sliders cubic power taper.
- 2026-05-29: Parity hardening — CI now runs the Kotlin parity test; golden widened from 1→6 scenarios (Dynamic/Binary/Hybrid × Wave/Stairs/Surge + high-sustain) + motor2 + shared output-stage columns
- 2026-05-29: Mirrored Kotlin sustain-stage clamp to match Rust; fixed spectral-centroid DC-bin bias (both engines)
- 2026-05-29: Unified the output stage — shared `audio::map_output` (Rust) / `mapOutput` (Kotlin), parity-tested; Android slew is now configurable (`output_slew_ms`) and matches the desktop "pump" feel
- 2026-05-29: Removed desktop-only beat-sync (TapTempo/quantize) — −581 lines in gui.rs; predictive onset (engine-level) retained
- 2026-05-29: Settings correctness — surge_boost clamp 1.5, climax_build_up range aligned to engine, apply_preset is now a complete snapshot + re-sanitizes, fixed dead preset-migration branch
- 2026-03-20: Improved Android haptic timing and BLE command responsiveness

**Validation:** `cargo fmt --all --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked --all-targets` (includes the parity golden), `cargo test -p audio-capture --locked`, and `cd android && ./gradlew testDebugUnitTest testReleaseUnitTest assembleDebug assembleRelease`. Run both sides when touching the engine.

**Known open items (checked against code 2026-09-30 unless marked):**
- Rust still hardwires `rms_power`/`dominant_frequency` to 0.0 (Kotlin computes them), so the desktop RMS fallback in `gui.rs` effectively uses only the legacy loudness value.
- Numeric precision: the desktop engine clock is f32 ms since launch (`as_secs_f32() * 1000`), coarsening to ~1 ms steps after ~2.3 h; the Kotlin FFT derives twiddles by recurrence (no precomputed table); Lorenz is one Euler step per frame. Low priority.
- TEMPORAL_ARCHITECTURE Phase 2 (symmetric ~15 ms slew) remains deferred: the 0.35 / 0.15 slew rise ratios still own attacks in `gui.rs` and `AudioCaptureManager.kt`. Leave it per the current-state note above.
- Android dual-motor is Edge / Edge 2 only, by design.
- Not re-verified since July/August: Android concurrency items (mic-thread join, Visualizer race, onPause/onStop), Android UX/accessibility, WAVE-002 residuals.
- **THRESHOLD barely changes anything on music (2026-10-06, found in the Finally port; same code here).** Gate Threshold is only a noise gate on the normalized capture energy. On music the gate sits open, so the hits come from the spectral-flux BeatDetector and its tempo prefire. Moving THRESHOLD changes almost nothing (Dave: "threshold dead"). Finally fixed it in its own engine, not here: with TEMPO off (its default) the line itself fires the toy. BINARY is a square pulse at Binary Level, held while over the line and faded over RELEASE, and LOCKOUT blocks a re-trigger until the fade ends and a fresh crossing arrives. DYNAMIC follows how far over the line the signal is; HYBRID blends the two. TEMPO on runs this engine unchanged. Reference: FinallyPlayer `integration/studio-music` d3abbbb, `vibes/rhythm/ThresholdTrigger.kt`. Note only; no code changed here.
- **The line's useful range is the top few percent on loud masters (2026-10-08, measured).** The gate compares `normalize_capture_energy` / `normalizeCaptureEnergy` = clamp(6E)^0.65. Synthetic mixes were run through the Finally port of SpectralAnalyzer (Bass Drum, LowPass 120 Hz). Loud master (-8 dBFS RMS): kick p50 0.84 / p90 1.0 (pinned), bass bed p50 0.90 / p90 0.96, and 19 % of frames sit at 1.0. Quiet mix (-20 dBFS RMS): kick 0.40, bed 0.32. In a BINARY simulation over 33 kicks, the loud track hit 33/33 at a line of 0.90, 14/33 at 0.93 and 29/33 at 0.96; the quiet track hit 33/33 anywhere from 0.37 to 0.40. The hysteresis is fine (tightening it to 0.02 added off-beat hits). The cause is the x6 boost plus clamp, not log vs linear; a dB taper would squeeze the top further. Fix direction: measure the line in unclamped dB relative to the track's own level (a rise over the bed, like Finally LIGHTS' 4 dB REARM_RISE_DB), with the clamp out of the gate path. That is a feel change, so it needs a Domi A/B and the parity golden on both engines. Note only; no code changed here.
- Resolved since the old list (don't re-chase): Rust catalog has Deep 90 / Club 125 / Hard 140 + knee/curve fields; Android dual-motor detection uses DeviceType, not name sniffing; Android auto-reconnect and both clients' stop watchdogs shipped.
