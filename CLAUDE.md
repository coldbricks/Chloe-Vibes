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
- **Repo Root:** /home/kali/chloe-vibes
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
| Rust build config | `Cargo.toml` |
| Tests | None — tested on real hardware |
| CI/CD | None |
| Generated (DO NOT EDIT) | `android/build/`, `android/app/build/`, `android/.gradle/`, `target/` |

# Build and Run

```
# Android — build debug APK
cd /home/kali/chloe-vibes && JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64 ./gradlew assembleDebug

# Android — install to connected device
adb install -r android/app/build/outputs/apk/debug/app-debug.apk

# Android — clean
cd /home/kali/chloe-vibes/android && ./gradlew clean

# Rust desktop — build
cargo build --release

# Rust desktop — run
cargo run --release
```

Always run the build after making changes. If the build fails, fix it before moving on. Never hand the user a broken build.

# Architecture Invariants

These are load-bearing design decisions. Do not refactor away from them without explicit approval.

1. Signal chain order is fixed: SpectralAnalyzer -> Gate -> BeatDetector -> EnvelopeProcessor -> ClimaxEngine -> Output. Never reorder or skip stages.
2. Audio processing runs on a dedicated thread at ~60Hz. UI reads state via volatile fields. Never do signal processing on the main/UI thread.
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
- Always set `JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64` when running Gradle commands.
- Android SDK is at `~/android-sdk`.

## Grep and Glob
- When searching this project, start with the Key File Map above before doing broad searches.
- Exclude `android/build/`, `android/app/build/`, `android/.gradle/`, `target/` from searches.
- For Kotlin files use glob `**/*.kt`. For Rust files use glob `**/*.rs`.

## Read
- Config files (`build.gradle.kts`, `Cargo.toml`, `AndroidManifest.xml`) are short — read them fully.
- Source files regularly exceed 500 lines (MainScreen.kt is 1165, AudioCaptureManager.kt is 560, gui.rs is 3979) — use offset/limit.

## Agent
- Use for multi-step investigations that require exploring unknown parts of the codebase.
- Do NOT use for tasks where the file locations are already known.

# Environment-Specific Notes

- OS: Kali Linux (zsh), not a standard Android dev environment
- Device connected via ADB is a Samsung Galaxy S23 Ultra — Visualizer API behavior and BLE timing may differ from emulator/other devices
- Lovense Domi 2 is the primary test device for haptic output
- No emulator — all testing is on real hardware
- JAVA_HOME must be set explicitly: `/usr/lib/jvm/java-21-openjdk-amd64`
- Gradle wrapper is in `android/` subdirectory, not repo root

# Domain-Specific Knowledge

- **Visualizer API** taps system audio output (not mic input) — requires `RECORD_AUDIO` permission and an active audio session ID. Returns FFT magnitude data, not raw PCM.
- **Lovense intensity** is 0-20 (integer), mapped to hardware PWM. The `Vibrate:N;` command sets single-motor intensity. `Vibrate1:X;` and `Vibrate2:Y;` control dual motors independently.
- **Nordic UART Service (NUS)** is the BLE GATT service Lovense devices use. TX characteristic UUID: `6e400002-...`, RX: `6e400003-...`. Commands are ASCII with `;` terminator.
- **Spectral flux** is the frame-to-frame change in FFT magnitude — used for onset/beat detection. High flux = transient (drum hit, note attack).
- **ADSR envelope** shapes the haptic response to each beat: Attack (ramp up), Decay (pull back), Sustain (hold), Release (fade out). Curve exponents control the shape of each stage.
- **ClimaxEngine** adds slow modulation over the audio-reactive signal — prevents neural adaptation by varying intensity patterns over 30-120 second cycles. Uses Lorenz attractor chaos, micro-oscillator detuning, and sub-harmonic flutter.
- **Gate threshold** operates on raw spectral energy values, not normalized 0-1. The auto-gate adapts to ambient noise level.
- **Processing rate** is ~60Hz (16ms per frame). UI updates at ~30Hz. BLE command rate is throttled to prevent device buffer overflow.

# Active Work Context

Update this section as work progresses. It survives compaction because CLAUDE.md is re-injected each turn.

**Current task:** Ongoing signal tuning and BLE responsiveness improvements
**Blocked on:** Nothing
**Recent changes:**
- 2026-03-20: Improved Android haptic timing and BLE command responsiveness
- 2026-03-20: Fixed gate threshold (raw energy), cranked gains, fixed BLE command spam
- 2026-03-20: Added output gain slider, bumped input volume to 5x
- 2026-03-19: Fixed gate threshold — replaced broken knee hysteresis, extended range to 0-1
- 2026-03-19: Fixed background attack bug, added ADSR scope + manual entry + log freq slider

**Known issues in current branch:**
- No automated tests — all validation is manual on real devices (S23 Ultra + Domi 2)
- MainScreen.kt is 1165 lines — large but functional, no immediate need to split
