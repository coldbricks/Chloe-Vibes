use std::{
    collections::VecDeque,
    env, fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use buttplug::{
    client::{ButtplugClient, ButtplugClientError},
    core::{
        connector::{
            ButtplugInProcessClientConnectorBuilder, ButtplugRemoteClientConnector as RemoteConn,
            ButtplugWebsocketClientTransport as WebsocketTransport,
        },
        message::serializer::ButtplugClientJSONSerializer as JsonSer,
    },
    server::{
        device::{configuration::DeviceConfigurationManagerBuilder, ServerDeviceManagerBuilder},
        ButtplugServerBuilder,
    },
    util::device_configuration::load_protocol_configs,
};

const DEFAULT_SERVER_ADDR: &str = "ws://127.0.0.1:12345";

pub type ClientConnectionResult = Result<ButtplugClient, Box<ButtplugClientError>>;

pub const NO_CAPTURE_PACKET: u64 = u64::MAX;
pub const CAPTURE_STALE_MS: u64 = 250;

/// The GUI being alive is not evidence that the audio source is alive.
pub fn capture_is_fresh(now_ms: u64, last_packet_ms: u64) -> bool {
    last_packet_ms != NO_CAPTURE_PACKET && now_ms.saturating_sub(last_packet_ms) < CAPTURE_STALE_MS
}

/// Apply the current user ceiling after any delay or smoothing history.
pub fn limit_output(level: f32, ceiling: f32) -> f32 {
    if !level.is_finite() || !ceiling.is_finite() {
        return 0.0;
    }
    level.clamp(0.0, ceiling.clamp(0.0, 1.0))
}

/// Delay in elapsed time, independent of repaint rate. A changed delay starts
/// a new timeline rather than replaying output from the previous setting.
#[derive(Default)]
pub struct OutputDelay {
    samples: VecDeque<(f64, f32)>,
    delay_ms: f32,
}

impl OutputDelay {
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    pub fn push(&mut self, now_ms: f64, level: f32, delay_ms: f32) -> f32 {
        let delay_ms = if delay_ms.is_finite() {
            delay_ms.clamp(0.0, 500.0)
        } else {
            0.0
        };
        if self.delay_ms != delay_ms || self.samples.back().is_some_and(|(time, _)| *time > now_ms)
        {
            self.clear();
            self.delay_ms = delay_ms;
        }
        if delay_ms == 0.0 {
            self.clear();
            return level;
        }
        self.samples.push_back((now_ms, level));
        let due_ms = now_ms - f64::from(delay_ms);
        while self.samples.len() > 1 && self.samples[1].0 <= due_ms {
            self.samples.pop_front();
        }
        self.samples
            .front()
            .filter(|(time, _)| *time <= due_ms)
            .map_or(0.0, |(_, value)| *value)
    }
}

async fn connect_remote(client_name: &str, addr: &str) -> ClientConnectionResult {
    let remote_connector =
        RemoteConn::<_, JsonSer>::new(WebsocketTransport::new_insecure_connector(addr));
    let client = ButtplugClient::new(client_name);
    client.connect(remote_connector).await?;
    Ok(client)
}

async fn connect_embedded(client_name: &str) -> Option<ButtplugClient> {
    let name = client_name.to_string();
    match tokio::task::spawn(async move { start_embedded_client(&name).await }).await {
        Ok(Ok(client)) => Some(client),
        Ok(Err(e)) => {
            eprintln!("Embedded in-process server startup failed: {e}");
            None
        }
        Err(e) => {
            eprintln!("Embedded in-process server failed: {e}");
            None
        }
    }
}

fn intiface_config_paths() -> Option<(PathBuf, PathBuf)> {
    let appdata = env::var("APPDATA").ok()?;
    let base = PathBuf::from(appdata)
        .join("com.nonpolynomial")
        .join("intiface_central")
        .join("config");
    Some((
        base.join("buttplug-device-config-v3.json"),
        base.join("buttplug-user-device-config-v3.json"),
    ))
}

fn build_device_config_manager_builder(
    allow_raw_messages: bool,
) -> DeviceConfigurationManagerBuilder {
    let mut builder = if let Some((main_path, user_path)) = intiface_config_paths() {
        let main_cfg = fs::read_to_string(&main_path).ok();
        let user_cfg = fs::read_to_string(&user_path).ok();

        if main_cfg.is_some() || user_cfg.is_some() {
            match load_protocol_configs(&main_cfg, &user_cfg, true) {
                Ok(builder) => {
                    eprintln!("Loaded embedded config from Intiface files");
                    builder
                }
                Err(e) => {
                    eprintln!("Couldn't load Intiface config for embedded mode: {e}");
                    DeviceConfigurationManagerBuilder::default()
                }
            }
        } else {
            DeviceConfigurationManagerBuilder::default()
        }
    } else {
        DeviceConfigurationManagerBuilder::default()
    };

    builder.allow_raw_messages(allow_raw_messages);
    builder
}

async fn start_embedded_client(client_name: &str) -> Result<ButtplugClient, String> {
    let mut dcm_builder = build_device_config_manager_builder(false);
    let dcm = dcm_builder
        .finish()
        .map_err(|e| format!("DCM build failed: {e}"))?;

    let mut device_manager_builder = ServerDeviceManagerBuilder::new(dcm);

    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "ios",
        target_os = "android"
    ))]
    {
        use buttplug::server::device::hardware::communication::btleplug::BtlePlugCommunicationManagerBuilder;
        device_manager_builder.comm_manager(BtlePlugCommunicationManagerBuilder::default());
    }

    {
        use buttplug::server::device::hardware::communication::websocket_server::websocket_server_comm_manager::WebsocketServerDeviceCommunicationManagerBuilder;
        device_manager_builder.comm_manager(
            WebsocketServerDeviceCommunicationManagerBuilder::default()
                .listen_on_all_interfaces(false),
        );
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        use buttplug::server::device::hardware::communication::serialport::SerialPortCommunicationManagerBuilder;
        device_manager_builder.comm_manager(SerialPortCommunicationManagerBuilder::default());
    }

    {
        use buttplug::server::device::hardware::communication::lovense_connect_service::LovenseConnectServiceCommunicationManagerBuilder;
        device_manager_builder
            .comm_manager(LovenseConnectServiceCommunicationManagerBuilder::default());
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        use buttplug::server::device::hardware::communication::lovense_dongle::{
            LovenseHIDDongleCommunicationManagerBuilder,
            LovenseSerialDongleCommunicationManagerBuilder,
        };
        device_manager_builder.comm_manager(LovenseHIDDongleCommunicationManagerBuilder::default());
        device_manager_builder
            .comm_manager(LovenseSerialDongleCommunicationManagerBuilder::default());
    }

    #[cfg(target_os = "windows")]
    {
        use buttplug::server::device::hardware::communication::xinput::XInputDeviceCommunicationManagerBuilder;
        device_manager_builder.comm_manager(XInputDeviceCommunicationManagerBuilder::default());
    }

    let device_manager = device_manager_builder
        .finish()
        .map_err(|e| format!("Device manager build failed: {e}"))?;
    let server = ButtplugServerBuilder::new(device_manager)
        .finish()
        .map_err(|e| format!("Server build failed: {e}"))?;
    let connector = ButtplugInProcessClientConnectorBuilder::default()
        .server(server)
        .finish();
    let client = ButtplugClient::new(client_name);
    client
        .connect(connector)
        .await
        .map_err(|e| format!("Client connect failed: {e}"))?;
    Ok(client)
}

