use std::{fmt, mem::size_of, ptr::null_mut, time::Duration};

use winapi::{
    shared::{
        mmreg::{
            WAVEFORMATEX, WAVEFORMATEXTENSIBLE, WAVE_FORMAT_EXTENSIBLE, WAVE_FORMAT_IEEE_FLOAT,
            WAVE_FORMAT_PCM,
        },
        winerror::{E_INVALIDARG, E_POINTER, E_UNEXPECTED, HRESULT_FROM_WIN32, WAIT_TIMEOUT},
    },
    um::{
        audioclient::{
            IAudioCaptureClient, IAudioClient, AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY,
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR,
            AUDCLNT_E_UNSUPPORTED_FORMAT,
        },
        audiosessiontypes::{
            AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
        },
        avrt::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW},
        combaseapi::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL},
        errhandlingapi::GetLastError,
        handleapi::CloseHandle,
        mmdeviceapi::{eConsole, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator},
        synchapi::{CreateEventW, WaitForSingleObject},
        winbase::{WAIT_FAILED, WAIT_OBJECT_0},
        winnt::HANDLE,
    },
    Class, Interface,
};

use crate::{
    read_unaligned,
    win::common::{DATAFORMAT_SUBTYPE_IEEE_FLOAT, DATAFORMAT_SUBTYPE_PCM},
    Format, SampleFormat,
};

use super::common::{winapi_result, WinError};
use super::endpoints::{endpoint_id, ComApartment};

pub use super::endpoints::{
    default_render_endpoint_id, enumerate_render_endpoints, RenderEndpoint,
};

fn duration_to_reference_time(duration: Duration) -> Result<i64, WinError> {
    // WASAPI REFERENCE_TIME is 100 ns. Upstream multiplied nanos by 100,
    // turning the intended 80 ms buffer request into 800 seconds.
    i64::try_from(duration.as_nanos() / 100).map_err(|_| WinError(E_INVALIDARG))
}

// Always a finite wait, so source changes and host shutdown stay observable.
// Round fractional milliseconds up: a short timeout must not become a spin.
fn packet_wait_ms(timeout: Duration) -> u32 {
    timeout.as_nanos().div_ceil(1_000_000).min(1_000) as u32
}

fn last_win32_error() -> WinError {
    WinError(unsafe { HRESULT_FROM_WIN32(GetLastError()) })
}

struct PacketEvent(HANDLE);

impl PacketEvent {
    fn new() -> Result<Self, WinError> {
        // Auto-reset, initially unsignaled; no inheritable or named handle.
        let handle = unsafe { CreateEventW(null_mut(), 0, 0, null_mut()) };
        if handle.is_null() {
            Err(last_win32_error())
        } else {
            Ok(Self(handle))
        }
    }

    fn wait(&self, timeout: Duration) -> Result<bool, WinError> {
        match unsafe { WaitForSingleObject(self.0, packet_wait_ms(timeout)) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            WAIT_FAILED => Err(last_win32_error()),
            _ => Err(WinError(E_UNEXPECTED)),
        }
    }
}

impl Drop for PacketEvent {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct AudioThreadTask(HANDLE);

impl AudioThreadTask {
    fn register() -> Option<Self> {
        // Use the Windows-defined Audio task's normal policy. Registration is
        // optional; unavailable MMCSS leaves the original thread priority.
        let task_name: Vec<u16> = "Audio".encode_utf16().chain(Some(0)).collect();
        let mut task_index = 0;
        let handle = unsafe { AvSetMmThreadCharacteristicsW(task_name.as_ptr(), &mut task_index) };
        if handle.is_null() {
            None
        } else {
            Some(Self(handle))
        }
    }
}

impl Drop for AudioThreadTask {
    fn drop(&mut self) {
        // AudioCapture is thread-bound, so this runs on the registering thread.
        unsafe { AvRevertMmThreadCharacteristics(self.0) };
    }
}

pub struct AudioCapture {
    pub buffer_frame_size: u32,
    pub wave_format: *mut WAVEFORMATEX,
    pub channels: u16,
    pub enumerator: *mut IMMDeviceEnumerator,
    pub device: *mut IMMDevice,
    pub client: *mut IAudioClient,
    pub capture_client: *mut IAudioCaptureClient,
    /// Actual endpoint selected during initialization, including default mode.
    pub endpoint_id: String,
    sample_buffer: Vec<f32>,
    packet_event: Option<PacketEvent>,
    audio_thread_task: Option<AudioThreadTask>,
    // Dropped after our COM interface references. Capture stays on its creator
    // thread; COM apartment ownership must never migrate via Send/Sync.
    _apartment: ComApartment,
}

impl AudioCapture {
    pub fn init(buffer_duration: Duration) -> Result<Self, WinError> {
        Self::init_for_device(buffer_duration, None)
    }

