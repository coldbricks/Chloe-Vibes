## v1.5.0 — FIND BOOM + Domi hang fix + ship the desk

Built from master @ 4a7650a. Windows x64 exe + Android debug APK (sideload).

### Default path: Bass Drum boom
- **Default = kick-only natural BOOM** — instant peak, long musical decay (~78% of the beat), near-zero floor
- **FIND BOOM** (UI name for AUTO-LOCK) auto-tunes band / gate / trigger / envelope for max dynamic contrast
- Tempo macros: Deep 90 / Club 125 / Hard 140
- Climax stays off on the boom path

### Domi hang fix
- Boom/pluck envelopes release to **true zero** instead of re-arming on residual gate energy
- Device-class rest floors and power curves map intensity for real motors (Domi wand, etc.)
- Placebo Rise/Fall UI removed; silence hard-snaps past slew/dither so the motor actually stops

### UI + crash hardening
- Expert knobs (trigger, slew, curves, trim) collapsed into **OVERRIDE**; main surface is presets + FIND BOOM
- `trim_ms` forced to 0 on load (desktop is already low-latency); auto-start scan on server connect
- WASAPI capture init/read panics caught and retried (class that killed the app during toy scan)
- 80 ms capture buffer; panic-safe Drop with bounded device stop; eframe errors logged instead of unwrap
- **Session heartbeat** every 1s to `%APPDATA%\chloe-vibes\session.log` with clean-exit marker; unclean sessions preserved as `session.prev.log`

### Tooling
- Clippy-clean on rustc 1.97 (`float-literal-f32-fallback` + `useless_borrows_in_formatting`)
- Version aligned: `VERSION` / Cargo / Android `1.5.0` (versionCode 5)

### Downloads
- **Windows:** `ChloeVibes-windows-x64.exe`
- **Android:** `ChloeVibes-android.apk` (debug, sideload)

The rolling `latest` release carries the same builds.