pub async fn start_bp_server(server_addr: Option<String>) -> ClientConnectionResult {
    let name = "chloe-vibes";
    // Validate server address scheme
    let server_addr = server_addr.filter(|addr| {
        if addr.starts_with("ws://") || addr.starts_with("wss://") {
            if !addr.contains("127.0.0.1") && !addr.contains("localhost") && addr.starts_with("ws://") {
                eprintln!("Warning: connecting to non-localhost server without TLS. Consider using wss://");
            }
            true
        } else {
            eprintln!("Invalid server address: must start with ws:// or wss://. Falling back to embedded server.");
            false
        }
    });
    let client = if let Some(addr) = server_addr.as_deref() {
        match connect_remote(name, addr).await {
            Ok(client) => client,
            Err(e) => {
                eprintln!("Couldn't connect to external server ({addr}): {e}");
                eprintln!("Trying embedded in-process server");
                if let Some(client) = connect_embedded(name).await {
                    client
                } else {
                    return Err(e);
                }
            }
        }
    } else {
        eprintln!("No server configured; trying external server at {DEFAULT_SERVER_ADDR}");
        match connect_remote(name, DEFAULT_SERVER_ADDR).await {
            Ok(client) => client,
            Err(e) => {
                eprintln!(
                    "Couldn't connect to default external server ({DEFAULT_SERVER_ADDR}): {e}"
                );
                eprintln!("Falling back to embedded in-process server");
                if let Some(client) = connect_embedded(name).await {
                    client
                } else {
                    return Err(e);
                }
            }
        }
    };

    let server_name = client.server_name();
    let server_name = server_name.as_deref().unwrap_or("<unknown>");
    eprintln!("Server name: {server_name}");

    Ok(client)
}

