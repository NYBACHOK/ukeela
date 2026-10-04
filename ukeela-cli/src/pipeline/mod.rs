pub mod gst;
use std::sync::Arc;
use std::{collections::HashMap, time::Duration};

use tokio::sync::{
    broadcast,
    mpsc::{Receiver, Sender, channel},
};

use crate::{
    frame::Frame,
    middleware::chain::ProcessingChain,
    pipeline::{input::InputHandle, output::OutputHandle},
};

pub mod input;
pub mod output;

#[derive(Debug)]
pub struct FramePipeline {
    // Control signals
    shutdown_tx: broadcast::Sender<()>,
    pub stats_rx: broadcast::Receiver<PipelineStats>,
}

#[derive(Clone, Debug)]
pub struct PipelineStats {
    pub frames_processed: u64,
    pub avg_fps: f32,
    pub dropped_frames: u64,
    pub avg_processing_time_ms: f32,
    pub middleware_processing_times_ms: HashMap<&'static str, f32>,
}

impl FramePipeline {
    /// Create new pipeline with configurable channel size
    pub fn new(
        processing_chain: ProcessingChain,
        channel_size: usize,
    ) -> (Self, InputHandle, OutputHandle) {
        let (input_tx, input_rx) = channel(channel_size);
        let (output_tx, output_rx) = channel(channel_size);
        let (stats_tx, stats_rx) = broadcast::channel(16);
        let (shutdown_tx, _) = broadcast::channel(1);

        let pipeline = FramePipeline {
            shutdown_tx: shutdown_tx.clone(),
            stats_rx,
        };

        let input_handle = InputHandle {
            tx: input_tx,
            shutdown_rx: Arc::new(shutdown_tx.subscribe()),
        };

        let output_handle = OutputHandle {
            rx: output_rx,
            tx: input_handle.clone(),
        };

        // Spawn processing thread
        let shutdown = shutdown_tx.subscribe();

        tokio::spawn(async move {
            tracing::info!("Processing thread started");

            process_thread(input_rx, output_tx, processing_chain, shutdown, stats_tx).await;
        });

        (pipeline, input_handle, output_handle)
    }
}

/// Background processing thread
async fn process_thread(
    mut input_rx: Receiver<Frame>,
    output_tx: Sender<Frame>,
    chain: ProcessingChain,
    mut shutdown: broadcast::Receiver<()>,
    stats_tx: broadcast::Sender<PipelineStats>,
) {
    let mut frames_processed = 0u64;
    let mut processing_time = Duration::ZERO;
    let mut dropped = 0u64;
    let mut middleware_processing_time = HashMap::<&'static str, Duration>::new();
    let mut window_start = std::time::Instant::now();
    let mut stats_interval = tokio::time::interval(Duration::from_secs(1));
    stats_interval.tick().await;

    loop {
        tokio::select! {
            // Incoming frame
            Some(frame) = input_rx.recv() => {
                let start = std::time::Instant::now();

                // Process through middlewares
                match chain.process(frame).await {
                    Ok(processed) => {
                        let elapsed = start.elapsed();
                        processing_time += elapsed;
                        frames_processed += 1;

                        {
                            let metadata = processed.metadata.read().expect(crate::POISONED_LOCK_MSG);
                            for (name, duration) in &metadata.processing_time {
                                *middleware_processing_time.entry(name).or_default() += *duration;
                            }
                        }

                        // Try to send to output (non-blocking)
                        if output_tx.try_send(processed).is_err() {
                            // Output consumer is too slow, drop frame
                            dropped += 1;
                            tracing::warn!("Dropped frame {} - output backlog", frames_processed);
                        }

                    }
                    Err(e) => {
                        tracing::error!("Frame processing error: {}", e);
                        dropped += 1;
                    }
                }
            }

            _ = stats_interval.tick() => {
                let elapsed = window_start.elapsed();
                publish_stats(
                    &stats_tx,
                    frames_processed,
                    dropped,
                    processing_time,
                    &middleware_processing_time,
                    elapsed,
                );

                frames_processed = 0;
                dropped = 0;
                processing_time = Duration::ZERO;
                middleware_processing_time.clear();
                window_start = std::time::Instant::now();
            }

            // Shutdown signal
            _ = shutdown.recv() => {
               tracing:: warn!("Shutdown signal received, finishing processing thread");
                break;
            }
        }
    }

    publish_stats(
        &stats_tx,
        frames_processed,
        dropped,
        processing_time,
        &middleware_processing_time,
        window_start.elapsed(),
    );

    tracing::info!(
        "Processing thread ended: {} frames processed, {} dropped in final interval",
        frames_processed,
        dropped
    );
}

fn publish_stats(
    stats_tx: &broadcast::Sender<PipelineStats>,
    frames_processed: u64,
    dropped_frames: u64,
    processing_time: Duration,
    middleware_processing_time: &HashMap<&'static str, Duration>,
    interval: Duration,
) {
    let middleware_processing_times_ms = middleware_processing_time
        .iter()
        .map(|(name, elapsed)| {
            (
                *name,
                (elapsed.as_secs_f64() * 1000.0 / frames_processed.max(1) as f64) as f32,
            )
        })
        .collect();
    let stats = PipelineStats {
        frames_processed,
        avg_fps: if interval.is_zero() {
            0.0
        } else {
            frames_processed as f32 / interval.as_secs_f32()
        },
        dropped_frames,
        avg_processing_time_ms: if frames_processed == 0 {
            0.0
        } else {
            processing_time.as_secs_f64() as f32 * 1000.0 / frames_processed as f32
        },
        middleware_processing_times_ms,
    };

    if stats_tx.send(stats).is_err() {
        tracing::debug!("No pipeline statistics receiver is available");
    }
}

impl Drop for FramePipeline {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
    }
}
