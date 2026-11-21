use std::num::NonZero;

use ukeela_common::{define_middleware_dispatch, types::Percent};
use ukeela_middleware_grayscale::GrayscaleMiddleware;
use ukeela_middleware_optical_flow_estimation::MotionEstimationMiddleware;
use ukeela_middleware_optical_flow_lk::{
    FeatureDetectionConfig as OpticalFlowLkFeatureDetectionConfig,
    feature_detection::OpticalFlowLkFeatureDetectionMiddleware,
    motion_tracking::OpticalFlowLkMatchesMiddleware,
};
use ukeela_middleware_optical_flow_overlay::{
    feature_detection::FeaturePointOverlayMiddleware,
    motion_tracking::MotionMatchesOverlayMiddleware,
};
use ukeela_middleware_optical_flow_pcv::{
    feature_detection::{
        FeatureDetectionConfig, fast::FastFeatureDetectionMiddleware,
        orb::OrbFeatureDetectionMiddleware,
    },
    motion_tracking::MotionMatchesMiddleware,
};
use ukeela_middleware_resize::ResizeMiddleware;
use ukeela_middleware_stabilization::StabilizationMiddleware;

use crate::args::{FeaturePointsMethod, ProcessingFlags};

define_middleware_dispatch! {
    #[derive(Debug)]
    pub(crate) enum MiddlewareDispatch {
        Fast(FastFeatureDetectionMiddleware),
        Orb(OrbFeatureDetectionMiddleware),
        FeaturePointOverlay(FeaturePointOverlayMiddleware),
        Grayscale(GrayscaleMiddleware),
        Resize(ResizeMiddleware),
        MotionMatches(MotionMatchesMiddleware),
        OpticalFlowLkFeatureDetection(OpticalFlowLkFeatureDetectionMiddleware),
        OpticalFlowLkMatches(OpticalFlowLkMatchesMiddleware),
        MotionMatchesOverlay(MotionMatchesOverlayMiddleware),
        MotionEstimation(MotionEstimationMiddleware),
        Stabilization(StabilizationMiddleware),
    }
}

#[derive(Debug, Clone, Copy)]
struct FeatureDetectionFlags {
    num_features: NonZero<usize>,
    distance_to_border: u32,
    fast_threshold: u8,
}

impl From<FeatureDetectionFlags> for FeatureDetectionConfig {
    fn from(flags: FeatureDetectionFlags) -> Self {
        Self {
            num_features: flags.num_features,
            distance_to_border: flags.distance_to_border,
            fast_threshold: flags.fast_threshold,
        }
    }
}

impl From<FeatureDetectionFlags> for OpticalFlowLkFeatureDetectionConfig {
    fn from(flags: FeatureDetectionFlags) -> Self {
        Self {
            num_features: flags.num_features,
            distance_to_border: flags.distance_to_border,
        }
    }
}