#[derive(Clone)]
pub struct SharedF32(Arc<AtomicU32>);

impl SharedF32 {
    pub fn new(v: f32) -> Self {
        Self(Arc::new(AtomicU32::new(v.to_bits())))
    }

    pub fn store(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

pub fn low_pass(samples: &[f32], time: Duration, rc: f32, channels: usize) -> Vec<f32> {
    let len = samples.len();
    if len < channels {
        return vec![];
    }
    let mut res = vec![0.0; len];
    let dt = time.as_secs_f32();
    let a = dt / (rc + dt);
    for c in 0..channels {
        res[c] = a * samples[c];
    }
    for i in channels..len {
        res[i] = a * samples[i] + (1.0 - a) * res[i - channels];
    }
    res
}

pub fn calculate_power(samples: &[f32], channels: usize) -> Vec<f32> {
    let mut sums = vec![0.0; channels];
    for frame in samples.chunks_exact(channels) {
        for (acc, sample) in sums.iter_mut().zip(frame) {
            *acc += sample.abs().powi(2);
        }
    }
    let frame_count = samples.len() / channels;
    for sum in &mut sums {
        *sum /= frame_count.max(1) as f32;
        *sum = sum.sqrt().clamp(0.0, 1.0);
    }
    sums
}

pub fn avg(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().sum::<f32>() / samples.len() as f32
}

pub trait MinCutoff {
    fn min_cutoff(self, min: Self) -> Self;
}

impl MinCutoff for f32 {
    fn min_cutoff(self, min: Self) -> Self {
        if self < min {
            0.0
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_health_requires_packets_even_if_the_ui_keeps_ticking() {
        assert!(!capture_is_fresh(0, NO_CAPTURE_PACKET));
        assert!(capture_is_fresh(1_000, 1_000));
        assert!(capture_is_fresh(1_249, 1_000));
        for now in [1_250, 1_500, 2_000, 20_000] {
            assert!(!capture_is_fresh(now, 1_000));
        }
        assert!(capture_is_fresh(20_001, 20_001));
    }

    #[test]
    fn delay_uses_timestamps_across_irregular_frames_and_preserves_rests() {
        let mut delay = OutputDelay::default();
        assert_eq!(delay.push(0.0, 0.8, 100.0), 0.0);
        assert_eq!(delay.push(20.0, 0.4, 100.0), 0.0);
        assert_eq!(delay.push(70.0, 0.0, 100.0), 0.0);
        assert_eq!(delay.push(99.0, 0.2, 100.0), 0.0);
        assert_eq!(delay.push(100.0, 0.3, 100.0), 0.8);
        assert_eq!(delay.push(160.0, 0.6, 100.0), 0.4);
        assert_eq!(delay.push(175.0, 0.7, 100.0), 0.0);
    }

    #[test]
    fn capture_stop_or_delay_change_cannot_replay_an_old_peak() {
        let mut delay = OutputDelay::default();
        delay.push(0.0, 1.0, 100.0);
        delay.clear();
        assert_eq!(delay.push(150.0, 0.0, 100.0), 0.0);
        assert_eq!(delay.push(300.0, 0.0, 100.0), 0.0);
        delay.push(400.0, 1.0, 100.0);
        assert_eq!(delay.push(500.0, 0.0, 200.0), 0.0);
        assert_eq!(delay.push(550.0, 0.5, 0.0), 0.5);
    }

    #[test]
    fn both_motor_delays_share_one_timeline() {
        let mut primary = OutputDelay::default();
        let mut secondary = OutputDelay::default();
        for now in [0.0, 16.0, 39.0, 65.0, 82.0, 110.0] {
            let value = if now < 39.0 { 0.8 } else { 0.0 };
            let a = primary.push(now, value, 50.0);
            let b = secondary.push(now, value * 0.5, 50.0);
            assert_eq!(a * 0.5, b);
        }
    }

    #[test]
    fn lowering_ceiling_immediately_bounds_a_queued_peak_and_slew_tail() {
        let mut delay = OutputDelay::default();
        delay.push(0.0, 0.8, 500.0);
        let queued_peak = delay.push(500.0, 0.1, 500.0);
        assert_eq!(queued_peak, 0.8);
        assert_eq!(limit_output(queued_peak, 0.2), 0.2);
        assert_eq!(limit_output(0.7, 0.2), 0.2);
        assert_eq!(limit_output(queued_peak, 0.0), 0.0);
        assert_eq!(limit_output(f32::NAN, 0.8), 0.0);
    }

    #[test]
    fn test_shared_f32_round_trip() {
        let s = SharedF32::new(1.5);
        assert!((s.load() - 1.5).abs() < 1e-6);
        s.store(0.0);
        assert_eq!(s.load(), 0.0);
        s.store(-1.5);
        assert!((s.load() - (-1.5)).abs() < 1e-6);
    }

    #[test]
    fn test_shared_f32_special_values() {
        let s = SharedF32::new(f32::INFINITY);
        assert!(s.load().is_infinite());
        s.store(f32::NEG_INFINITY);
        assert!(s.load().is_infinite());
    }

    #[test]
    fn test_avg_normal() {
        assert!((avg(&[1.0, 2.0, 3.0]) - 2.0).abs() < 1e-6);
        assert!((avg(&[10.0]) - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_avg_empty() {
        assert_eq!(avg(&[]), 0.0);
    }

    #[test]
    fn test_calculate_power_mono() {
        // All 1.0 samples, mono: RMS should be 1.0
        let samples = vec![1.0f32; 100];
        let result = calculate_power(&samples, 1);
        assert_eq!(result.len(), 1);
        assert!((result[0] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_calculate_power_stereo() {
        // Stereo, both channels 1.0: each should be 1.0
        let samples = vec![1.0f32; 200]; // 100 frames, 2 channels
        let result = calculate_power(&samples, 2);
        assert_eq!(result.len(), 2);
        assert!(
            (result[0] - 1.0).abs() < 1e-4,
            "left channel power should be 1.0, got {}",
            result[0]
        );
        assert!(
            (result[1] - 1.0).abs() < 1e-4,
            "right channel power should be 1.0, got {}",
            result[1]
        );
    }

    #[test]
    fn test_calculate_power_silence() {
        let samples = vec![0.0f32; 100];
        let result = calculate_power(&samples, 1);
        assert_eq!(result[0], 0.0);
    }

    #[test]
    fn test_low_pass_basic() {
        let samples = vec![1.0f32; 10];
        let result = low_pass(&samples, Duration::from_millis(10), 0.01, 1);
        assert_eq!(result.len(), 10);
        // Output should converge toward input
        assert!(result[9] > result[0]);
    }

    #[test]
    fn test_low_pass_empty() {
        let result = low_pass(&[], Duration::from_millis(10), 0.01, 1);
        assert!(result.is_empty());
    }

    #[test]
    fn test_min_cutoff() {
        assert_eq!(0.5f32.min_cutoff(0.3), 0.5);
        assert_eq!(0.2f32.min_cutoff(0.3), 0.0);
        assert_eq!(0.3f32.min_cutoff(0.3), 0.3);
    }
}
