//! Tracks acknowledged actuator commands and the earliest next dispatch time.

use std::time::Duration;

pub const DEVICE_UPDATE_INTERVAL: Duration = Duration::from_millis(20);

/// A transport acknowledgment does not prove that a motor stopped. Repeat
/// a quiet-input stop three times, without flooding BLE or delaying resume.
pub struct QuietStopSchedule {
    remaining: u8,
    next_at: Duration,
}

impl Default for QuietStopSchedule {
    fn default() -> Self {
        Self {
            remaining: 3,
            next_at: Duration::ZERO,
        }
    }
}

impl QuietStopSchedule {
    pub fn due(&self, now: Duration, unacknowledged: bool) -> bool {
        (self.remaining > 0 || unacknowledged) && now >= self.next_at
    }
    pub fn attempted(&mut self, now: Duration, success: bool) {
        if success {
            self.remaining = self.remaining.saturating_sub(1);
        }
        self.next_at = now + Duration::from_millis(100);
    }
}

/// Output carries the capture generation that produced it. A fresh heartbeat
/// from a replacement source must never authorize an older GUI frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OutputFrame {
    pub levels: [f32; 2],
    pub capture_epoch: u64,
}

impl OutputFrame {
    pub fn new(levels: [f32; 2], capture_epoch: u64) -> Self {
        Self {
            levels,
            capture_epoch,
        }
    }

    pub fn stopped(capture_epoch: u64) -> Self {
        Self::new([0.0, 0.0], capture_epoch)
    }

    pub fn levels_for_epoch(self, current_epoch: u64) -> [f32; 2] {
        if self.capture_epoch == current_epoch {
            self.levels
        } else {
            [0.0, 0.0]
        }
    }
}

/// Pace actual command starts, not reads of unchanged output. The caller owns
/// the monotonic clock and awaits each command before asking for another slot.
#[derive(Default)]
pub struct DeviceDispatchSchedule {
    next_send_at: Duration,
}

impl DeviceDispatchSchedule {
    pub fn delay(&self, now: Duration) -> Duration {
        self.next_send_at.saturating_sub(now)
    }