    /// Initialize loopback capture for a stable Windows playback endpoint ID.
    /// `None` resolves the current Windows default (eRender/eConsole).
    /// Prefers event-driven capture with engine-selected buffer capacity. If
    /// event setup fails, retries polling with `buffer_duration` capacity.
    /// This initializes the stream; samples begin only after `start()`.
    pub fn init_for_device(
        buffer_duration: Duration,
        device_id: Option<&str>,
    ) -> Result<Self, WinError> {
        // A failed event setup drops its complete/partial COM client before
        // retrying. An IAudioClient must not be initialized a second time.
        Self::init_with_mode(buffer_duration, device_id, true)
            .or_else(|_| Self::init_with_mode(buffer_duration, device_id, false))
    }

    /// Explicit compatibility mode: use a polling client and the requested
    /// capacity. Automatic event mode lets Windows choose its own buffer.
    pub fn init_for_device_polling(
        buffer_duration: Duration,
        device_id: Option<&str>,
    ) -> Result<Self, WinError> {
        Self::init_with_mode(buffer_duration, device_id, false)
    }

    fn init_with_mode(
        buffer_duration: Duration,
        device_id: Option<&str>,
        event_driven: bool,
    ) -> Result<Self, WinError> {
        let duration = duration_to_reference_time(buffer_duration)?;
        if device_id.is_some_and(|id| id.is_empty() || id.contains('\0')) {
            return Err(WinError(E_INVALIDARG));
        }
        let apartment = ComApartment::new()?;
        // A partially initialized capture owns every successful allocation,
        // so any subsequent HRESULT error cleans up through the same Drop.
        let mut capture = Self {
            buffer_frame_size: 0,
            wave_format: null_mut(),
            channels: 0,
            enumerator: null_mut(),
            device: null_mut(),
            client: null_mut(),
            capture_client: null_mut(),
            endpoint_id: String::new(),
            sample_buffer: Vec::new(),
            packet_event: None,
            audio_thread_task: None,
            _apartment: apartment,
        };
        winapi_result(unsafe {
            CoCreateInstance(
                &MMDeviceEnumerator::uuidof(),
                null_mut(),
                CLSCTX_ALL,
                &IMMDeviceEnumerator::uuidof(),
                &mut capture.enumerator as *mut _ as _,
            )
        })?;

        if let Some(id) = device_id {
            let wide_id: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
            winapi_result(unsafe {
                (*capture.enumerator).GetDevice(wide_id.as_ptr(), &mut capture.device)
            })?;
        } else {
            winapi_result(unsafe {
                (*capture.enumerator).GetDefaultAudioEndpoint(
                    eRender,
                    eConsole,
                    &mut capture.device,
                )
            })?;
        }
        capture.endpoint_id = unsafe { endpoint_id(capture.device) }?;

        winapi_result(unsafe {
            (*capture.device).Activate(
                &IAudioClient::uuidof(),
                CLSCTX_ALL,
                null_mut(),
                &mut capture.client as *mut _ as _,
            )
        })?;

        winapi_result(unsafe { (*capture.client).GetMixFormat(&mut capture.wave_format) })?;
        let wave_format = capture.wave_format;
        capture.channels = unsafe { read_unaligned!(wave_format.nChannels) };
        // Reject formats we cannot decode before exposing a running capture.
        capture
            .format()
            .map_err(|_| WinError(AUDCLNT_E_UNSUPPORTED_FORMAT))?;
        if event_driven {
            capture.packet_event = Some(PacketEvent::new()?);
        }
        winapi_result(unsafe {
            (*capture.client).Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | if event_driven {
                        AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    } else {
                        0
                    },
                // Shared event mode requires zero duration/periodicity; the
                // engine selects its minimum buffer. Polling uses the caller's
                // requested capacity in correctly converted 100 ns units.
                if event_driven { 0 } else { duration },
                0,
                wave_format,
                null_mut(),
            )
        })?;
        if let Some(event) = &capture.packet_event {
            winapi_result(unsafe { (*capture.client).SetEventHandle(event.0) })?;
        }

