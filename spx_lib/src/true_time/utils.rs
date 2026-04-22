use super::TrueTimeService;
use chrono::{DateTime, Utc};

impl TrueTimeService {
    pub fn commit_wait(&self, time: DateTime<Utc>) {
        while !self.after(time) {}
    }
}
