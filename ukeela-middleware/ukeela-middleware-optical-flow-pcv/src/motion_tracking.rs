use std::sync::{Arc, Mutex};

use purecv::{
    core::{Matrix, Point2f, Size2i, TermCriteria, TermType},
    video::calc_optical_flow_pyramid_lk,
};
use ukeela_common::{
    Frame, Middleware, MotionMatch, ProcessResult, extensions::lock::AcquireMutexLock,
};

#[derive(Debug, Clone)]
pub struct MotionMatchesMiddleware {
    min_matches: usize,
    state: Arc<Mutex<State>>,
}

#[derive(Debug, Default)]
struct State {
    previous_gray: Option<Arc<Matrix<u8>>>,
    previous_points: Vec<Point2f>,
}

impl MotionMatchesMiddleware {
    #[must_use]
    pub fn new(min_matches: usize) -> Self {
        Self {
            min_matches,
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let current_grayscale = frame
            .metadata
            .grayscale
            .as_ref()
            .ok_or(anyhow::anyhow!("frame metadata doesn't contain grayscale"))?;

        let window = Size2i::new(21, 21);

        let mut state = self.state.acquire_lock();

        let motion_matches = match state.previous_gray.as_ref() {
            None => Ok(Vec::new()),
            Some(_) if state.previous_points.len() < self.min_matches => Ok(Vec::new()),
            Some(previous_grayscale) => {
                let previous_points: Vec<_> = state
                    .previous_points
                    .iter()
                    .copied()
                    .filter(|point| {
                        point_is_in_bounds(*point, previous_grayscale.cols, previous_grayscale.rows)
                    })
                    .collect();

                if previous_points.len() < self.min_matches {
                    Ok(Vec::new())
                } else {
                    calc_optical_flow_pyramid_lk(
                        previous_grayscale,
                        &current_grayscale,
                        &previous_points,
                        None,
                        window,
                        3,
                        TermCriteria::new(TermType::Both, 30, 0.01),
                        0,
                        1e-4,
                    )
                    .map_err(anyhow::Error::from)
                    .map(|(tracked, status, _)| {
                        previous_points
                            .iter()
                            .zip(tracked.iter())
                            .zip(status.iter())
                            .filter_map(|((&previous, &current), &status)| {
                                (status != 0
                                    && point_is_in_bounds(
                                        current,
                                        current_grayscale.cols,
                                        current_grayscale.rows,
                                    ))
                                .then_some(MotionMatch { previous, current })
                            })
                            .collect::<Vec<_>>()
                    })
                }
            }
        }?;

        state.previous_gray = Some(current_grayscale.clone());
        state.previous_points = frame
            .metadata
            .feature_points
            .iter()
            .map(|this| this.pt)
            .collect();

        frame.metadata.motion_matches = motion_matches;

        Ok(frame)
    }
}

fn point_is_in_bounds(point: Point2f, width: usize, height: usize) -> bool {
    point.x.is_finite()
        && point.y.is_finite()
        && point.x >= 0.0
        && point.y >= 0.0
        && point.x < width as f32
        && point.y < height as f32
}

impl Middleware for MotionMatchesMiddleware {
    fn name(&self) -> &'static str {
        "of_motion_tracking_pcv"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        tokio::runtime::Handle::current()
            .spawn_blocking({
                let this = self.clone();

                move || this.process_frame(frame)
            })
            .await?
            .map_err(Into::into)
    }

    fn process(&self, frame: Frame) -> ProcessResult {
        self.process_frame(frame)
    }
}