        winapi_result(unsafe { (*capture.client).GetBufferSize(&mut capture.buffer_frame_size) })?;

        winapi_result(unsafe {
            (*capture.client).GetService(
                &IAudioCaptureClient::uuidof(),
                &mut capture.capture_client as *mut _ as _,
            )
        })?;

        Ok(capture)
    }

    /// Shared audio engine period, distinct from the capture buffer capacity.
    pub fn device_period(&self) -> Result<Duration, WinError> {
        let mut period = 0;
        winapi_result(unsafe { (*self.client).GetDevicePeriod(&mut period, null_mut()) })?;
        if period <= 0 {
            return Err(WinError(E_UNEXPECTED));
        }
        let nanos = u64::try_from(period)
            .ok()
            .and_then(|value| value.checked_mul(100))
            .ok_or(WinError(E_UNEXPECTED))?;
        Ok(Duration::from_nanos(nanos))
    }

    /// True when this stream uses a WASAPI auto-reset packet event.
    pub fn is_event_driven(&self) -> bool {
        self.packet_event.is_some()
    }

    /// True while the running capture owns an MMCSS Audio registration.
    /// An unavailable service is a normal-priority fallback, not an error.
    pub fn is_mmcss_active(&self) -> bool {
        self.audio_thread_task.is_some()
    }

    /// Wait for a packet event, or sleep in polling fallback mode. Waits are
    /// capped at one second. `true` means an event fired, not a packet guarantee.
    /// Call `read_samples` after either result: timeouts also cover older
    /// loopback implementations that initialize successfully but emit no event.
    pub fn wait_for_packet(&self, timeout: Duration) -> Result<bool, WinError> {
        if let Some(event) = &self.packet_event {
            event.wait(timeout)
        } else {
            std::thread::sleep(Duration::from_millis(packet_wait_ms(timeout) as u64));
            Ok(false)
        }
    }

    pub fn format(&self) -> Result<Format, UnknownFormat> {
        let wave_format = self.wave_format;

        let channels;
        let sample_rate;
        let sample_format;
        unsafe {
            let sample_bitsize = read_unaligned!(wave_format.wBitsPerSample);
            let struct_size = read_unaligned!(wave_format.cbSize);
            let format_tag = read_unaligned!(wave_format.wFormatTag);
            sample_format = match (format_tag, sample_bitsize) {
                (WAVE_FORMAT_PCM, 8) => Some(SampleFormat::Int8),
                (WAVE_FORMAT_PCM, 16) => Some(SampleFormat::Int16),
                (WAVE_FORMAT_IEEE_FLOAT, 32) => Some(SampleFormat::Float32),
                (WAVE_FORMAT_EXTENSIBLE, _)
                    if size_of::<WAVEFORMATEXTENSIBLE>() - size_of::<WAVEFORMATEX>()
                        == struct_size as usize =>
                {
                    let wave_format: *mut WAVEFORMATEXTENSIBLE = wave_format as _;
                    let format_guid = read_unaligned!(wave_format.SubFormat);
                    match (format_guid.into(), sample_bitsize) {
                        (DATAFORMAT_SUBTYPE_PCM, 8) => Some(SampleFormat::Int8),
                        (DATAFORMAT_SUBTYPE_PCM, 16) => Some(SampleFormat::Int16),
                        (DATAFORMAT_SUBTYPE_IEEE_FLOAT, 32) => Some(SampleFormat::Float32),
                        _ => None,
                    }
                }
                _ => None,
            };
            sample_rate = read_unaligned!(wave_format.nSamplesPerSec);
            channels = read_unaligned!(wave_format.nChannels);
        }
        let sample_format = sample_format.ok_or(UnknownFormat)?;
        let block_align = unsafe { read_unaligned!(wave_format.nBlockAlign) };
        if channels == 0
            || sample_rate == 0
            || u32::from(block_align)
                != u32::from(channels) * sample_format.bits_per_sample() as u32 / 8
        {
            return Err(UnknownFormat);
        }

        Ok(Format {
            channels,
            sample_rate,
            sample_format,
        })
    }

    pub fn start(&mut self) -> Result<(), WinError> {
        let newly_registered = self.audio_thread_task.is_none();
        if newly_registered {
            self.audio_thread_task = AudioThreadTask::register();
        }
        let result = winapi_result(unsafe { (*self.client).Start() });
        if result.is_err() && newly_registered {
            self.audio_thread_task.take();
        }
        result
    }