pub(crate) fn build_processing_chain(
    ProcessingFlags {
        scale,
        crop,
        smoothing_factor,
        display_stabilization,
        features_method,
        filter_type,
        display_features,
        display_motion_vectors,
        optical_flow_lk,
        num_features,
        distance_to_border,
        fast_threshold,
        motion_min_matches,
    }: ProcessingFlags,
) -> Result<Vec<MiddlewareDispatch>, anyhow::Error> {
    let feature_frame_scale =
        Percent::try_new(f32::from(scale) / 100.0).ok_or(anyhow::anyhow!("invalid frame scale"))?;
    let stabilization_crop =
        Percent::try_new(f32::from(crop) / 100.0).ok_or(anyhow::anyhow!("invalid frame scale"))?;
    let stabilization_smoothing = Percent::try_new(f32::from(smoothing_factor) / 100.0)
        .ok_or(anyhow::anyhow!("invalid stabilization smoothing factor"))?;

    let mut middlewares = vec![
        MiddlewareDispatch::Resize(ResizeMiddleware::new(
            feature_frame_scale,
            fast_image_resize::ResizeAlg::Convolution(filter_type.into()),
        )),
        MiddlewareDispatch::Grayscale(GrayscaleMiddleware::new()),
    ];

    let feature_detection_flags = FeatureDetectionFlags {
        num_features,
        distance_to_border,
        fast_threshold,
    };

    if optical_flow_lk {
        let config = feature_detection_flags.into();
        middlewares.push(MiddlewareDispatch::OpticalFlowLkFeatureDetection(
            OpticalFlowLkFeatureDetectionMiddleware::new(config),
        ));
    } else {
        let config = FeatureDetectionConfig::from(feature_detection_flags);
        match features_method {
            FeaturePointsMethod::Fast => middlewares.push(MiddlewareDispatch::Fast(
                FastFeatureDetectionMiddleware::new(config),
            )),
            FeaturePointsMethod::Orb => middlewares.push(MiddlewareDispatch::Orb(
                OrbFeatureDetectionMiddleware::new(config),
            )),
        }
    }

    if display_features {
        middlewares.push(MiddlewareDispatch::FeaturePointOverlay(
            FeaturePointOverlayMiddleware::new(),
        ));
    }

    middlewares.push(if optical_flow_lk {
        MiddlewareDispatch::OpticalFlowLkMatches(OpticalFlowLkMatchesMiddleware::new(
            motion_min_matches,
        ))
    } else {
        MiddlewareDispatch::MotionMatches(MotionMatchesMiddleware::new(motion_min_matches))
    });

    middlewares.push(MiddlewareDispatch::MotionEstimation(
        MotionEstimationMiddleware::new(motion_min_matches),
    ));

    if display_motion_vectors {
        middlewares.push(MiddlewareDispatch::MotionMatchesOverlay(
            MotionMatchesOverlayMiddleware::new(),
        ));
    }

    middlewares.push(MiddlewareDispatch::Stabilization(
        StabilizationMiddleware::new(
            display_stabilization,
            stabilization_crop,
            stabilization_smoothing,
        ),
    ));

    Ok(middlewares)
}

#[cfg(test)]
mod tests {
    use std::num::NonZero;

    use ukeela_common::Middleware;

    use crate::args::{DEFAULT_MIN_MATCHES, FeaturePointsMethod, FilterType, ProcessingFlags};

    use super::build_processing_chain;

    fn flags(optical_flow_lk: bool) -> ProcessingFlags {
        ProcessingFlags {
            scale: 25,
            crop: 5,
            smoothing_factor: 50,
            display_stabilization: false,
            features_method: FeaturePointsMethod::Fast,
            filter_type: FilterType::Bilinear,
            display_features: false,
            display_motion_vectors: false,
            optical_flow_lk,
            num_features: NonZero::new(100).expect("positive feature count"),
            distance_to_border: 50,
            fast_threshold: 30,
            motion_min_matches: DEFAULT_MIN_MATCHES,
        }
    }

    #[test]
    fn optical_flow_lk_flag_selects_matching_detection_and_tracking_middleware() {
        let standard_names: Vec<_> = build_processing_chain(flags(false))
            .expect("standard pipeline is valid")
            .iter()
            .map(Middleware::name)
            .collect();
        assert!(standard_names.contains(&"of_feature_detection_pcv_fast"));
        assert!(standard_names.contains(&"of_motion_tracking_pcv"));
        assert!(!standard_names.contains(&"of_feature_detection_lk"));
        assert!(!standard_names.contains(&"of_motion_tracking_lk"));

        let optical_flow_names: Vec<_> = build_processing_chain(flags(true))
            .expect("optical-flow pipeline is valid")
            .iter()
            .map(Middleware::name)
            .collect();
        assert!(optical_flow_names.contains(&"of_feature_detection_lk"));
        assert!(optical_flow_names.contains(&"of_motion_tracking_lk"));
        assert!(optical_flow_names.contains(&"motion_estimation"));
        assert!(!optical_flow_names.contains(&"of_feature_detection_pcv_fast"));
        assert!(!optical_flow_names.contains(&"of_motion_tracking_pcv"));
    }
}
