#[derive(Clone)]
pub struct LogEntry {
    // The term number when the leader originally proposed this write
    pub term: u32,

    // The slot number of this entry in the append-only log
    pub slot: u32,

    // The actual database command/mutation
    pub entry: String,
}
