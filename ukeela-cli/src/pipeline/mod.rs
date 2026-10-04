pub mod gst;
use std::sync::Arc;

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
    pub processing_times_ms: Vec<f32>,
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
    let mut total_time = std::time::Duration::ZERO;
    let mut dropped = 0u64;

    loop {
        tokio::select! {
            // Incoming frame
            Some(frame) = input_rx.recv() => {
                let start = std::time::Instant::now();

                // Process through middlewares
                match chain.process(frame).await {
                    Ok(processed) => {
                        // Try to send to output (non-blocking)
                        if output_tx.try_send(processed).is_err() {
                            // Output consumer is too slow, drop frame
                            dropped += 1;
                            tracing::warn!("Dropped frame {} - output backlog", frames_processed);
                        }

                        frames_processed += 1;
                        total_time += start.elapsed();
                    }
                    Err(e) => {
                        tracing::error!("Frame processing error: {}", e);
                        dropped += 1;
                    }
                }
            }

            // Shutdown signal
            _ = shutdown.recv() => {
               tracing:: warn!("Shutdown signal received, finishing processing thread");
                break;
            }
        }
    }

    // Emit final stats
    let avg_time = if frames_processed > 0 {
        (total_time.as_secs_f32() * 1000.0) / frames_processed as f32
    } else {
        0.0
    };

    let _ = stats_tx.send(PipelineStats {
        frames_processed,
        avg_fps: if total_time.as_secs_f32() > 0.0 {
            frames_processed as f32 / total_time.as_secs_f32()
        } else {
            0.0
        },
        dropped_frames: dropped,
        processing_times_ms: vec![avg_time],
    });

    tracing::info!(
        "Processing thread ended: {} frames, {} dropped",
        frames_processed,
        dropped
    );
}

impl Drop for FramePipeline {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
    }
}
