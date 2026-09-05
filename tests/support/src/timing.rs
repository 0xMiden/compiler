//! Opt-in integration-test checkpoint timing without retaining compiler artifacts.

use std::time::{Duration, Instant};

use midenc_compile::pipeline::{Artifact, CheckpointId, Observer, TargetRole};

pub(crate) struct PipelineTiming {
    started: Instant,
    checkpoints: Vec<(CheckpointId, Duration)>,
}

impl PipelineTiming {
    pub(crate) fn enabled() -> bool {
        std::env::var("MIDENC_TEST_TIMINGS").is_ok_and(|value| value == "1")
    }

    pub(crate) fn new() -> Self {
        Self {
            started: Instant::now(),
            checkpoints: Vec::new(),
        }
    }

    // Observers must not perform I/O: report after the pipeline returns instead.
    pub(crate) fn report(&self, name: &str) {
        let total = self.started.elapsed();
        let mut previous = Duration::ZERO;
        for (checkpoint, elapsed) in &self.checkpoints {
            eprintln!(
                "midenc-timing artifact={name:?} checkpoint={checkpoint} interval_ms={:.3} \
                 elapsed_ms={:.3}",
                (*elapsed - previous).as_secs_f64() * 1000.0,
                elapsed.as_secs_f64() * 1000.0,
            );
            previous = *elapsed;
        }
        eprintln!(
            "midenc-timing artifact={name:?} checkpoint=returned interval_ms={:.3} \
             elapsed_ms={:.3}",
            (total - previous).as_secs_f64() * 1000.0,
            total.as_secs_f64() * 1000.0,
        );
    }
}

impl Observer for PipelineTiming {
    fn on_checkpoint(&mut self, checkpoint: CheckpointId, role: TargetRole, _artifact: &Artifact) {
        if role.is_root() {
            self.checkpoints.push((checkpoint, self.started.elapsed()));
        }
    }
}
