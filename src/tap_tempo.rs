//! Session-only tempo input. Taps never represent audio onsets or motor commands.
#[derive(Default)]
pub struct TapTempo {
    last_ms: Option<f64>,
    intervals: Vec<f64>,
    pub bpm: Option<f32>,
}

impl TapTempo {
    pub fn tap(&mut self, now_ms: f64) {
        if !now_ms.is_finite() {
            return;
        }
        if let Some(last) = self.last_ms {
            let interval = now_ms - last;
            if (0.0..200.0).contains(&interval) {
                return;
            } // accidental double click
            if (200.0..=2000.0).contains(&interval) {
                self.intervals.push(interval);
                if self.intervals.len() > 7 {
                    self.intervals.remove(0);
                }
                if self.intervals.len() >= 3 {
                    let mut ordered = self.intervals.clone();
                    ordered.sort_by(f64::total_cmp);
                    let middle = ordered.len() / 2;
                    let median = if ordered.len().is_multiple_of(2) {
                        (ordered[middle - 1] + ordered[middle]) * 0.5
                    } else {
                        ordered[middle]
                    };
                    self.bpm = Some((60_000.0 / median) as f32);
                }
            } else {
                self.reset();
            }
        }
        self.last_ms = Some(now_ms);
    }

    pub fn count(&self) -> usize {
        self.intervals.len() + usize::from(self.last_ms.is_some())
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn four_taps_reject_bounce_and_jitter_then_restart_after_a_pause() {
        let mut tap = TapTempo::default();
        for time in [1000.0, 1480.0, 1500.0, 1965.0] {
            tap.tap(time);
        }
        assert_eq!(tap.count(), 3);
        assert!(tap.bpm.is_none());
        tap.tap(2440.0);
        assert_eq!(tap.bpm, Some(125.0));
        tap.tap(f64::NAN);
        assert_eq!(tap.bpm, Some(125.0));
        tap.tap(6000.0);
        assert_eq!(tap.count(), 1);
        assert!(tap.bpm.is_none());
        for time in [6600.0, 7200.0, 7800.0] {
            tap.tap(time);
        }
        assert_eq!(tap.bpm, Some(100.0));
        tap.reset();
        assert_eq!(tap.count(), 0);
        assert!(tap.bpm.is_none());
    }
}
