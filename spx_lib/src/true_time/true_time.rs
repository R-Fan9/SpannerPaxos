use chrono::{DateTime, Utc};

pub struct TimeInterval {
    pub earliest: DateTime<Utc>,
    pub latest: DateTime<Utc>,
}

// A True Time (TT) service responsible for returning accurate current time as a bounded interval
// to allow strong consistency implementation
pub struct TrueTimeService {}

impl TrueTimeService {
    // Returns the current time as a bounded interval
    pub fn now(&self) -> TimeInterval {
        TimeInterval {
            earliest: Utc::now(),
            latest: Utc::now(),
        }
    }

    // Checks if a timestamp is before the lower bound of the current time interval
    pub fn before(&self, timestamp: DateTime<Utc>) -> bool {
        true
    }

    // Checks if a timestamp is after the upper bound of the current time interval
    pub fn after(&self, timestamp: DateTime<Utc>) -> bool {
        true
    }
}
