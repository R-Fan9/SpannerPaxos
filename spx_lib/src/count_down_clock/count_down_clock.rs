use rand::Rng;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::time;

const MIN_WAIT_TIME_MS: u64 = 15;

// A count-down clock that can be reset to generate a new random wait time within a set upper bound
pub struct CountDownClock {
    max_wait_time_ms: u64,
    started: AtomicBool,
}

impl CountDownClock {
    pub fn new(max_wait_time_ms: u64) -> Self {
        assert!(
            max_wait_time_ms >= MIN_WAIT_TIME_MS,
            "max_wait_time must be at least {MIN_WAIT_TIME_MS}ms, got {max_wait_time_ms}"
        );
        Self {
            max_wait_time_ms,
            started: AtomicBool::new(false),
        }
    }

    pub fn has_started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }

    pub fn reset(&self) {
        self.started.store(false, Ordering::Release);
    }

    pub async fn start(&self) {
        // Atomically claim the started slot, only the first caller proceeds, all others skip
        // Note: AcqRel ensures this thread sees the freshest 'started' value (Acquire) and,
        // if it wins, immediately publishes the updated `started` value (true) to all other threads (Release)
        if self
            .started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            // The count-down clock has already started, skip
            return;
        }

        // Generate a random wait time within the set upper bound
        let wait_ms = rand::thread_rng().gen_range(MIN_WAIT_TIME_MS..=self.max_wait_time_ms);

        // Sleep for the specified wait time
        time::sleep(Duration::from_millis(wait_ms)).await
    }
}