    /// Failed commands still used a transport slot and must remain rate limited.
    pub fn started(&mut self, now: Duration) {
        self.next_send_at = now.saturating_add(DEVICE_UPDATE_INTERVAL);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceOutput {
    pub vibrators: Vec<f64>,
    pub oscillators: Vec<f64>,
}

impl DeviceOutput {
    fn levels(&self) -> impl Iterator<Item = &f64> {
        self.vibrators.iter().chain(&self.oscillators)
    }
}

/// Apply actuator tuning with a final ceiling, including after gain.
pub fn actuator_output(input: f32, multiplier: f32, min: f32, max: f32, enabled: bool) -> f64 {
    if !enabled || ![input, multiplier, min, max].iter().all(|v| v.is_finite()) {
        return 0.0;
    }
    let level = (input.max(0.0) * multiplier.max(0.0)).clamp(0.0, max.clamp(0.0, 1.0));
    if level < min.clamp(0.0, 1.0) {
        0.0
    } else {
        f64::from(level)
    }
}

#[derive(Default)]
pub struct DeviceDispatchState {
    acknowledged: Option<DeviceOutput>,
    retry_pending: bool,
}

impl DeviceDispatchState {
    pub fn should_send(&self, requested: &DeviceOutput) -> bool {
        if self.retry_pending {
            return true;
        }
        let Some(previous) = &self.acknowledged else {
            return true;
        };
        requested.vibrators.len() != previous.vibrators.len()
            || requested.oscillators.len() != previous.oscillators.len()
            || requested
                .levels()
                .zip(previous.levels())
                .any(|(next, old)| (next - old).abs() >= 0.005 || (*next == 0.0 && *old > 0.0))
    }

    /// Success means every requested actuator command completed successfully.
    pub fn complete(&mut self, requested: &DeviceOutput, success: bool) {
        if success {
            self.acknowledged = Some(requested.clone());
        }
        self.retry_pending = !success;
    }

    pub fn needs_stop(&self) -> bool {
        self.retry_pending
            || self
                .acknowledged
                .as_ref()
                .is_some_and(|output| output.levels().any(|level| *level > 0.0))
    }

    pub fn confirm_stop(&mut self) {
        if let Some(output) = self.acknowledged.as_mut() {
            output.vibrators.fill(0.0);
            output.oscillators.fill(0.0);
        }
        self.retry_pending = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_stops_are_reinforced_and_failures_remain_retryable() {
        let mut stop = QuietStopSchedule::default();
        for ms in [0, 100, 200] {
            assert!(stop.due(Duration::from_millis(ms), false));
            stop.attempted(Duration::from_millis(ms), true);
            assert!(!stop.due(Duration::from_millis(ms + 1), false));
        }
        assert!(!stop.due(Duration::from_secs(1), false));
        assert!(stop.due(Duration::from_secs(1), true));
        stop.attempted(Duration::from_secs(1), false);
        assert!(stop.due(Duration::from_millis(1100), true));
        assert!(QuietStopSchedule::default().due(Duration::ZERO, false));
    }

    fn output(primary: f64, secondary: f64) -> DeviceOutput {
        DeviceOutput {
            vibrators: vec![primary, secondary],
            oscillators: vec![],
        }
    }

    fn attempt_at(
        schedule: &mut DeviceDispatchSchedule,
        state: &mut DeviceDispatchState,
        now_ms: u64,
        requested: &DeviceOutput,
        success: bool,
    ) -> bool {
        let now = Duration::from_millis(now_ms);
        if !state.should_send(requested) || !schedule.delay(now).is_zero() {
            return false;
        }
        schedule.started(now);
        state.complete(requested, success);
        true
    }

    #[test]
    fn onset_after_idle_uses_last_command_time_not_last_poll() {
        let mut schedule = DeviceDispatchSchedule::default();
        let mut state = DeviceDispatchState::default();
        let rest = output(0.0, 0.0);
        assert!(attempt_at(&mut schedule, &mut state, 0, &rest, true));
        for now in [20, 40, 60, 80, 100] {
            assert!(!attempt_at(&mut schedule, &mut state, now, &rest, true));
        }
        assert!(attempt_at(
            &mut schedule,
            &mut state,
            101,
            &output(0.8, 0.3),
            true
        ));
    }

    #[test]
    fn rapid_updates_coalesce_to_latest_and_preserve_command_start_spacing() {
        let mut schedule = DeviceDispatchSchedule::default();
        let mut state = DeviceDispatchState::default();
        let mut sent = Vec::new();
        for now in 0..=80 {
            let latest = output(now as f64 / 100.0, 0.0);
            if attempt_at(&mut schedule, &mut state, now, &latest, true) {
                sent.push((now, latest.vibrators[0]));
            }
        }
        assert_eq!(
            sent,
            vec![(0, 0.0), (20, 0.2), (40, 0.4), (60, 0.6), (80, 0.8)]
        );
    }

    #[test]
    fn a_peak_replaced_before_its_slot_is_not_replayed_after_rest() {
        let mut schedule = DeviceDispatchSchedule::default();
        let mut state = DeviceDispatchState::default();
        let rest = output(0.0, 0.0);
        assert!(attempt_at(&mut schedule, &mut state, 0, &rest, true));
        assert!(!attempt_at(
            &mut schedule,
            &mut state,
            5,
            &output(0.9, 0.0),
            true
        ));
        assert!(!attempt_at(&mut schedule, &mut state, 8, &rest, true));
        assert!(!attempt_at(&mut schedule, &mut state, 20, &rest, true));
        assert!(attempt_at(
            &mut schedule,
            &mut state,
            101,
            &output(0.7, 0.0),
            true
        ));
        assert!(!attempt_at(&mut schedule, &mut state, 102, &rest, true));
        assert!(attempt_at(&mut schedule, &mut state, 121, &rest, true));
    }

    #[test]
    fn failed_commands_keep_their_slot_and_retry_the_latest_zero() {
        let mut schedule = DeviceDispatchSchedule::default();
        let mut state = DeviceDispatchState::default();
        let rest = output(0.0, 0.0);
        assert!(attempt_at(
            &mut schedule,
            &mut state,
            0,
            &output(0.8, 0.3),
            false
        ));
        assert!(state.needs_stop());
        assert!(!attempt_at(&mut schedule, &mut state, 7, &rest, false));
        assert!(attempt_at(&mut schedule, &mut state, 20, &rest, false));
        assert!(state.needs_stop());
        assert!(!attempt_at(&mut schedule, &mut state, 39, &rest, true));
        assert!(attempt_at(&mut schedule, &mut state, 40, &rest, true));
        assert!(!state.needs_stop());
        assert!(!attempt_at(&mut schedule, &mut state, 60, &rest, true));
    }

    #[test]
    fn a_slow_command_does_not_add_an_extra_interval_after_completion() {
        let mut schedule = DeviceDispatchSchedule::default();
        let mut state = DeviceDispatchState::default();
        schedule.started(Duration::ZERO);
        // The in-flight command finishes at 75ms; output changed meanwhile.
        state.complete(&output(0.3, 0.0), true);
        assert!(attempt_at(
            &mut schedule,
            &mut state,
            75,
            &output(0.8, 0.2),
            true
        ));
        assert!(!attempt_at(
            &mut schedule,
            &mut state,
            94,
            &output(0.0, 0.0),
            true
        ));
        assert!(attempt_at(
            &mut schedule,
            &mut state,
            95,
            &output(0.0, 0.0),
            true
        ));
    }

    #[test]
    fn settings_only_output_changes_receive_the_same_rate_limited_slot() {
        let mut schedule = DeviceDispatchSchedule::default();
        let mut state = DeviceDispatchState::default();
        assert!(attempt_at(
            &mut schedule,
            &mut state,
            0,
            &output(0.8, 0.3),
            true
        ));
        // Audio stays fixed while a per-actuator ceiling changes.
        let tuned = output(actuator_output(0.8, 1.0, 0.0, 0.2, true), 0.3);
        assert!(!attempt_at(&mut schedule, &mut state, 8, &tuned, true));
        assert!(attempt_at(&mut schedule, &mut state, 20, &tuned, true));
    }

    #[test]
    fn watch_publication_broadcasts_coherent_latest_values_and_keeps_zero() {
        let (tx, mut first) = tokio::sync::watch::channel(OutputFrame::stopped(1));
        let mut second = tx.subscribe();
        tx.send_replace(OutputFrame::new([0.8, 0.3], 1));
        assert!(first.has_changed().unwrap());
        assert_eq!(*first.borrow_and_update(), OutputFrame::new([0.8, 0.3], 1));
        tx.send_replace(OutputFrame::stopped(1));
        assert_eq!(*second.borrow_and_update(), OutputFrame::stopped(1));
        assert!(first.has_changed().unwrap());
        assert_eq!(*first.borrow_and_update(), OutputFrame::stopped(1));
        assert!(!first.has_changed().unwrap());
        // A device attached after an idle period also sees the current output.
        tx.send_replace(OutputFrame::new([0.6, 0.1], 1));
        assert_eq!(*tx.subscribe().borrow(), OutputFrame::new([0.6, 0.1], 1));
    }

    #[test]
    fn a_late_old_frame_stops_instead_of_using_a_new_sources_fresh_heartbeat() {
        use std::sync::{
            atomic::{AtomicU64, Ordering},
            mpsc, Arc,
        };

        let current_epoch = Arc::new(AtomicU64::new(1));
        let (tx, mut received) = tokio::sync::watch::channel(OutputFrame::stopped(1));
        let (started_tx, started_rx) = mpsc::sync_channel(0);
        let (resume_tx, resume_rx) = mpsc::sync_channel(0);
        let producer_epoch = current_epoch.clone();
        let producer = std::thread::spawn(move || {
            // A GUI frame has already consumed the previous source's spectrum.
            let frame = OutputFrame::new([0.8, 0.3], producer_epoch.load(Ordering::Relaxed));
            started_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
            tx.send_replace(frame);
        });
        started_rx.recv().unwrap();
        // Replacement capture is live before the old GUI frame finishes.
        current_epoch.store(2, Ordering::Relaxed);
        resume_tx.send(()).unwrap();
        producer.join().unwrap();

        let frame = *received.borrow_and_update();
        assert_eq!(frame.capture_epoch, 1);
        let [primary, secondary] = frame.levels_for_epoch(current_epoch.load(Ordering::Relaxed));
        assert_eq!([primary, secondary], [0.0, 0.0]);
        let stopped = output(f64::from(primary), f64::from(secondary));
        let mut schedule = DeviceDispatchSchedule::default();
        let mut state = DeviceDispatchState::default();
        assert!(attempt_at(
            &mut schedule,
            &mut state,
            0,
            &output(0.8, 0.3),
            true
        ));
        // Mismatch uses the ordinary available slot, not the 250ms watchdog wait.
        assert!(attempt_at(&mut schedule, &mut state, 101, &stopped, false));
        assert!(state.needs_stop());
        assert!(!attempt_at(&mut schedule, &mut state, 120, &stopped, true));
        assert!(attempt_at(&mut schedule, &mut state, 121, &stopped, true));
        assert!(!state.needs_stop());
    }

    #[test]
    fn matching_generation_passes_and_stopped_frames_are_always_zero() {
        let frame = OutputFrame::new([0.8, 0.3], 7);
        assert_eq!(frame.levels_for_epoch(7), [0.8, 0.3]);
        assert_eq!(frame.levels_for_epoch(6), [0.0, 0.0]);
        assert_eq!(frame.levels_for_epoch(8), [0.0, 0.0]);
        for epoch in [6, 7, 8] {
            assert_eq!(OutputFrame::stopped(7).levels_for_epoch(epoch), [0.0, 0.0]);
        }
    }

    #[test]
    fn identical_levels_from_a_new_generation_are_a_new_watch_publication() {
        let (tx, mut received) = tokio::sync::watch::channel(OutputFrame::new([0.6, 0.1], 1));
        let current = OutputFrame::new([0.6, 0.1], 2);
        assert!(tx.send_if_modified(|previous| {
            if *previous == current {
                false
            } else {
                *previous = current;
                true
            }
        }));
        assert!(received.has_changed().unwrap());
        assert_eq!(received.borrow_and_update().levels_for_epoch(2), [0.6, 0.1]);
    }

    #[test]
    fn unchanged_failed_output_retries_until_acknowledged() {
        let mut state = DeviceDispatchState::default();
        let active = output(0.6, 0.3);
        let mut attempts = 0;
        for success in [false, false, true, true] {
            if state.should_send(&active) {
                attempts += 1;
                state.complete(&active, success);
            }
        }
        assert_eq!(attempts, 3);
        assert!(!state.should_send(&active));
    }

    #[test]
    fn failed_disable_remains_pending_and_watchdog_still_requires_stop() {
        let mut state = DeviceDispatchState::default();
        state.complete(&output(0.6, 0.3), true);
        let stopped = output(0.0, 0.0);
        assert!(state.should_send(&stopped));
        state.complete(&stopped, false);
        assert!(state.should_send(&stopped));
        assert!(state.needs_stop());
        state.complete(&stopped, true);
        assert!(!state.needs_stop());
        assert!(!state.should_send(&stopped));
    }

    #[test]
    fn stopping_either_tiny_channel_bypasses_deadband_and_retries_failed_stop() {
        for (primary, secondary) in [(0.004, 0.002), (0.004, 0.0), (0.0, 0.002)] {
            let mut state = DeviceDispatchState::default();
            state.complete(&output(primary, secondary), true);
            let stopped = output(0.0, 0.0);
            assert!(state.should_send(&stopped));
            state.complete(&stopped, false);
            assert!(state.should_send(&stopped));
            assert!(state.needs_stop());
            state.complete(&stopped, true);
            assert!(!state.should_send(&stopped));
            assert!(!state.needs_stop());
        }
    }

    #[test]
    fn stopping_one_tiny_channel_preserves_the_other_requested_channel() {
        for stopped in [output(0.0, 0.002), output(0.004, 0.0)] {
            let mut state = DeviceDispatchState::default();
            state.complete(&output(0.004, 0.002), true);
            assert!(state.should_send(&stopped));
            state.complete(&stopped, true);
            assert!(!state.should_send(&stopped));
            assert!(state.needs_stop());
        }
    }

    #[test]
    fn failed_or_partial_write_retries_latest_request_even_below_the_deadband() {
        let mut state = DeviceDispatchState::default();
        state.complete(&output(0.6, 0.3), true);
        state.complete(&output(0.6, 0.7), false);
        let latest = output(0.601, 0.3);
        assert!(state.should_send(&latest));
        state.complete(&latest, true);
        assert!(!state.should_send(&output(0.6, 0.3)));
        assert!(state.should_send(&output(0.6, 0.0)));
    }

    #[test]
    fn failed_first_command_is_not_assumed_stopped_and_confirmed_stop_allows_resume() {
        let mut state = DeviceDispatchState::default();
        let active = output(0.6, 0.3);
        state.complete(&active, false);
        assert!(state.needs_stop());
        state.confirm_stop();
        assert!(!state.needs_stop());
        assert!(state.should_send(&active));
    }

    #[test]
    fn tuning_one_actuator_during_steady_note_dispatches_new_value() {
        let mut state = DeviceDispatchState::default();
        let mut note = output(0.6, 0.3);
        note.oscillators.push(0.4);
        state.complete(&note, true);
        note.vibrators[1] = actuator_output(0.3, 0.5, 0.0, 1.0, true);
        assert!(state.should_send(&note));
        state.complete(&note, true);
        note.oscillators[0] = actuator_output(0.4, 1.0, 0.0, 0.2, true);
        assert!(state.should_send(&note));
    }

    #[test]
    fn tiny_oscillator_stop_is_not_lost() {
        let mut state = DeviceDispatchState::default();
        let mut command = DeviceOutput {
            vibrators: vec![],
            oscillators: vec![0.003],
        };
        state.complete(&command, true);
        command.oscillators[0] = 0.0;
        assert!(state.should_send(&command));
        state.complete(&command, false);
        assert!(state.needs_stop());
    }

    #[test]
    fn actuator_limits_bind_after_gain_and_invalid_tuning_stops() {
        assert!((actuator_output(0.8, 5.0, 0.0, 0.2, true) - 0.2).abs() < 1e-6);
        assert_eq!(actuator_output(0.8, 5.0, 0.0, 0.2, false), 0.0);
        assert_eq!(actuator_output(0.8, 1.0, 0.5, -1.0, true), 0.0);
        assert_eq!(actuator_output(0.8, f32::NAN, 0.0, 1.0, true), 0.0);
    }
}
