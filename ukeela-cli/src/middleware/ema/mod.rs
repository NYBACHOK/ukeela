use std::sync::{Arc, Mutex};

use crate::{
    POISONED_LOCK_MSG,
    frame::Frame,
    middleware::{
        Middleware, ProcessResult,
        ema::{
            config::{EmaConfig, MotionVector},
            state::EmaState,
        },
    },
};

pub mod config;
mod state;

#[derive(Debug)]
pub struct EmaMiddleware {
    state: Arc<Mutex<EmaState>>,
}

impl EmaMiddleware {
    pub fn new(config: EmaConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(EmaState::new(config))),
        }
    }

    pub fn get_current_offset(&self) -> (f32, f32) {
        self.state.lock().expect(POISONED_LOCK_MSG).get_offset()
    }

    pub fn set_alpha(&self, alpha: f32) -> Result<(), &'static str> {
        if !(0.0..=1.0).contains(&alpha) {
            return Err("Alpha must be in [0.0, 1.0]");
        }
        self.state.lock().expect(POISONED_LOCK_MSG).config.alpha = alpha;
        Ok(())
    }
}

impl Middleware for EmaMiddleware {
    fn name(&self) -> &'static str {
        "ema_trasform_calc"
    }

    fn has_gpu_support(&self) -> bool {
        false
    }

    fn estimated_latency(&self) -> f32 {
        0.1
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        let motion = {
            let meta = frame.metadata.read().expect(POISONED_LOCK_MSG);
            if meta.motion_vectors.is_none()
                || meta
                    .motion_vectors
                    .as_ref()
                    .is_some_and(|this| this.is_empty())
            {
                std::mem::drop(meta);

                return Ok(frame);
            }

            let mvs = meta.motion_vectors.as_ref().expect("checked above");

            MotionVector {
                dx: mvs.iter().map(|(dx, _)| *dx as f32).sum::<f32>() / mvs.len() as f32,
                dy: mvs.iter().map(|(_, dy)| *dy as f32).sum::<f32>() / mvs.len() as f32,
            }
        };

        // Single motion vector - use scalar path (SIMD overhead > benefit)
        let (offset_x, offset_y) = {
            let mut state = self.state.lock().expect(POISONED_LOCK_MSG);
            state.update_single(&motion)
        };

        let transform = [1.0, 0.0, offset_x, 0.0, 1.0, offset_y];

        frame
            .metadata
            .write()
            .expect(POISONED_LOCK_MSG)
            .estimated_transform = Some(transform);
        Ok(frame)
    }
}
