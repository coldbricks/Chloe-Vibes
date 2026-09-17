# ChloeVibes 1.7.0 — Musical Intelligence & Android FIND BOOM

Automated sweet-spot bass-drum tuning on Android, musical rhythm intelligence with syncopation lock, and frequency-targeted transient flux on Windows and Android.

## Android FIND BOOM (Auto-Lock)

- **Automated Sweet-Spot Tuner:** Android brings the full desktop FIND BOOM experience to mobile. One tap listens to the playing audio track for 4–8 seconds, evaluates energy punch across 8 FFT bands, finds the dominant kick frequency, sets an optimal dynamic gate, and fits a boom envelope decay spanning ~78% of the felt beat.
- **Interactive FindBoomCard:** Clean, responsive Jetpack Compose card with real-time state feedback (`FIND BOOM`, `TUNING Xs…`, `BOOM XX%`, `NO LOCK`), complete fitted parameter readout line, and Revert / Keep controls.
- **1.5s Parameter Gliding:** Tuned parameters glide smoothly into place over 1.5 seconds so motor feel transitions seamlessly without abrupt jerks. Tweaking any slider dissolves the lock back to Idle safely.

## Musical Rhythm Intelligence & Latency Compensation

- **Perceptual Beat Folding (`BeatDetector`):** Solves the syncopation bug where 8th-note hi-hats, offbeats, and subdivisions caused inter-onset interval CV to spike, collapsing tempo confidence to 0.0. Candidate intervals are folded into the human perceptual beat window (70–180 BPM, 333–857 ms) with harmonic candidate evaluation, locking syncopated grooves with ≥0.70 confidence.
- **Beat-Grid Phase Alignment:** Candidate onset anchors are phase-tested against recent onset history modulo beat interval, anchoring predictions directly onto the downbeat grid rather than floating offbeats.
- **Predictive Prefire Latency Compensation:** With rhythm confidence locked on syncopated material, the 50 ms predictive pre-fire engages reliably, ensuring mechanical motor attacks land precisely on-beat with zero perceptual lag.

## Frequency-Targeted Transient Flux

- **Targeted Mode Flux Extraction (`SpectralAnalyzer`):** Band-delta flux calculation isolates transients within the active filter range (`LowPass`, `BandPass`, `HighPass`). High-frequency vocal consonants, snare hits, and cymbal chatter are rejected in LowPass bass-drum mode, eliminating spurious sub-channel vibration.
- **Bit-for-Bit Golden Parity:** Preserved `FrequencyMode::Full` parity contract (0 diff across all 6 scenarios in the synthetic regression suite).

## System Audio & Stability

- **Meta Quest / Android Audio Recovery:** Hardened system audio capture initialization with `initializeVisualizer` and `VisualizerSetupTarget`, preventing silent capture deadlocks and improving landscape orientation sizing on headsets.

## Downloads

| Platform | File |
| --- | --- |
| Windows x64 | `ChloeVibes-windows-x64.exe` |
| Android 8.0+ | `ChloeVibes-android.apk` / `app-debug.apk` |
| Android 8.0+ (Optimized) | `ChloeVibes-android-release.apk` |
| Checksums | `SHA256SUMS.txt` |
