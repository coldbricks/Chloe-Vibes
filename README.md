# Chloe Vibes

**Audio-reactive haptic controller** — A synthesizer for vibration motors. Captures system audio, analyzes it with FFT spectral analysis, and drives Bluetooth haptic devices through the [Buttplug.io](https://buttplug.io) protocol.

## What's New in v4

### Climax Engine (New)
Optional long-cycle modulation layer designed for stronger sessions:
- **Build -> Tease -> Surge** cycle on top of audio-reactive ADSR output
- **Pattern modes:** Wave, Stairs, Surge
- **Profiles:** Edge, Overload, Punisher
- **Live cycle meter** and **Reset** button for manual timing

Controls are in the **CLIMAX** section and are fully persistent across runs.

### Preset System
Presets snap all signal-processing parameters to known-good configurations. Think of them like synth patches — pick one as a starting point, then tweak.

**Categories:**
- **INIT** — Default, Transparent (neutral starting points)
- **DRUMS** — Drum Hit, Kick Follow, Staccato Pulse (short, punchy envelopes)
- **MUSICAL** — Slow Swell, Vocal Ride, Pluck (smooth, sustained tracking)
- **BASS** — Sub Throb, Wobble Bass (heavy, low-frequency focused)
- **FX** — Ambient Wash, Heartbeat, Hi-Hat Tingle (creative effects)

When you manually adjust any parameter, the preset shows as "Custom."

### Redesigned Controls
The UI now uses proper synthesizer terminology with clear labeling:

**Signal Chain:** `Audio In → Frequency Filter → Noise Gate → Trigger → ADSR Envelope → Output Range → Device`

- **INPUT** — Volume gain and frequency filter mode (Full Range, Low Pass, High Pass, Band Pass)
- **GATE** — Noise gate with Threshold, Auto-Sense, and Smoothing
- **TRIGGER** — How audio energy becomes vibration intensity (Dynamic / Binary / Hybrid)
- **ENVELOPE** — Color-coded A/D/S/R sliders matching the visual preview
- **OUTPUT** — Floor (minimum when active) and Ceiling (maximum clamp)

### Improved ADSR Visibility
- Envelope shape visualization labeled "ENVELOPE SHAPE — Attack → Decay → Sustain → Release"
- Phase labels color-coded: teal (A) / purple (D) / amber (S) / red (R)
- Matching slider label colors so you can see which knob controls which curve segment
- Hover tooltips on every parameter with synth analogies

### Gate/Threshold Clarity
- **Threshold** = "How loud audio must be to trigger vibration"
- **Auto-Sense** = "Automatically adapt threshold to music level"
- **Smooth** = "How gradually the gate opens/closes"
- Live OPEN/CLOSED indicator

## Architecture

```
System Audio → FFT Analysis → Freq Filter → Noise Gate → Trigger → ADSR Envelope → Device Output
```

## Building

```
cargo build --release
```

Requires Windows (WASAPI audio capture).
By default, the app launches an embedded Buttplug server, so Intiface Central is optional.
If you want to use Intiface, start it and pass `--server-addr ws://127.0.0.1:12345`.

## Quick Start

1. Run `chloe-vibes.exe`
2. Click "Start scanning" to find devices
3. Enable your device
4. Select a preset, play music, adjust to taste
5. Optional: enable **CLIMAX** and pick a profile (Edge / Overload / Punisher)
6. Double-click any slider to reset to default