    pub fn stop(&mut self) -> Result<(), WinError> {
        let result = winapi_result(unsafe { (*self.client).Stop() });
        self.audio_thread_task.take();
        result
    }

    /// Reads samples from system's internal queue, running provided callback
    /// for each "packet", then return.
    ///
    /// You will need to call this function in loop to keep reading new samples,
    /// as it doesn't spawn background thread for you. It's done this way to
    /// be more flexible for users.
    pub fn read_samples<E, F>(&mut self, mut f: F) -> Result<(), ReadSamplesError<E>>
    where
        F: FnMut(&[f32], Info) -> Result<(), E>,
    {
        let format = self
            .format()
            .map_err(|_| WinError(AUDCLNT_E_UNSUPPORTED_FORMAT))?;
        let mut packet_length = 0;
        winapi_result(unsafe { (*self.capture_client).GetNextPacketSize(&mut packet_length) })?;

        while packet_length > 0 {
            let mut buffer: *mut u8 = null_mut();
            let mut buffer_size = 0;
            let mut flags = 0;
            let mut device_position_frames = 0;
            let mut qpc_position_100ns = 0;
            winapi_result(unsafe {
                (*self.capture_client).GetBuffer(
                    &mut buffer,
                    &mut buffer_size,
                    &mut flags,
                    &mut device_position_frames,
                    &mut qpc_position_100ns,
                )
            })?;
            // WASAPI may return S_BUFFER_EMPTY after the packet-size query.
            if buffer_size == 0 {
                break;
            }
            let mut packet = AcquiredPacket {
                client: self.capture_client,
                frames: buffer_size,
            };

            let is_silent = (flags & AUDCLNT_BUFFERFLAGS_SILENT) != 0;
            let data_discontinuity = (flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY) != 0;
            let timestamp_error = (flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR) != 0;

            let sample_count = (buffer_size as usize)
                .checked_mul(self.channels as usize)
                .ok_or(WinError(E_UNEXPECTED))?;
            if is_silent {
                // Silent packets need not contain usable PCM data. Never
                // dereference their pointer; synthesize exactly their frames.
                self.sample_buffer.clear();
                self.sample_buffer.resize(sample_count, 0.0);
            } else {
                let byte_count = sample_count
                    .checked_mul(format.sample_format.bits_per_sample() as usize / 8)
                    .filter(|count| *count <= isize::MAX as usize)
                    .ok_or(WinError(E_UNEXPECTED))?;
                if buffer.is_null() {
                    return Err(WinError(E_POINTER).into());
                }
                let bytes = unsafe { std::slice::from_raw_parts(buffer, byte_count) };
                decode_samples(bytes, format.sample_format, &mut self.sample_buffer);
            }

            // Release the Windows-owned packet before application code runs;
            // even a panicking callback cannot strand an acquired buffer.
            packet.release()?;

            let info = Info {
                is_silent,
                data_discontinuity,
                timestamp_error,
                device_position_frames,
                qpc_position_100ns,
            };

            f(&self.sample_buffer, info).map_err(ReadSamplesError::E)?;

            winapi_result(unsafe { (*self.capture_client).GetNextPacketSize(&mut packet_length) })?;
        }
        Ok(())
    }
}

fn decode_samples(bytes: &[u8], format: SampleFormat, output: &mut Vec<f32>) {
    output.clear();
    match format {
        SampleFormat::Int8 => {
            output.extend(bytes.iter().map(|sample| (*sample as f32 - 128.0) / 128.0))
        }
        SampleFormat::Int16 => output.extend(
            bytes
                .chunks_exact(2)
                .map(|sample| i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32768.0),
        ),
        SampleFormat::Float32 => output.extend(bytes.chunks_exact(4).map(|sample| {
            let value = f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]);
            if value.is_finite() {
                value
            } else {
                0.0
            }
        })),
    }
}

struct AcquiredPacket {
    client: *mut IAudioCaptureClient,
    frames: u32,
}

impl AcquiredPacket {
    fn release(&mut self) -> Result<(), WinError> {
        let frames = std::mem::take(&mut self.frames);
        winapi_result(unsafe { (*self.client).ReleaseBuffer(frames) })
    }
}

impl Drop for AcquiredPacket {
    fn drop(&mut self) {
        if self.frames != 0 {
            let _ = self.release();
        }
    }
}

