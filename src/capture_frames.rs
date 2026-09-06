//! Assemble exact analysis hops independently of Windows packet/poll sizes.

use std::collections::VecDeque;

/// Independent of presets, gain, gate smoothing and ADSR release. A quiet
/// packet is still a fresh packet; freshness alone cannot authorize a motor.
pub struct CaptureActivity {
    quiet_frames: usize,
    quiet_limit: usize,
    channels: usize,
    active: bool,
}

impl CaptureActivity {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        Self {
            quiet_frames: 0,
            quiet_limit: (sample_rate as usize * 40).div_ceil(1000).max(1),
            channels: channels.max(1),
            active: false,
        }
    }

    pub fn push(&mut self, samples: &[f32]) -> bool {
        for frame in samples.chunks_exact(self.channels) {
            if frame.iter().any(|x| x.is_finite() && x.abs() > 0.00001) {
                self.quiet_frames = 0;
                self.active = true;
            } else {
                self.quiet_frames = self.quiet_frames.saturating_add(1);
                if self.quiet_frames >= self.quiet_limit {
                    self.active = false;
                }
            }
        }
        self.active
    }
}

pub struct CaptureFrames {
    samples: VecDeque<f32>,
    channels: usize,
    window_samples: usize,
    hop_frames: usize,
    pending_frames: usize,
    total_frames: u64,
}

impl CaptureFrames {
    pub fn new(channels: usize, window_frames: usize, hop_frames: usize) -> Self {
        assert!(channels > 0 && hop_frames > 0 && hop_frames <= window_frames);
        Self {
            samples: VecDeque::with_capacity(window_frames * channels),
            channels,
            window_samples: window_frames * channels,
            hop_frames,
            pending_frames: 0,
            total_frames: 0,
        }
    }

    pub fn clear(&mut self) {
        self.samples.clear();
        self.pending_frames = 0;
        self.total_frames = 0;
    }

    pub fn push(&mut self, samples: &[f32], mut analyze: impl FnMut(&[f32], u64)) {
        let complete_samples = samples.len() / self.channels * self.channels;
        let mut offset = 0;
        while offset < complete_samples {
            let frames = ((complete_samples - offset) / self.channels)
                .min(self.hop_frames - self.pending_frames);
            let end = offset + frames * self.channels;
            // Evict before extending to keep allocation bounded to one window.
            let remove = (self.samples.len() + end - offset).saturating_sub(self.window_samples);
            self.samples.drain(..remove);
            self.samples.extend(samples[offset..end].iter().copied());
            offset = end;
            self.pending_frames += frames;
            self.total_frames += frames as u64;
            if self.pending_frames == self.hop_frames {
                self.pending_frames = 0;
                analyze(self.samples.make_contiguous(), self.total_frames);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_stops_after_forty_ms_even_with_continuous_packets() {
        for rate in [44100, 48000, 96000] {
            let mut guard = CaptureActivity::new(rate, 2);
            assert!(!guard.push(&[0.0; 100]));
            assert!(guard.push(&[0.0, 0.3])); // Either channel is enough.
            let frames = (rate as usize * 40).div_ceil(1000);
            assert!(guard.push(&vec![0.0; (frames - 1) * 2]));
            assert!(!guard.push(&[0.0, 0.0]));
            assert!(!guard.push(&[f32::NAN, f32::INFINITY]));
            assert!(guard.push(&[0.00002, 0.0])); // Preserve very quiet music.
        }
    }

    #[test]
    fn silence_guard_observes_the_tail_of_a_batched_packet() {
        let mut guard = CaptureActivity::new(48000, 1);
        let mut packet = vec![0.5; 480];
        packet.extend(vec![0.0; 1920]);
        assert!(!guard.push(&packet));
        packet.push(0.1);
        assert!(guard.push(&packet));
    }

    fn windows(packet_frames: &[usize]) -> Vec<(u64, Vec<f32>)> {
        let mut buffer = CaptureFrames::new(2, 2048, 1024);
        let mut output = Vec::new();
        let mut offset = 0;
        for count in packet_frames {
            let samples: Vec<_> = (offset..offset + count)
                .flat_map(|frame| [frame as f32, -(frame as f32)])
                .collect();
            buffer.push(&samples, |window, end| output.push((end, window.to_vec())));
            offset += count;
        }
        output
    }

    #[test]
    fn packet_sizes_and_poll_backlogs_do_not_change_fft_windows() {
        let regular = windows(&[480; 64]);
        let burst = windows(&[30720]);
        assert_eq!(regular, burst);
        assert_eq!(regular.len(), 30);
        for (index, (end, samples)) in regular.iter().enumerate() {
            assert_eq!(*end, (index as u64 + 1) * 1024);
            assert_eq!(samples[samples.len() - 2], (*end - 1) as f32);
            assert_eq!(samples[samples.len() - 1], -((*end - 1) as f32));
        }
    }

    #[test]
    fn device_switch_or_discontinuity_discards_the_old_partial_window() {
        let mut buffer = CaptureFrames::new(1, 8, 4);
        buffer.push(&[9.0; 3], |_, _| panic!("incomplete hop"));
        buffer.clear();
        let mut output = Vec::new();
        buffer.push(&[1.0; 4], |samples, end| {
            output.push((end, samples.to_vec()))
        });
        assert_eq!(output, vec![(4, vec![1.0; 4])]);
    }
}
