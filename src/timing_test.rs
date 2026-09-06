//! An audible end-to-end test: render into the selected playback endpoint,
//! then let the ordinary loopback/DSP/device path respond to the tone.
use crate::{audio_source::AudioSource, settings::Settings};
use audio_capture::{
    win::{capture::AudioCapture, common::winapi_result},
    SampleFormat,
};
use eframe::egui::{self, Color32, ProgressBar, RichText, Slider};
use std::{
    ptr::null_mut,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use winapi::{
    um::{
        audioclient::{IAudioClient, IAudioRenderClient},
        audiosessiontypes::AUDCLNT_SHAREMODE_SHARED,
        combaseapi::CLSCTX_ALL,
        unknwnbase::IUnknown,
    },
    Interface,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Groove {
    WetFloorBass,
    TileRoomThrob,
    HowLongYouLast,
}
impl Groove {
    const ALL: [Self; 3] = [
        Self::WetFloorBass,
        Self::TileRoomThrob,
        Self::HowLongYouLast,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::WetFloorBass => "Wet Floor Bass",
            Self::TileRoomThrob => "Tile Room Throb",
            Self::HowLongYouLast => "How Long You Last",
        }
    }

    fn details(self) -> &'static str {
        match self {
            Self::WetFloorBass => "Around 00:55 · four bars · about 126.6 BPM",
            Self::TileRoomThrob => "Around 00:38 · four bars · 125 BPM",
            Self::HowLongYouLast => "Around 01:30 · four bars · about 126 BPM",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::WetFloorBass => include_bytes!("../assets/wet-floor-bass-loop.wav"),
            Self::TileRoomThrob => include_bytes!("../assets/tile-room-throb-loop.wav"),
            Self::HowLongYouLast => include_bytes!("../assets/how-long-you-last-loop.wav"),
        }
    }
}

#[derive(Clone, Copy)]
struct Tone {
    gain: f32,
    interval: f32,
    duration: f32,
    bass_only: bool,
    sweep: bool,
    frequency: f32,
    groove: bool,
    groove_gain: f32,
    track: Groove,
}
impl Default for Tone {
    fn default() -> Self {
        Self {
            gain: 0.3,
            interval: 3.0,
            duration: 0.6,
            bass_only: true,
            sweep: true,
            frequency: 90.0,
            groove: true,
            groove_gain: 1.0,
            track: Groove::WetFloorBass,
        }
    }
}
impl Tone {
    fn groove_bytes(self) -> &'static [u8] {
        self.track.bytes()
    }

    fn groove_frames(self) -> usize {
        (self.groove_bytes().len() - 44) / 4
    }

    fn length(self) -> f32 {
        if self.groove {
            self.groove_frames() as f32 / 48000.0
        } else {
            self.duration
        }
    }

    fn channel_sample(self, frame: u64, rate: u32, repeat: bool, channel: usize) -> f32 {
        if !self.groove {
            return self.sample(frame, rate, repeat);
        }
        let Some(frame) = frame.checked_sub(rate as u64) else {
            return 0.0;
        };
        // Source-frame interpolation follows the endpoint's native clock.
        let position = frame as f64 * 48000.0 / rate.max(1) as f64;
        let count = self.groove_frames();
        let bytes = self.groove_bytes();
        if !repeat && position >= count as f64 {
            return 0.0;
        }
        let position = position % count as f64;
        let first = position.floor() as usize;
        let next = (first + 1) % count;
        let read = |index: usize| {
            let offset = 44 + index * 4 + (channel % 2) * 2;
            i16::from_le_bytes([bytes[offset], bytes[offset + 1]]) as f32 / 32768.0
        };
        let a = read(first);
        (a + (read(next) - a) * position.fract() as f32) * self.groove_gain.clamp(0.0, 1.0)
    }

    fn sample(self, frame: u64, rate: u32, repeat: bool) -> f32 {
        // One second of leading silence allows capture to settle first.
        let time = frame as f64 / rate.max(1) as f64 - 1.0;
        if time < 0.0 || (!repeat && time >= self.duration as f64) {
            return 0.0;
        }
        let t = time % self.interval.clamp(2.0, 5.0) as f64;
        let duration = self.duration.clamp(0.1, 0.6) as f64;
        if t >= duration {
            return 0.0;
        }
        let rise = (t / 0.004).min(1.0);
        let fade = if self.sweep {
            // Keep energy through the sweep instead of losing most of it
            // before the selected frequency band has been reached.
            ((duration - t) / 0.02).min(1.0)
        } else {
            (-6.0 * t / duration).exp() * ((duration - t) / 0.02).min(1.0)
        };
        let phase = std::f64::consts::TAU * t;
        let wave = if self.sweep {
            // Integral of a linear 40–400 Hz sweep; phase stays continuous.
            (std::f64::consts::TAU * (40.0 * t + 0.5 * (360.0 / duration) * t * t)).sin()
        } else if self.bass_only {
            (self.frequency.clamp(30.0, 2000.0) as f64 * phase).sin()
        } else {
            0.6 * (90.0 * phase).sin() + 0.2 * (350.0 * phase).sin() + 0.2 * (440.0 * phase).sin()
        };
        (wave * rise * fade) as f32 * self.gain.clamp(0.0, 0.8)
    }
}

