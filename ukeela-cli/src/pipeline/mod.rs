pub mod gst;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use tokio::sync::{broadcast, mpsc::channel, watch};

use crate::{
    frame::Frame,
    middleware::chain::ProcessingChain,
    pipeline::{input::InputHandle, output::OutputHandle},
};

pub mod input;
pub mod output;

#[derive(Debug, Clone)]
pub struct FramePipeline {
    shutdown_tx: broadcast::Sender<()>,
}

impl FramePipeline {
    /// Create a pipeline with a bounded output queue and paced latest-frame ingress.
    pub fn new(
        processing_chain: ProcessingChain,
        channel_size: usize,
    ) -> (
        Self,
        InputHandle,
        OutputHandle,
        broadcast::Receiver<PipelineStats>,
    ) {
        let (ingress_tx, mut ingress_rx) = watch::channel((0, Frame::default()));
        let (output_tx, output_rx) = channel(channel_size);
        let (stats_tx, stats_rx) = broadcast::channel(16);
        let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);

        let pipeline = FramePipeline {
            shutdown_tx: shutdown_tx.clone(),
        };

        let input_handle = InputHandle {
            tx: ingress_tx.clone(),
            sequence: 0,
            shutdown_rx: shutdown_tx.subscribe(),
        };

        let output_handle = OutputHandle { rx: output_rx };

        tokio::spawn(async move {
            tracing::info!("Frame pipeline started");

            let mut stats = PipelineStats::default();
            let mut last_sequence = 0;
            let mut processing_time = Duration::ZERO;
            let stats_started = Instant::now();

            let mut clock = Instant::now();
            const SECOND: Duration = Duration::from_secs(1);

            loop {
                let changed = tokio::select! {
                    _ = shutdown_rx.recv() => break,
                    changed = ingress_rx.changed() => changed,
                };

                if changed.is_err() {
                    break;
                }

                let (sequence, frame) = ingress_rx.borrow().clone();
                stats.dropped_frames += sequence.saturating_sub(last_sequence).saturating_sub(1);
                last_sequence = sequence;

                let processing_started = Instant::now();
                let frame = process_frame(frame, &processing_chain, &mut stats).await;
                let elapsed = processing_started.elapsed();
                if let Some(frame) = frame {
                    stats.frames_processed += 1;
                    processing_time += elapsed;
                    stats.avg_processing_time_ms = (processing_time.as_secs_f64() * 1000.0
                        / stats.frames_processed as f64)
                        as f32;

                    if output_tx.send(frame).await.is_err() {
                        stats.dropped_frames += 1;
                    }
                } else {
                    stats.dropped_frames += 1;
                }

                let elapsed = stats_started.elapsed();
                stats.avg_fps = if elapsed.is_zero() {
                    0
                } else {
                    (stats.frames_processed as f32 / elapsed.as_secs_f32()).round() as u8
                };

                if clock.elapsed() > SECOND {
                    clock = Instant::now();
                    let _ = stats_tx.send(stats.clone());
                    stats.middleware_processing_times.clear();
                }
            }

            tracing::info!("Closed main application pipeline");
        });

        (pipeline, input_handle, output_handle, stats_rx)
    }

    #[inline]
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    #[inline]
    pub async fn closed(&self) {
        if tokio::time::timeout(Duration::from_secs(5), self.shutdown_tx.closed())
            .await
            .is_err()
        {
            tracing::warn!("Failed to wait for resources to release FORCING shutdown");
        }
    }

    #[inline]
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }
}

#[derive(Clone, Debug, Default)]
pub struct PipelineStats {
    pub frames_processed: u64,
    pub avg_fps: u8,
    pub dropped_frames: u64,
    pub current_width: u32,
    pub current_height: u32,
    pub avg_processing_time_ms: f32,
    pub middleware_processing_times: HashMap<&'static str, Duration>,
}

async fn process_frame(
    frame: Frame,
    chain: &ProcessingChain,
    stats: &mut PipelineStats,
) -> Option<Frame> {
    let mut frame = chain.process(frame).await.ok()?;

    stats.current_height = frame.height;
    stats.current_width = frame.width;
    stats.middleware_processing_times = std::mem::take(&mut frame.metadata.processing_time);

    Some(frame)
}
