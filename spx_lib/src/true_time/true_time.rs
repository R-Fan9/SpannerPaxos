use chrono::{DateTime, Utc};
use std::time::Duration;
use tokio::time;

const UNCERTAINTY_MS: u64 = 7;

struct TimeInterval {
    earliest: DateTime<Utc>,
    latest: DateTime<Utc>,
}

// A True Time (TT) service responsible for returning accurate current time as a bounded interval to allow strong consistency implementation
pub struct TrueTime {}

impl TrueTime {
    // Returns the current time as a bounded interval [now - epsilon, now + epsilon]
    fn now() -> TimeInterval {
        let now = Utc::now();
        let uncertainty = Duration::from_millis(UNCERTAINTY_MS);
        TimeInterval {
            earliest: now - uncertainty,
            latest: now + uncertainty,
        }
    }

    // Returns true if the timestamp is definitely in the future (hasn't arrived yet)
    pub fn before(timestamp: DateTime<Utc>) -> bool {
        timestamp > Self::now().latest
    }

    // Returns true if the timestamp is definitely in the past (has already passed)
    pub fn after(timestamp: DateTime<Utc>) -> bool {
        timestamp < Self::now().earliest
    }

    // Wait until the timestamp is definitely in the future
    pub async fn commit_wait(time: DateTime<Utc>) {
        while !Self::after(time) {
            time::sleep(Duration::from_micros(100)).await;
        }
    }
}