pub struct TimingTest {
    tone: Tone,
    running: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<String>>,
    worker: Option<JoinHandle<()>>,
    endpoint: Option<String>,
}
impl Default for TimingTest {
    fn default() -> Self {
        Self {
            tone: Tone::default(),
            running: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new("Ready".into())),
            worker: None,
            endpoint: None,
        }
    }
}
impl Drop for TimingTest {
    fn drop(&mut self) {
        self.stop();
    }
}
impl TimingTest {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    pub fn check_route(&mut self, source: Option<&AudioSource>) {
        if self.running.load(Ordering::Acquire)
            && source.map(|s| s.id.as_str()) != self.endpoint.as_deref()
        {
            self.stop();
            *self.status.lock().unwrap_or_else(|e| e.into_inner()) =
                "Audio route changed — test stopped. Start again on the new route.".into();
        }
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            let _ = self.worker.take().unwrap().join();
        }
    }

    fn start(&mut self, source: &AudioSource, repeat: bool, ctx: &egui::Context) {
        if self.worker.is_some() {
            return;
        }
        self.stop.store(false, Ordering::Release);
        self.running.store(true, Ordering::Release);
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) =
            "Opening selected audio output…".into();
        self.endpoint = Some(source.id.clone());
        let id = source.id.clone();
        let tone = self.tone;
        let stop = self.stop.clone();
        let running = self.running.clone();
        let status = self.status.clone();
        let repaint = ctx.clone();
        let result = std::thread::Builder::new()
            .name("timing-tone".into())
            .spawn(move || {
                let result = render(&id, tone, repeat, &stop, &status);
                if let Err(error) = result {
                    *status.lock().unwrap_or_else(|e| e.into_inner()) = error;
                } else if !stop.load(Ordering::Acquire) {
                    *status.lock().unwrap_or_else(|e| e.into_inner()) = "Test finished".into();
                }
                running.store(false, Ordering::Release);
                repaint.request_repaint();
            });
        match result {
            Ok(worker) => self.worker = Some(worker),
            Err(error) => {
                self.running.store(false, Ordering::Release);
                *self.status.lock().unwrap_or_else(|e| e.into_inner()) =
                    format!("Could not start tone: {error}");
            }
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        open: &mut bool,
        source: Option<&AudioSource>,
        settings: &mut Settings,
        output: f32,
        active: bool,
    ) {
        egui::Window::new("Timing test · DOOOP").open(open).default_width(530.0).resizable(false).collapsible(false).show(ctx, |ui| {
            ui.label(RichText::new("HEAR IT. FEEL IT. COMPARE.").color(Color32::from_rgb(56,190,235)).strong());
            ui.label("Pause other music, connect and enable the device, then play a sample.");
            ui.label("Test audio travels through the same capture, preset and output controls as music.");
            ui.label(format!("Output: {}", source.map(|s| s.name.as_str()).unwrap_or("No playback device")));
            ui.separator();
            let running = self.running.load(Ordering::Acquire);
            ui.add_enabled_ui(!running, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for track in Groove::ALL {
                        if ui.selectable_label(self.tone.groove && self.tone.track == track, track.name()).clicked() {
                            self.tone.groove = true;
                            self.tone.track = track;
                        }
                    }
                    ui.selectable_value(&mut self.tone.groove, false, "Tone / sweep");
                });
                if self.tone.groove {
                    ui.label(self.tone.track.details());
                    ui.label("Beat-aligned loop with short fades at the join.");
                    ui.add(Slider::new(&mut self.tone.groove_gain, 0.0..=1.0).text("Groove audio level"));
                } else {
                ui.checkbox(&mut self.tone.sweep, "Frequency sweep: 40 to 400 Hz");
                ui.add_enabled(!self.tone.sweep, egui::Checkbox::new(&mut self.tone.bass_only, "Pure tone (choose frequency)"));
                ui.add_enabled(!self.tone.sweep && self.tone.bass_only, Slider::new(&mut self.tone.frequency, 30.0..=2000.0).logarithmic(true).text("Tone frequency (Hz)"));
                ui.label("Sweep holds its level across the bass bands. Uncheck to choose a fixed tone or chime.");
                ui.add(Slider::new(&mut self.tone.gain, 0.0..=0.8).text("Tone audio level"));
                ui.add(Slider::new(&mut self.tone.interval, 2.0..=5.0).text("Seconds between tones"));
                ui.add(Slider::new(&mut self.tone.duration, 0.1..=0.6).text("Tone length (seconds)"));
                }
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(!running && source.is_some(), egui::Button::new("Loop · 60 seconds")).clicked() { self.start(source.unwrap(), true, ctx); }
                if ui.add_enabled(!running && source.is_some(), egui::Button::new("Play once")).clicked() { self.start(source.unwrap(), false, ctx); }
                if ui.add_enabled(running, egui::Button::new("Stop audio")).clicked() {
                    self.stop();
                    *self.status.lock().unwrap_or_else(|e| e.into_inner()) = "Audio stopped".into();
                }
            });
            ui.label(self.status.lock().unwrap_or_else(|e| e.into_inner()).clone());
            ui.label(if active { "Input: audio present" } else { "Input: silence / waiting" });
            ui.add(ProgressBar::new(output).text(format!("Device command: {:.0}%", output * 100.0)));
            ui.label("This meter shows software output, not measured motor motion.");
            ui.separator();
            ui.label(format!("Input gate: {:.0}% (unchanged by this test)", settings.gate_threshold * 100.0));
            ui.add(Slider::new(&mut settings.trim_ms, 0.0..=500.0).text("Added haptic delay (ms)"));
            ui.label("Use added delay only if the vibration arrives BEFORE the sound. Zero is fastest.");
            ui.label("If vibration arrives late, keep delay at zero and compare wired audio with Bluetooth headphones.");
            ui.label("No response? Choose a tone inside the selected frequency focus, then raise its audio level. Preset, gate and output limits stay unchanged.");
            ui.small("Closing this panel stops test audio. Stop all devices also stops the test.");
        });
        if !*open {
            self.stop();
        }
    }
}

