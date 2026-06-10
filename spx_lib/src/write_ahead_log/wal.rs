use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct LogEntry {
    // The Paxos term in which this entry was proposed
    pub term: u32,

    // The position of this entry in the append-only log
    pub slot: u32,

    // The database command or mutation carried by this entry
    pub entry: String,

    // The latest bound of TrueTime at the moment this entry was written to the leader WAL.
    // Used for commit_wait to enforce external consistency.
    pub timestamp: DateTime<Utc>,
}

pub struct WriteAheadLog {
    entries: BTreeMap<u32, LogEntry>,
}

impl WriteAheadLog {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn append(&mut self, entry: LogEntry) {
        self.entries.insert(entry.slot, entry);
    }

    pub fn has_entry(&self, slot: u32) -> bool {
        self.entries.contains_key(&slot)
    }

    pub fn get_term(&self, slot: u32) -> Option<u32> {
        self.entries.get(&slot).map(|e| e.term)
    }

    pub fn get_entries_from(&self, from_slot: u32) -> Vec<LogEntry> {
        self.entries
            .range(from_slot..)
            .map(|(_, e)| e.clone())
            .collect()
    }

    pub fn truncate_from(&mut self, starting_slot: u32) {
        self.entries.split_off(&starting_slot);
    }

    pub fn find_highest_slot_for_term(&self, term: u32) -> Option<u32> {
        self.entries
            .iter()
            .filter(|(_, e)| e.term == term)
            .map(|(slot, _)| *slot)
            .max()
    }

    /// Used by followers to report the first slot where log divergence could begin at a given term.
    pub fn find_lowest_slot_for_term(&self, term: u32) -> Option<u32> {
        self.entries
            .iter()
            .filter(|(_, e)| e.term == term)
            .map(|(slot, _)| *slot)
            .min()
    }

    /// Used to locate the highest anchor point where a leader's log can safely align with the follower's log.
    pub fn find_highest_anchor(&self, term: u32) -> Option<(u32, u32)> {
        let mut current_term = term;
        loop {
            let highest_slot = self
                .entries
                .iter()
                .filter(|(_, e)| e.term == current_term)
                .map(|(slot, _)| *slot)
                .max();

            if let Some(slot) = highest_slot {
                return Some((current_term, slot));
            }

            current_term = current_term.checked_sub(1)?;
        }
    }
}
