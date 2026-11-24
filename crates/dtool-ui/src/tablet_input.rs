#[cfg(windows)]
mod windows {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};

    #[derive(Clone, Copy)]
    pub struct PenSample {
        pub id: u64,
        pub phase: egui::TouchPhase,
        pub client_px: [i32; 2],
        pub pressure: Option<f32>,
        pub tilt: [f32; 2],
        pub time_ms: u32,
    }

    thread_local! {
        static SAMPLES: RefCell<VecDeque<(Instant, PenSample)>> = const { RefCell::new(VecDeque::new()) };
    }

    pub fn push(sample: PenSample) {
        SAMPLES.with_borrow_mut(|samples| {
            if samples.len() >= 1024 {
                samples.pop_front();
            }
            samples.push_back((Instant::now(), sample));
        });
    }

    pub(crate) fn consume(
        id: egui::TouchId,
        phase: egui::TouchPhase,
        pos: egui::Pos2,
        pixels_per_point: f32,
    ) -> Option<PenSample> {
        SAMPLES.with_borrow_mut(|samples| {
            while samples
                .front()
                .is_some_and(|(time, _)| time.elapsed() > Duration::from_millis(500))
            {
                samples.pop_front();
            }
            let [x, y] = [pos.x, pos.y].map(|value| value * pixels_per_point);
            let index = samples.iter().position(|(_, sample)| {
                sample.id == id.0
                    && sample.phase == phase
                    && ((sample.client_px[0] as f32 - x).abs() <= 2.0)
                    && ((sample.client_px[1] as f32 - y).abs() <= 2.0)
            })?;
            samples.remove(index).map(|(_, sample)| sample)
        })
    }
}

#[cfg(windows)]
pub(crate) use windows::consume;
#[cfg(windows)]
pub use windows::{PenSample, push};