pub enum ReadSamplesError<E> {
    E(E),
    WinError(WinError),
}

impl<E: fmt::Debug> fmt::Debug for ReadSamplesError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::E(e) => e.fmt(f),
            Self::WinError(e) => e.fmt(f),
        }
    }
}

impl<E: fmt::Display> fmt::Display for ReadSamplesError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::E(e) => e.fmt(f),
            Self::WinError(e) => e.fmt(f),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ReadSamplesError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ReadSamplesError::E(e) => Some(e),
            ReadSamplesError::WinError(e) => Some(e),
        }
    }
}

impl<E> From<WinError> for ReadSamplesError<E> {
    fn from(e: WinError) -> Self {
        Self::WinError(e)
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(self.wave_format as _);
            if !self.capture_client.is_null() {
                (*self.capture_client).Release();
            }
            if !self.client.is_null() {
                (*self.client).Stop();
                (*self.client).Release();
            }
            if !self.device.is_null() {
                (*self.device).Release();
            }
            if !self.enumerator.is_null() {
                (*self.enumerator).Release();
            }
        }
    }
}

#[allow(unused)]
pub struct Info {
    pub is_silent: bool,
    pub data_discontinuity: bool,
    pub timestamp_error: bool,
    /// Position of the packet's first frame, in frames from stream start.
    pub device_position_frames: u64,
    /// QPC timestamp of its first frame, already converted to 100 ns units.
    /// The timestamp must not be used when `timestamp_error` is set.
    pub qpc_position_100ns: u64,
}

#[derive(Debug)]
pub struct UnknownFormat;

impl fmt::Display for UnknownFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for UnknownFormat {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_wait_timeout_is_finite_and_rounds_up() {
        assert_eq!(packet_wait_ms(Duration::ZERO), 0);
        assert_eq!(packet_wait_ms(Duration::from_nanos(1)), 1);
        assert_eq!(packet_wait_ms(Duration::from_micros(1500)), 2);
        assert_eq!(packet_wait_ms(Duration::from_millis(10)), 10);
        assert_eq!(packet_wait_ms(Duration::MAX), 1000);
    }

    #[test]
    fn packet_event_is_unsignaled_and_auto_resets() {
        let event = PacketEvent::new().unwrap();
        assert!(!event.wait(Duration::ZERO).unwrap());
        assert_ne!(unsafe { winapi::um::synchapi::SetEvent(event.0) }, 0);
        assert!(event.wait(Duration::ZERO).unwrap());
        assert!(!event.wait(Duration::ZERO).unwrap());
    }

    #[test]
    fn wasapi_duration_uses_hundred_nanoseconds() {
        assert_eq!(duration_to_reference_time(Duration::ZERO).unwrap(), 0);
        assert_eq!(
            duration_to_reference_time(Duration::from_millis(80)).unwrap(),
            800_000
        );
        assert_eq!(
            duration_to_reference_time(Duration::from_secs(1)).unwrap(),
            10_000_000
        );
        assert_eq!(
            duration_to_reference_time(Duration::new(2, 123_456_789)).unwrap(),
            21_234_567
        );
    }

    #[test]
    fn wasapi_duration_rejects_overflow() {
        assert!(duration_to_reference_time(Duration::MAX).is_err());
    }

    #[test]
    fn pcm_decoding_preserves_scale_and_channels() {
        let mut result = Vec::new();
        decode_samples(&[0, 128, 255], SampleFormat::Int8, &mut result);
        assert_eq!(result, [-1.0, 0.0, 127.0 / 128.0]);
        decode_samples(&[0, 128, 0, 0, 255, 127], SampleFormat::Int16, &mut result);
        assert_eq!(result, [-1.0, 0.0, 32767.0 / 32768.0]);
    }

    #[test]
    fn float_decoding_preserves_signal_and_rejects_nonfinite() {
        let bytes: Vec<u8> = [-0.75f32, 0.0, 0.8, f32::NAN, f32::INFINITY]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let mut result = vec![99.0];
        decode_samples(&bytes, SampleFormat::Float32, &mut result);
        assert_eq!(result, [-0.75, 0.0, 0.8, 0.0, 0.0]);
    }

    #[test]
    fn invalid_endpoint_id_is_rejected_before_com_activation() {
        assert!(AudioCapture::init_for_device(Duration::from_millis(80), Some("")).is_err());
        assert!(AudioCapture::init_for_device(Duration::from_millis(80), Some("a\0b")).is_err());
    }
}
