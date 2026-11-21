use super::*;

#[derive(Clone)]
pub struct OpticalFlowLkMatchesMiddleware {
    min_matches: usize,
    state: Arc<Mutex<TrackerState>>,
}

struct TrackerState {
    tracker: TrackerContext,
    previous_dimensions: Option<(u32, u32)>,
    previous_points: Vec<(f32, f32)>,
}

impl OpticalFlowLkMatchesMiddleware {
    #[must_use]
    pub fn new(min_matches: usize) -> Self {
        Self {
            min_matches,
            state: Arc::new(Mutex::new(TrackerState {
                tracker: TrackerContext::new(),
                previous_dimensions: None,
                previous_points: Vec::new(),
            })),
        }
    }

    fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let grayscale = frame
            .metadata
            .grayscale
            .as_ref()
            .ok_or(anyhow::anyhow!("frame metadata doesn't contain grayscale"))?
            .clone();
        let feature_points: Vec<_> = frame
            .metadata
            .feature_points
            .iter()
            .map(|point| (point.pt.x, point.pt.y))
            .collect();
        let min_matches = self.min_matches;

        let grayscale = gray_image_from_matrix(&grayscale)?;
        let dimensions = grayscale.dimensions();
        let mut state = self.state.acquire_lock();
        state.tracker.advance(&grayscale, PYRAMID_LEVELS);
        let previous_points = std::mem::take(&mut state.previous_points);

        let matches = if state.previous_dimensions == Some(dimensions)
            && previous_points.len() >= min_matches
        {
            let params = TrackParams {
                window_size: WINDOW_SIZE,
                max_iterations: MAX_ITERATIONS,
                min_eigen_threshold: DEFAULT_MIN_EIGEN_THRESHOLD,
            };

            state
                .tracker
                .track_fb(&previous_points, None, params, DEFAULT_FB_THRESHOLD)
                .iter()
                .zip(&previous_points)
                .filter_map(|(tracked, &(x, y))| {
                    let (current_x, current_y) = tracked.pos;
                    (tracked.status == TrackStatus::Tracked
                        && point_is_in_bounds(current_x, current_y, dimensions))
                    .then_some(MotionMatch {
                        previous: Point2f::new(x, y),
                        current: Point2f::new(current_x, current_y),
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        state.previous_dimensions = Some(dimensions);
        state.previous_points = feature_points;

        frame.metadata.motion_matches = matches;
        Ok(frame)
    }
}

impl fmt::Debug for OpticalFlowLkMatchesMiddleware {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpticalFlowLkMatchesMiddleware")
            .field("min_matches", &self.min_matches)
            .finish_non_exhaustive()
    }
}

impl Middleware for OpticalFlowLkMatchesMiddleware {
    fn name(&self) -> &'static str {
        "of_motion_tracking_lk"
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
        self.process_frame(frame).map_err(Into::into)
    }
}
