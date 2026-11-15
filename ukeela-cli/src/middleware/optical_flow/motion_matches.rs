use std::sync::{Arc, Mutex};

use purecv::{
    core::{Matrix, Point2f, Size2i, TermCriteria, TermType},
    video::calc_optical_flow_pyramid_lk,
};

use crate::{
    POISONED_LOCK_MSG,
    frame::{Frame, MotionMatch},
    middleware::{Middleware, ProcessResult},
};

pub const DEFAULT_MIN_MATCHES: usize = 10;

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

impl Default for MotionMatchesMiddleware {
    fn default() -> Self {
        Self {
            min_matches: DEFAULT_MIN_MATCHES,
            state: Default::default(),
        }
    }
}

impl MotionMatchesMiddleware {
    pub fn new(min_matches: usize) -> Self {
        Self {
            min_matches,
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    async fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let current_grayscale = frame
            .metadata
            .grayscale
            .as_ref()
            .ok_or(anyhow::anyhow!("frame metadata doesn't contain grayscale"))?;

        let window = Size2i::new(21, 21);

        let motion_matches = tokio::runtime::Handle::current()
            .spawn_blocking({
                let state = Arc::clone(&self.state);
                let current_grayscale = Arc::clone(&current_grayscale);
                let feature_points = frame.metadata.feature_points.clone();
                let min_matches = self.min_matches;

                move || {
                    let mut state = state.lock().expect(POISONED_LOCK_MSG);

                    let Some(previous_grayscale) = state.previous_gray.as_ref() else {
                        state.previous_gray = Some(current_grayscale.clone());
                        state.previous_points = feature_points.iter().map(|this| this.pt).collect();

                        return Ok(Vec::new());
                    };

                    if state.previous_points.len() < min_matches {
                        return Ok(Vec::new());
                    }

                    let (tracked, status, _) = calc_optical_flow_pyramid_lk(
                        previous_grayscale,
                        &current_grayscale,
                        &state.previous_points,
                        None,
                        window,
                        3,
                        TermCriteria::new(TermType::Both, 30, 0.01),
                        0,
                        1e-4,
                    )?;

                    let motion_matches = state
                        .previous_points
                        .iter()
                        .zip(tracked.iter())
                        .zip(status.iter())
                        .filter_map(|((&previous, &current), &status)| {
                            (status != 0
                                && current.x.is_finite()
                                && current.y.is_finite()
                                && current.x >= 0.0
                                && current.y >= 0.0
                                && current.x < current_grayscale.cols as f32
                                && current.y < current_grayscale.rows as f32)
                                .then_some(MotionMatch { previous, current })
                        })
                        .collect::<Vec<_>>();

                    Result::<_, anyhow::Error>::Ok(motion_matches)
                }
            })
            .await??;

        frame.metadata.motion_matches = motion_matches;

        let mut state = self.state.lock().expect(POISONED_LOCK_MSG);

        state.previous_gray = Some(Arc::clone(&current_grayscale));
        state.previous_points = frame
            .metadata
            .feature_points
            .iter()
            .map(|this| this.pt)
            .collect();

        Ok(frame)
    }
}

impl Middleware for MotionMatchesMiddleware {
    fn name(&self) -> &'static str {
        "optical_flow"
    }

    async fn process_async(&self, frame: crate::frame::Frame) -> ProcessResult {
        self.process_frame(frame).await.map_err(Into::into)
    }
}
