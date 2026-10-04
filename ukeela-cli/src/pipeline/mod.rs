pub mod gst;
use std::{collections::HashMap, time::Duration};
use std::{
    num::NonZero,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use tokio::sync::{
    broadcast,
    mpsc::{Sender, channel},
    watch,
};

use crate::{
    frame::Frame,
    middleware::chain::ProcessingChain,
    pipeline::{
        input::{IngressFrame, InputHandle},
        output::OutputHandle,
    },
};

pub mod input;
pub mod output;

pub struct FramePipeline {
    // Control signals
    shutdown_tx: broadcast::Sender<()>,
    ingress_closed: Arc<AtomicBool>,
    pub stats_rx: broadcast::Receiver<PipelineStats>,
}

impl std::fmt::Debug for FramePipeline {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FramePipeline")
            .field(
                "ingress_closed",
                &self.ingress_closed.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
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
    /// Create a pipeline with a bounded output queue and paced latest-frame ingress.
    pub fn new(
        processing_chain: ProcessingChain,
        channel_size: usize,
        desired_fps: NonZero<u32>,
    ) -> (Self, InputHandle, OutputHandle) {
        let (ingress_tx, ingress_rx) = watch::channel(None);
        let ingress_closed = Arc::new(AtomicBool::new(false));
        let (output_tx, output_rx) = channel(channel_size);
        let (stats_tx, stats_rx) = broadcast::channel(16);
        let (shutdown_tx, _) = broadcast::channel(1);

        let pipeline = FramePipeline {
            shutdown_tx: shutdown_tx.clone(),
            ingress_closed: ingress_closed.clone(),
            stats_rx,
        };

        let input_handle = InputHandle {
            tx: ingress_tx.clone(),
            sequence: Arc::new(AtomicU64::new(0)),
            closed: ingress_closed,
            shutdown_rx: Arc::new(shutdown_tx.subscribe()),
        };

        let output_handle = OutputHandle {
            rx: output_rx,
            tx: input_handle.clone(),
        };

        // Spawn processing thread
        let shutdown = shutdown_tx.subscribe();
        let period = Duration::from_nanos((1_000_000_000 / u64::from(desired_fps.get())).max(1));

        tokio::spawn(async move {
            tracing::info!("Processing thread started");

            process_thread(
                ingress_rx,
                output_tx,
                processing_chain,
                shutdown,
                stats_tx,
                period,
            )
            .await;
        });

        (pipeline, input_handle, output_handle)
    }
}

/// Background processing thread
async fn process_thread(
    mut ingress: watch::Receiver<Option<IngressFrame>>,
    output_tx: Sender<Frame>,
    chain: ProcessingChain,
    mut shutdown: broadcast::Receiver<()>,
    stats_tx: broadcast::Sender<PipelineStats>,
    period: Duration,
) {
    let mut frames_processed = 0u64;
    let mut processing_time = Duration::ZERO;
    let mut dropped = 0u64;
    let mut middleware_processing_time = HashMap::<&'static str, Duration>::new();
    let mut window_start = std::time::Instant::now();
    let mut stats_interval = tokio::time::interval(Duration::from_secs(1));
    stats_interval.tick().await;
    let mut pacer = FramePacer::new(period);
    let mut pending_frame = None;
    let mut last_sequence = 0;

    loop {
        if pending_frame.is_none() {
            tokio::select! {
                changed = ingress.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    if let Some((sequence, frame, skipped)) = take_latest(&mut ingress, &mut last_sequence) {
                        pending_frame = Some((sequence, frame));
                        dropped += skipped;
                    } else {
                        break;
                    }
                }
                _ = stats_interval.tick() => {
                    publish_stats(
                        &stats_tx,
                        frames_processed,
                        dropped,
                        processing_time,
                        &middleware_processing_time,
                        window_start.elapsed(),
                    );
                    frames_processed = 0;
                    dropped = 0;
                    processing_time = Duration::ZERO;
                    middleware_processing_time.clear();
                    window_start = std::time::Instant::now();
                }
                _ = shutdown.recv() => {
                    tracing::warn!("Shutdown signal received, finishing processing thread");
                    break;
                }
            }
            continue;
        }

        match pacer.check(std::time::Instant::now()) {
            PacerAction::WaitUntil(deadline) => {
                tokio::select! {
                    changed = ingress.changed() => {
                        if changed.is_err() {
                            break;
                        }
                        if !replace_pending_with_latest(
                            &mut ingress,
                            &mut pending_frame,
                            &mut last_sequence,
                            &mut dropped,
                        ) {
                            break;
                        }
                    }
                    _ = tokio::time::sleep_until(deadline.into()) => {}
                    _ = stats_interval.tick() => {
                        publish_stats(
                            &stats_tx,
                            frames_processed,
                            dropped,
                            processing_time,
                            &middleware_processing_time,
                            window_start.elapsed(),
                        );
                        frames_processed = 0;
                        dropped = 0;
                        processing_time = Duration::ZERO;
                        middleware_processing_time.clear();
                        window_start = std::time::Instant::now();
                    }
                    _ = shutdown.recv() => {
                        tracing::warn!("Shutdown signal received, finishing processing thread");
                        break;
                    }
                }

                if pending_frame.is_some()
                    && ingress.has_changed().unwrap_or(false)
                    && !replace_pending_with_latest(
                        &mut ingress,
                        &mut pending_frame,
                        &mut last_sequence,
                        &mut dropped,
                    )
                {
                    break;
                }
                continue;
            }
            PacerAction::TooLate => {
                pending_frame = None;
                dropped += 1;
                continue;
            }
            PacerAction::Ready => {}
        }

        let (_, frame) = pending_frame.take().expect("pending frame was checked");
        let start = std::time::Instant::now();
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

                if output_tx.try_send(processed).is_err() {
                    dropped += 1;
                    tracing::warn!("Dropped processed frame - output backlog");
                }
            }
            Err(error) => {
                tracing::error!("Frame processing error: {}", error);
                dropped += 1;
            }
        }

        tokio::select! {
            changed = ingress.changed() => {
                if changed.is_err() {
                    break;
                }
                if let Some((sequence, frame, skipped)) = take_latest(&mut ingress, &mut last_sequence) {
                    pending_frame = Some((sequence, frame));
                    dropped += skipped;
                } else {
                    break;
                }
            }
            _ = stats_interval.tick() => {
                publish_stats(
                    &stats_tx,
                    frames_processed,
                    dropped,
                    processing_time,
                    &middleware_processing_time,
                    window_start.elapsed(),
                );
                frames_processed = 0;
                dropped = 0;
                processing_time = Duration::ZERO;
                middleware_processing_time.clear();
                window_start = std::time::Instant::now();
            }
            _ = shutdown.recv() => {
                tracing::warn!("Shutdown signal received, finishing processing thread");
                break;
            }
        }
    }

    if let Some((sequence, _)) = ingress.borrow().as_ref() {
        dropped += sequence.saturating_sub(last_sequence);
    }
    if pending_frame.is_some() {
        dropped += 1;
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

fn take_latest(
    ingress: &mut watch::Receiver<Option<IngressFrame>>,
    last_sequence: &mut u64,
) -> Option<(u64, Frame, u64)> {
    let (sequence, frame) = ingress.borrow_and_update().clone()?;
    let skipped = sequence.saturating_sub(*last_sequence).saturating_sub(1);
    *last_sequence = sequence;
    Some((sequence, frame, skipped))
}

fn replace_pending_with_latest(
    ingress: &mut watch::Receiver<Option<IngressFrame>>,
    pending_frame: &mut Option<IngressFrame>,
    last_sequence: &mut u64,
    dropped: &mut u64,
) -> bool {
    let Some((sequence, frame, skipped)) = take_latest(ingress, last_sequence) else {
        return false;
    };
    if pending_frame.replace((sequence, frame)).is_some() {
        *dropped += 1;
    }
    *dropped += skipped;
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PacerAction {
    Ready,
    WaitUntil(std::time::Instant),
    TooLate,
}

#[derive(Debug)]
struct FramePacer {
    period: Duration,
    next_slot: Option<std::time::Instant>,
}

impl FramePacer {
    fn new(period: Duration) -> Self {
        Self {
            period,
            next_slot: None,
        }
    }

    fn check(&mut self, now: std::time::Instant) -> PacerAction {
        let Some(deadline) = self.next_slot else {
            self.next_slot = Some(now + self.period);
            return PacerAction::Ready;
        };

        if now < deadline {
            return PacerAction::WaitUntil(deadline);
        }

        if now.duration_since(deadline) >= self.period {
            self.next_slot = Some(now + self.period);
            return PacerAction::TooLate;
        }

        self.next_slot = Some(deadline + self.period);
        PacerAction::Ready
    }
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
        self.ingress_closed.store(true, Ordering::Release);
        let _ = self.shutdown_tx.send(());
    }
}