struct Com<T>(*mut T);
impl<T> Com<T> {
    fn empty() -> Self {
        Self(null_mut())
    }
}
impl<T> Drop for Com<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                (*(self.0 as *mut IUnknown)).Release();
            }
        }
    }
}
struct RenderClient(Com<IAudioClient>);
impl Drop for RenderClient {
    fn drop(&mut self) {
        if !self.0 .0.is_null() {
            unsafe {
                (*self.0 .0).Stop();
            }
        }
    }
}

fn render(
    id: &str,
    tone: Tone,
    repeat: bool,
    stop: &AtomicBool,
    status: &Mutex<String>,
) -> Result<(), String> {
    // Reuse the capture backend's thread-bound COM/endpoint/format ownership.
    // This helper's loopback stream is initialized but never started.
    let endpoint = AudioCapture::init_for_device(Duration::from_millis(40), Some(id))
        .map_err(|e| format!("Audio output unavailable: {e}"))?;
    let format = endpoint
        .format()
        .map_err(|e| format!("Unsupported output format: {e}"))?;
    let check = |result| winapi_result(result).map_err(|e| format!("Tone playback failed: {e}"));
    let mut client = RenderClient(Com::<IAudioClient>::empty());
    unsafe {
        check((*endpoint.device).Activate(
            &IAudioClient::uuidof(),
            CLSCTX_ALL,
            null_mut(),
            &mut client.0 .0 as *mut _ as _,
        ))?;
    }
    unsafe {
        check((*client.0 .0).Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            0,
            200_000,
            0,
            endpoint.wave_format,
            std::ptr::null(),
        ))?;
    }
    let mut capacity = 0;
    unsafe {
        check((*client.0 .0).GetBufferSize(&mut capacity))?;
    }
    let mut renderer = Com::<IAudioRenderClient>::empty();
    unsafe {
        check((*client.0 .0).GetService(
            &IAudioRenderClient::uuidof(),
            &mut renderer.0 as *mut _ as _,
        ))?;
    }
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    *status.lock().unwrap_or_else(|e| e.into_inner()) = if tone.groove {
        format!(
            "{} {} — four bars",
            if repeat { "Looping" } else { "Playing" },
            tone.track.name()
        )
    } else if repeat {
        "Repeating — listen to the tone and the silence between pulses".into()
    } else {
        "Playing one tone".into()
    };
    let limit = ((if repeat {
        60.0
    } else {
        1.0 + tone.length() + 1.0
    }) * format.sample_rate as f32) as u64;
    let started = Instant::now();
    let mut cursor = 0;
    let mut playing = false;
    while !stop.load(Ordering::Acquire)
        && cursor < limit
        && started.elapsed() < Duration::from_secs(62)
    {
        let mut padding = 0;
        unsafe {
            check((*client.0 .0).GetCurrentPadding(&mut padding))?;
        }
        let available = capacity
            .saturating_sub(padding)
            .min((limit - cursor).min(u32::MAX as u64) as u32);
        if available > 0 {
            let mut bytes = null_mut();
            unsafe {
                check((*renderer.0).GetBuffer(available, &mut bytes))?;
            }
            // GetBuffer owns exactly available interleaved frames; no allocation
            // or panicking indexing between acquisition and ReleaseBuffer.
            for frame in 0..available as usize {
                for channel in 0..format.channels as usize {
                    let value = tone.channel_sample(
                        cursor + frame as u64,
                        format.sample_rate,
                        repeat,
                        channel,
                    );
                    let sample = frame * format.channels as usize + channel;
                    unsafe {
                        match format.sample_format {
                            SampleFormat::Float32 => {
                                (bytes as *mut f32).add(sample).write_unaligned(value)
                            }
                            SampleFormat::Int16 => (bytes as *mut i16)
                                .add(sample)
                                .write_unaligned((value * i16::MAX as f32).round() as i16),
                            SampleFormat::Int8 => bytes
                                .add(sample)
                                .write((value * 127.0 + 128.0).round() as u8),
                        }
                    }
                }
            }
            unsafe {
                check((*renderer.0).ReleaseBuffer(available, 0))?;
            }
            cursor += available as u64;
            if !playing {
                unsafe {
                    check((*client.0 .0).Start())?;
                }
                playing = true;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grooves_have_valid_pcm_and_clean_repeat_at_endpoint_rates() {
        for track in Groove::ALL {
            let tone = Tone {
                groove: true,
                track,
                ..Tone::default()
            };
            let bytes = tone.groove_bytes();
            assert_eq!(&bytes[..4], b"RIFF");
            assert_eq!(&bytes[36..40], b"data");
            assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 1);
            assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2);
            assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 48000);
            assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16);
            assert!(tone.groove_frames() > 350000 && tone.groove_frames() < 390000);
            for rate in [44100, 48000, 96000] {
                let period = tone.groove_frames() as f64 * rate as f64 / 48000.0;
                for channel in 0..2 {
                    assert_eq!(
                        tone.channel_sample(rate as u64 - 1, rate, true, channel),
                        0.0
                    );
                    let end = rate as u64 + period.ceil() as u64;
                    assert_eq!(tone.channel_sample(end, rate, false, channel), 0.0);
                    let before = tone.channel_sample(end - 1, rate, true, channel);
                    let after = tone.channel_sample(end, rate, true, channel);
                    assert!((before - after).abs() < 0.002, "{} at {rate}", track.name());
                    if period.fract() == 0.0 {
                        for offset in [0, 1, 1000, (period / 2.0) as u64] {
                            let first =
                                tone.channel_sample(rate as u64 + offset, rate, true, channel);
                            let repeated = tone.channel_sample(end + offset, rate, true, channel);
                            assert!(
                                first.is_finite()
                                    && first.abs() <= 1.0
                                    && (first - repeated).abs() < 0.00001
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn sweep_and_selected_bass_tone_reach_the_real_frequency_filter() {
        use crate::audio::{FrequencyMode, SpectralAnalyzer};
        use crate::capture_frames::CaptureFrames;
        for rate in [44100, 48000, 96000] {
            for (tone, minimum_energy) in [
                (
                    Tone {
                        groove: false,
                        ..Tone::default()
                    },
                    0.02,
                ),
                (
                    Tone {
                        groove: true,
                        track: Groove::HowLongYouLast,
                        ..Tone::default()
                    },
                    0.09,
                ),
                (
                    Tone {
                        groove: true,
                        track: Groove::WetFloorBass,
                        ..Tone::default()
                    },
                    0.09,
                ),
                (
                    Tone {
                        groove: true,
                        track: Groove::TileRoomThrob,
                        ..Tone::default()
                    },
                    0.09,
                ),
                (
                    Tone {
                        groove: false,
                        sweep: false,
                        frequency: 50.0,
                        gain: 0.8,
                        ..Tone::default()
                    },
                    0.09,
                ),
            ] {
                let mut analyzer = SpectralAnalyzer::new(rate as f32);
                let mut frames = CaptureFrames::new(1, 2048, 1024);
                let samples: Vec<_> = (0..rate as u64 * 10)
                    .map(|i| tone.channel_sample(i, rate, false, 0))
                    .collect();
                let mut peak = 0.0_f32;
                frames.push(&samples, |window, _| {
                    let spectrum = analyzer.analyze(window, 1);
                    peak = peak.max(SpectralAnalyzer::extract_energy(
                        &spectrum,
                        FrequencyMode::LowPass,
                        110.0,
                    ));
                });
                assert!(
                    peak > minimum_energy,
                    "{rate} Hz: filtered peak {peak} below {minimum_energy}"
                );
            }
        }
    }
    #[test]
    fn tone_has_bounded_amplitude_clean_edges_and_seconds_of_true_silence() {
        let tone = Tone::default();
        for rate in [44100, 48000, 96000] {
            for frame in 0..rate as u64 * 7 {
                let x = tone.sample(frame, rate, true);
                assert!(x.is_finite() && x.abs() <= tone.gain);
                let time = frame as f64 / rate as f64;
                if time < 1.0 || (time - 1.0) % 3.0 >= tone.duration as f64 {
                    assert_eq!(x, 0.0);
                }
            }
            assert_eq!(tone.sample(rate as u64, rate, true), 0.0);
            assert_eq!(tone.sample(rate as u64 * 4, rate, false), 0.0);
        }
    }
}
