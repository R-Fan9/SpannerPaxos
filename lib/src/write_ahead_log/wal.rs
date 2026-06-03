use std::collections::BTreeMap;

pub struct WriteAheadLog {
    // slot → (term, value)
    entries: BTreeMap<u32, (u32, String)>,
}

impl WriteAheadLog {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn append(&mut self, slot: u32, term: u32, value: String) {
        self.entries.insert(slot, (term, value));
    }

    /// Returns true if the WAL contains an entry at `slot`.
    pub fn has_entry(&self, slot: u32) -> bool {
        self.entries.contains_key(&slot)
    }

    /// Returns the term stored at `slot`, or `None` if no entry exists for that slot.
    pub fn get_term(&self, slot: u32) -> Option<u32> {
        self.entries.get(&slot).map(|(term, _)| *term)
    }

    /// Returns all entries with slot >= `from_slot`, in ascending slot order.
    pub fn get_entries_from(&self, from_slot: u32) -> Vec<(u32, u32, String)> {
        self.entries
            .range(from_slot..)
            .map(|(&slot, (term, value))| (slot, *term, value.clone()))
            .collect()
    }

    /// Removes all entries with slot >= `starting_slot`.
    pub fn truncate_from(&mut self, starting_slot: u32) {
        self.entries.split_off(&starting_slot);
    }

    /// Returns the highest slot logged at exactly `term`, or `None` if no entry exists for that term.
    pub fn find_highest_slot_for_term(&self, term: u32) -> Option<u32> {
        self.entries
            .iter()
            .filter(|(_, (t, _))| *t == term)
            .map(|(slot, _)| *slot)
            .max()
    }

    /// Returns the lowest slot logged at exactly `term`, or `None` if no entry exists for that term.
    /// Used by followers to report the first slot where log divergence could begin at a given term.
    pub fn find_lowest_slot_for_term(&self, term: u32) -> Option<u32> {
        self.entries
            .iter()
            .filter(|(_, (t, _))| *t == term)
            .map(|(slot, _)| *slot)
            .min()
    }

    /// Returns `(term, slot)` of the highest slot logged at `term`. If no entry exists for
    /// `term`, decrements the term and retries until a match is found or all terms are exhausted.
    /// Used to locate the highest anchor point where a leader's log can safely align with the follower's log.
    pub fn find_highest_anchor(&self, term: u32) -> Option<(u32, u32)> {
        let mut current_term = term;
        loop {
            let highest_slot = self.entries
                .iter()
                .filter(|(_, (t, _))| *t == current_term)
                .map(|(slot, _)| *slot)
                .max();

            if let Some(slot) = highest_slot {
                return Some((current_term, slot));
            }

            current_term = current_term.checked_sub(1)?;
        }
    }
}
