# ChloeVibes Windows capture dependency

Vendored from [Shadlock0133/audio-capture](https://github.com/Shadlock0133/audio-capture)
at commit `26e326cffcf00cdc564a2b840c6421e021e0c27f`. The original MIT license is
preserved in `LICENSE`. This directory is the desktop application's dependency;
do not apply its fixes to the Cargo cache.

Local changes:

- Convert `Duration` to WASAPI's 100 ns units correctly, with checked overflow.
  The polling fallback's 80 ms request now requests 80 ms of capacity.
  Capacity is not a measurement of audio-to-motor latency.
- Enumerate active playback endpoints with stable Windows IDs, friendly names,
  and the current default. Discovery opens no capture stream.
- Keep `AudioCapture::init(duration)` and add
  `AudioCapture::init_for_device(duration, Option<&str>)`. `None` resolves the
  current Windows default for the existing `eRender` / `eConsole` role. The
  resolved ID is available as `capture.endpoint_id`; changing endpoints still
  requires the host to drop and recreate capture.
- Expose `device_period()` separately from buffer capacity. Expose packet frame
  position and the already-converted 100 ns QPC timestamp in `Info`.
- Prefer event-driven WASAPI loopback with an auto-reset event. Shared event
  mode passes zero buffer duration and lets Windows choose capacity. If setup
  fails, release the partial client and retry a fresh polling client with the
  requested capacity. `wait_for_packet(timeout)` wakes on the event or uses a
  bounded sleep for polling fallback; call `read_samples` after both true and
  false results. Waits never exceed one second and use no global timer changes.
- Register the capture thread with the Windows MMCSS `Audio` task on `start()`;
  release the registration on `stop()` or drop, on the same thread. If MMCSS is
  unavailable, continue at normal priority. `is_event_driven()` and
  `is_mmcss_active()` report the actual state. No registry, power or process-wide
  scheduling settings are changed.
- Decode supported unsigned 8-bit PCM, signed 16-bit PCM and 32-bit float PCM
  into normalized floats. Silent packets become zeros without reading their
  data pointer. Nonfinite float samples become zeros. Other formats are rejected.
- Return initialization errors instead of unwrapping them. Release partial COM
  allocations on errors, free endpoint IDs/property values, balance apartment
  initialization, and release packets before calling application code. Capture
  must stay on the thread that created it.
- Accept successful nonzero HRESULTs and make Windows error formatting robust.

The public endpoint functions and `RenderEndpoint { id, name, is_default }` are
re-exported from `audio_capture::win::capture`:

```rust,ignore
let outputs = enumerate_render_endpoints()?;
let default_id = default_render_endpoint_id()?;
```

From the repository root, run `cargo test -p audio-capture --offline` and
`cargo clippy -p audio-capture --all-targets --offline -- -D warnings`.
The ignored `list_playback_endpoints` test performs read-only enumeration when
explicitly invoked; it never starts audio capture.

Windows API contracts: [shared event-driven initialization](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-initialize),
[event handle setup](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-seteventhandle),
[MMCSS task registration](https://learn.microsoft.com/en-us/windows/win32/api/avrt/nf-avrt-avsetmmthreadcharacteristicsw),
[same-thread registration release](https://learn.microsoft.com/en-us/windows/win32/api/avrt/nf-avrt-avrevertmmthreadcharacteristics).
