# ChloeVibes 1.6.0 — Feel every phrase

More expressive timing, clearer controls, and more dependable output on Windows and Android.

## Shape the music

- Start with presets, then reveal full attack, decay, sustain, release, and envelope curves when you want more control. Windows has expandable tuning; Android has Simple and Full controls.
- Envelopes now carry elapsed time correctly across phase boundaries. First hits trigger reliably, held audio frames do not invent extra onsets, and predictive timing avoids replaying the same beat.
- Windows FIND BOOM measures fresh audio and restores the complete previous response when you choose Revert.
- Windows output delay now follows elapsed time for both motors. Added delay defaults to zero; the optional delay control is for alignment, not a promise of zero end-to-end latency.

## Keep output responsive

- Windows capture polls more frequently, preserves analysis cadence, and clears stale audio. Device commands have bounded waits and retry the latest intensity after failures.
- Android processes each captured audio frame once while continuing envelope updates between frames. Microphone startup, source switching, and capture cleanup are more robust.
- Android Bluetooth queues coalesce changing intensity while preserving stop and control commands. Connection setup waits for notification readiness, stale callbacks are discarded, and disconnect attempts a bounded stop before closing.
- Output ceilings apply after gain and smoothing. Both apps clear stale input and prioritize zero output; disconnected hardware may still be unable to receive a stop command.

## Download

| Platform | File |
| --- | --- |
| Windows x64 | `ChloeVibes-windows-x64.exe` |
| Android 8.0+ | `ChloeVibes-android.apk` — debug build for sideloading |

`app-debug.apk` is an identical compatibility copy of the Android download. All published Android aliases for this version use the same APK and signing certificate. The certificate matches the main v1.5.1 `ChloeVibes-android.apk`; some older CI-generated APKs used different certificates, so Android may reject an in-place update from those builds. Uninstalling an older build removes its app data.

Windows supports compatible vibration and oscillation devices through Intiface / Buttplug. Android connects directly to supported Lovense Bluetooth devices. Android system-audio capture depends on the phone and playback app; microphone input is an explicit alternative.

## Validation

Rust formatting, strict Clippy checks, all-target tests, release compilation, Android unit tests, and APK assembly pass. Cross-platform golden scenarios check Rust/Kotlin signal behavior. These automated checks do not establish physical device latency or subjective feel.

The repository now has a concise visual overview, with setup, controls, compatibility, and development details in the [technical reference](https://github.com/coldbricks/Chloe-Vibes/blob/v1.6.0/docs/TECHNICAL_REFERENCE.md). CI produces test artifacts and does not overwrite published release downloads.
