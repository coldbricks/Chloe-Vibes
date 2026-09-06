//! Tracks acknowledged actuator commands; transport cadence stays in the caller.

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

    fn output(primary: f64, secondary: f64) -> DeviceOutput {
        DeviceOutput {
            vibrators: vec![primary, secondary],
            oscillators: vec![],
        }
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
