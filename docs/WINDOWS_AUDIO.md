# Windows audio and response timing

The **Audio source** menu chooses the playback device ChloeVibes listens to. It captures music sent to speakers or headphones; it is not a microphone selector.

**Follow Windows default** is the initial setting. When Windows changes its default playback device, ChloeVibes reconnects to that device automatically. There is no need to restart after switching headphones or speakers. Device discovery refreshes approximately twice a second.

To follow one particular output, select its name. That choice is remembered across restarts and stays selected when Windows changes its default. If the selected device disconnects, output stops while ChloeVibes waits for it to return. Choose **Follow Windows default** to resume following Windows instead.

Applications can route their sound to a different output from the Windows default. Select the output that is actually playing your music. The status beside the connection controls shows the active capture device.

## Getting a clearer pulse

The large **Envelope** graph is directly editable. Drag **A** horizontally for
Attack, **D** horizontally for Decay and vertically for Sustain, **S** vertically
for Sustain, and **R** horizontally for Release. Values update live and stay in
sync with the detailed sliders. A manual edit takes control from FIND BOOM and
marks the patch as Custom. The time scale stays still while dragging;
double-click empty graph space to fit the current shape.

Sustain's displayed width is an illustration, not a timed hold. Its level and
the trigger mode determine the sustained response. Connection messages occupy
fixed rows; hover a truncated message to read the full details. Long content
scrolls inside the window instead of changing its size.

Start with **Bass Drum**, then adjust output to a comfortable level. **FIND BOOM** fits frequency focus, gate and envelope shape to a short passage. Its fit describes the response to the music; it does not measure speaker-to-motor latency or guarantee a downbeat lock.

The **Threshold** control determines when the selected audio energy opens the gate. A very high threshold may wait until a bass note has swelled before starting a pulse. Lowering it can start earlier but can also admit extra notes. Change one control at a time when comparing the same musical passage.

Short Attack settings below 50 ms use the immediate-punch path; longer settings create a gradual software ramp. Decay, Release and output smoothing determine how much of the previous pulse remains between hits. Preserve clear rests if you want separated kicks.

Bluetooth headphones add delay to what you hear. That may change perceived alignment with a Bluetooth toy. Compare against wired audio when investigating response timing, and judge the toy's physical response separately from software command timing.

## Capture and output behavior

ChloeVibes prefers event-driven WASAPI capture and registers its capture thread with Windows' multimedia audio scheduler. If those facilities are unavailable, it falls back to bounded polling and normal thread scheduling. Analysis uses exact sample-count hops, independent of how Windows divides packets.

Device commands wake when a new output is available. Each device retains a maximum of 50 normal output batches per second and one batch in flight. Obsolete output is replaced by the newest value, and failed commands are retried. Audio-device changes discard previous analysis and prevent old-source output from being reused.

Code signing and the application icon do not change audio or Bluetooth timing.

For source and visualization diagnostics without connecting to a server or
scanning for toys, launch `chloe-vibes.exe --audio-only`. This mode lasts for
that launch only. Close it and start normally to connect devices again.
Add `--settings-path "C:\path\to\test-app.ron"` to use a separate settings
file for comparisons. Without this option, the app uses your normal saved settings.
