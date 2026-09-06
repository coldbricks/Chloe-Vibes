//! Assemble exact analysis hops independently of Windows packet/poll sizes.

use std::collections::VecDeque;

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
