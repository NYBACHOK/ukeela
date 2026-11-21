use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use tokio::sync::{broadcast, mpsc::channel, watch};
use ukeela_common::{Frame, Middleware};
use ukeela_processing_chain::ProcessingChain;

use crate::{input::InputHandle, output::OutputHandle};

pub mod input;
pub mod output;

const OUTPUT_CHANNEL_CAPACITY: usize = 32;

#[derive(Debug, Clone)]
pub struct FramePipeline {
    shutdown_tx: broadcast::Sender<()>,
}

impl FramePipeline {
    /// Create a pipeline using a processing chain and log telemetry at the given interval.
    ///
    /// The interval must be non-zero. Telemetry is logged locally and is not broadcast.
    pub fn from_chain<T, C>(
        mut processing_chain: C,
        telemetry_interval: Duration,
    ) -> (Self, InputHandle, OutputHandle)
    where
        T: Middleware + Send + Sync + 'static,
        C: ProcessingChain<T> + Send + 'static,
    {
        assert!(
            !telemetry_interval.is_zero(),
            "telemetry interval must be greater than zero"
        );

        let (ingress_tx, mut ingress_rx) = watch::channel((0, Frame::default()));
        let (output_tx, output_rx) = channel(OUTPUT_CHANNEL_CAPACITY);
        let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);
        let mut telemetry = tokio::time::interval_at(
            tokio::time::Instant::now() + telemetry_interval,
            telemetry_interval,
        );
        telemetry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let pipeline = Self {
            shutdown_tx: shutdown_tx.clone(),
        };
        let input_handle = InputHandle {
            tx: ingress_tx,
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

            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => break,
                    changed = ingress_rx.changed() => {
                        if changed.is_err() {
                            break;
                        }

                        let (sequence, mut frame) = ingress_rx.borrow().clone();
                        stats.dropped_frames +=
                            sequence.saturating_sub(last_sequence).saturating_sub(1);
                        last_sequence = sequence;
                        frame.metadata.arrival_time = Some(Instant::now());

                        if processing_chain.push(frame).is_some() {
                            stats.dropped_frames += 1;
                        }
                    }
                    result = processing_chain.pop() => {
                        match result {
                            Some(Ok(mut frame)) => {
                                stats.frames_processed += 1;
                                processing_time += frame
                                    .metadata
                                    .processing_time
                                    .values()
                                    .copied()
                                    .sum::<Duration>();
                                stats.avg_processing_time_ms =
                                    (processing_time.as_secs_f64() * 1000.0
                                        / stats.frames_processed as f64) as f32;
                                stats.current_height = frame.height;
                                stats.current_width = frame.width;
                                stats.middleware_processing_times =
                                    std::mem::take(&mut frame.metadata.processing_time);

                                if output_tx.send(frame).await.is_err() {
                                    stats.dropped_frames += 1;
                                }
                            }
                            Some(Err(error)) => {
                                stats.dropped_frames += 1;
                                tracing::error!(
                                    error = ?error,
                                    "Dropping frame after middleware processing failed"
                                );
                            }
                            None => break,
                        }
                    }
                    _ = telemetry.tick() => {
                        let elapsed = stats_started.elapsed();
                        stats.avg_fps = if elapsed.is_zero() {
                            0
                        } else {
                            (stats.frames_processed as f32 / elapsed.as_secs_f32()).round() as u8
                        };

                        log_pipeline_stats(&stats);
                        stats.middleware_processing_times.clear();
                    }
                }
            }

            tracing::info!("Closed main application pipeline");
        });

        (pipeline, input_handle, output_handle)
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
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }
}

#[derive(Debug, Default)]
struct PipelineStats {
    frames_processed: u64,
    avg_fps: u8,
    dropped_frames: u64,
    current_width: usize,
    current_height: usize,
    avg_processing_time_ms: f32,
    middleware_processing_times: BTreeMap<&'static str, Duration>,
}

fn log_pipeline_stats(stats: &PipelineStats) {
    tracing::info!(
        frames_processed = stats.frames_processed,
        avg_fps = stats.avg_fps,
        dropped_frames = stats.dropped_frames,
        current_width = stats.current_width,
        current_height = stats.current_height,
        avg_processing_time_ms = format!("{:.6}", stats.avg_processing_time_ms),
        middleware_processing_times_ms = ?stats
            .middleware_processing_times
            .iter()
            .map(|(name, duration)| (*name, format!("{:.2}", duration.as_secs_f64() * 1000.0)))
            .collect::<Vec<_>>(),
        "Pipeline statistics"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ukeela_processing_chain::{
        ProcessingChain, multi::MultiProcessingChain, single::SingleProcessingChain,
    };

    #[tokio::test]
    async fn processes_frames_from_a_single_chain() {
        let chain = SingleProcessingChain::<ukeela_common::NoOpMiddleware>::new();
        let (pipeline, mut input, mut output) =
            FramePipeline::from_chain(chain, Duration::from_secs(60));
        let frame = Frame {
            id: 7,
            ..Frame::default()
        };

        input.send_frame(frame).await.unwrap();
        let processed = tokio::time::timeout(Duration::from_secs(1), output.receive_frame())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(processed.id, 7);

        pipeline.shutdown();
        drop(input);
        drop(output);
        pipeline.closed().await;
    }

    #[tokio::test]
    async fn processes_frames_from_a_multi_chain() {
        let mut chain = MultiProcessingChain::<ukeela_common::NoOpMiddleware>::new();
        chain.add_middleware(ukeela_common::NoOpMiddleware);
        let (pipeline, mut input, mut output) =
            FramePipeline::from_chain(chain, Duration::from_secs(60));
        input
            .send_frame(Frame {
                id: 8,
                ..Frame::default()
            })
            .await
            .unwrap();

        let processed = tokio::time::timeout(Duration::from_secs(1), output.receive_frame())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(processed.id, 8);

        pipeline.shutdown();
        drop(input);
        drop(output);
        pipeline.closed().await;
    }
}
