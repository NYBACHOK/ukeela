use super::*;

#[derive(Debug, Clone, Copy)]
pub struct OpticalFlowLkFeatureDetectionMiddleware {
    max_features: usize,
    distance_to_border: u32,
}

impl OpticalFlowLkFeatureDetectionMiddleware {
    #[must_use]
    pub fn new(config: FeatureDetectionConfig) -> Self {
        Self {
            max_features: config.num_features.get(),
            distance_to_border: config.distance_to_border,
        }
    }

    fn process_frame(&self, mut frame: Frame) -> anyhow::Result<Frame> {
        let grayscale = frame
            .metadata
            .grayscale
            .as_ref()
            .ok_or(anyhow::anyhow!("frame metadata doesn't contain grayscale"))?
            .clone();

        let grayscale = gray_image_from_matrix(&grayscale)?;
        let width = grayscale.width();
        let height = grayscale.height();
        let points = good_features_to_track(&grayscale, 0.01, 0)
            .into_iter()
            .filter(|corner| {
                corner.x >= self.distance_to_border
                    && corner.y >= self.distance_to_border
                    && corner.x < width.saturating_sub(self.distance_to_border)
                    && corner.y < height.saturating_sub(self.distance_to_border)
            })
            .take(self.max_features)
            .map(|corner| {
                KeyPoint::new(
                    Point2f::new(corner.x as f32, corner.y as f32),
                    7.0,
                    -1.0,
                    corner.score,
                    0,
                    -1,
                )
            })
            .collect();

        frame.metadata.feature_points = points;

        Ok(frame)
    }
}

impl Middleware for OpticalFlowLkFeatureDetectionMiddleware {
    fn name(&self) -> &'static str {
        "of_feature_detection_lk"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        tokio::runtime::Handle::current()
            .spawn_blocking({
                let this = *self;

                move || this.process_frame(frame)
            })
            .await?
            .map_err(Into::into)
    }

    fn process(&self, frame: ukeela_common::Frame) -> ukeela_common::ProcessResult {
        self.process_frame(frame).map_err(Into::into)
    }
}
