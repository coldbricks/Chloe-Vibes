## v1.5.1 — Peak feel (WAVE-002)

Tryout build from master + WAVE-001/002 feel work. Windows x64 exe + Android debug APK (sideload).

### Punch & timing
- Prefire actually fires: strength floor + one-shot latch per predicted beat
- Desktop pipeline **Gate → Beat → prefire** (current-frame gate; Android parity)
- Onset pre-gate unified to **1.05** (no 1.02–1.05 dead band)
- Gate-edge thump can upgrade to real onset velocity within 20 ms

### Motor-honest contrast
- Silence-class through `map_output` + hard-snap past slew
- Micro-pauses 90–120 ms (time-based); schedule survives retrigger
- BLE peak-hold ~22 ms + large-drop / zero bypass

### Climax (Edge & Deny)
- Factory **Edge & Deny** has climax **ON** (was a no-op)
- Deep deny → true rest; post-deny soft ceiling 1.12
- Dual-motor: when climax ON, `motor2` follows ClimaxEngine spatial arc (not treble-only theater)

### Safety & UX
- Android 2 s audio-pipeline dead-man + lease fence + sticky Stop all
- Boom tempo macros Deep 90 / Club 125 / Hard 140 honesty
- Arousal-oriented Android safety rail

### Tooling
- Version: `VERSION` / Cargo / Android **1.5.1** (versionCode **6**)
- Parity golden regenerated; `cargo test` + `gradlew testDebugUnitTest` green

### Try it
1. **Windows:** run `ChloeVibes-windows-x64.exe` — Intiface on `ws://127.0.0.1:12345` or embedded server
2. **Android:** sideload `ChloeVibes-android.apk` — mic + nearby devices
3. Music: Bass Drum / FIND BOOM on a kick track
4. Climax: Edge & Deny; dual-motor Edge if you have one
5. **Stop all devices** when done

Hardware A/B still the real gate (Domi rest / Edge dual). Phase-2 15 ms slew still deferred.
