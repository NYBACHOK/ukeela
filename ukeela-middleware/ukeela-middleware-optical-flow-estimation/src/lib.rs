use rand::RngExt;
use std::sync::{Arc, Mutex};
use ukeela_common::{
    Frame, Middleware, MotionMatch, ProcessResult, SimilarityTransform,
    extensions::lock::AcquireMutexLock,
};

const RANSAC_ITERATIONS: usize = 2_000;
const RANSAC_THRESHOLD: f32 = 3.0;

#[derive(Debug, Clone)]
pub struct MotionEstimationMiddleware {
    min_matches: usize,
    state: Arc<Mutex<State>>,
}

#[derive(Debug)]
struct State {
    rand: rand::rngs::SmallRng,
}

impl MotionEstimationMiddleware {
    #[must_use]
    pub fn new(min_matches: usize) -> Self {
        Self {
            min_matches,
            state: Arc::new(Mutex::new(State {
                rand: rand::make_rng(),
            })),
        }
    }

    fn process_frame(&self, mut frame: Frame) -> Frame {
        let matches = &frame.metadata.motion_matches;
        if matches.len() < self.min_matches {
            return frame;
        }

        let mut state = self.state.acquire_lock();

        frame.metadata.similarity_transform =
            estimate_motion(matches, self.min_matches, &mut state.rand);

        frame
    }
}

impl Middleware for MotionEstimationMiddleware {
    fn name(&self) -> &'static str {
        "motion_estimation"
    }

    fn process_async(&self, frame: Frame) -> impl std::future::Future<Output = ProcessResult> {
        std::future::ready(Ok(self.process_frame(frame)))
    }

    fn process(&self, frame: Frame) -> ProcessResult {
        Ok(self.process_frame(frame))
    }
}

fn estimate_motion(
    matches: &[MotionMatch],
    min_matches: usize,
    rng: &mut rand::rngs::SmallRng,
) -> Option<SimilarityTransform> {
    let valid_matches: Vec<_> = matches
        .iter()
        .copied()
        .filter(|m| {
            [m.previous.x, m.previous.y, m.current.x, m.current.y]
                .into_iter()
                .all(f32::is_finite)
        })
        .collect();

    if valid_matches.len() < min_matches {
        return None;
    }

    let mut best_model = None;
    let mut best_inlier_count = 0;

    for _ in 0..RANSAC_ITERATIONS {
        let first = rng.random_range(0..valid_matches.len());
        let mut second = rng.random_range(0..valid_matches.len());
        while second == first {
            second = rng.random_range(0..valid_matches.len());
        }

        let Some(candidate) = fit_similarity_minimal(valid_matches[first], valid_matches[second])
        else {
            continue;
        };

        let inlier_count = valid_matches
            .iter()
            .filter(|m| reprojection_error_squared(candidate, **m) <= RANSAC_THRESHOLD.powi(2))
            .count();

        if inlier_count > best_inlier_count {
            best_model = Some(candidate);
            best_inlier_count = inlier_count;
        }
    }

    if best_inlier_count < min_matches {
        return None;
    }

    let mut model = best_model?;
    for _ in 0..2 {
        let inliers: Vec<_> = valid_matches
            .iter()
            .copied()
            .filter(|m| reprojection_error_squared(model, *m) <= RANSAC_THRESHOLD.powi(2))
            .collect();

        if inliers.len() < min_matches {
            return None;
        }
        model = fit_similarity_least_squares(&inliers)?;
    }

    let finite = [model.a, model.b, model.tx, model.ty]
        .into_iter()
        .all(f64::is_finite);
    if !finite || model.a * model.a + model.b * model.b <= f64::EPSILON {
        return None;
    }

    Some(model)
}

fn fit_similarity_minimal(m1: MotionMatch, m2: MotionMatch) -> Option<SimilarityTransform> {
    fit_similarity_least_squares(&[m1, m2])
}

fn fit_similarity_least_squares(matches: &[MotionMatch]) -> Option<SimilarityTransform> {
    if matches.len() < 2 {
        return None;
    }

    let count = matches.len() as f64;
    let (previous_x, previous_y, current_x, current_y) = matches.iter().fold(
        (0.0, 0.0, 0.0, 0.0),
        |(sum_px, sum_py, sum_cx, sum_cy), m| {
            (
                sum_px + f64::from(m.previous.x),
                sum_py + f64::from(m.previous.y),
                sum_cx + f64::from(m.current.x),
                sum_cy + f64::from(m.current.y),
            )
        },
    );

    let (previous_x, previous_y, current_x, current_y) = (
        previous_x / count,
        previous_y / count,
        current_x / count,
        current_y / count,
    );

    let (a_numerator, b_numerator, denominator) =
        matches
            .iter()
            .fold((0.0, 0.0, 0.0), |(sum_a, sum_b, sum_d), m| {
                let x = f64::from(m.previous.x) - previous_x;
                let y = f64::from(m.previous.y) - previous_y;
                let u = f64::from(m.current.x) - current_x;
                let v = f64::from(m.current.y) - current_y;
                (
                    sum_a + x * u + y * v,
                    sum_b + x * v - y * u,
                    sum_d + x * x + y * y,
                )
            });

    if denominator <= f64::EPSILON {
        return None;
    }

    let a = a_numerator / denominator;
    let b = b_numerator / denominator;
    let model = SimilarityTransform {
        a,
        b,
        tx: current_x - a * previous_x + b * previous_y,
        ty: current_y - b * previous_x - a * previous_y,
    };

    [model.a, model.b, model.tx, model.ty]
        .into_iter()
        .all(f64::is_finite)
        .then_some(model)
}

fn reprojection_error_squared(model: SimilarityTransform, matched: MotionMatch) -> f32 {
    let x = f64::from(matched.previous.x);
    let y = f64::from(matched.previous.y);
    let observed_x = f64::from(matched.current.x);
    let observed_y = f64::from(matched.current.y);

    let error_x = model.a * x - model.b * y + model.tx - observed_x;
    let error_y = model.b * x + model.a * y + model.ty - observed_y;
    (error_x * error_x + error_y * error_y) as f32
}
