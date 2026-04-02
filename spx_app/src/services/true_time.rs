pub struct TimeInterval {
    pub earliest: u64,
    pub latest: u64,
}

// A True Time (TT) service responsible for returning accurate current time as a bounded interval
// to allow strong consistency implementation
pub struct TrueTimeService {}

impl TrueTimeService {
    // Returns the current time as a bounded interval
    pub fn now(&self) -> TimeInterval {
        TimeInterval {
            earliest: 0,
            latest: 0,
        }
    }

    // Checks if a timestamp is before the lower bound of the current time interval
    pub fn before(&self, timestamp: u64) -> bool {
        true
    }

    // Checks if a timestamp is after the upper bound of the current time interval
    pub fn after(&self, timestamp: u64) -> bool {
        true
    }
}