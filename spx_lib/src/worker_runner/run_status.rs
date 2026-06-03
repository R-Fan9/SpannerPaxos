/// An enum representing the current run status of the worker runner
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum RunStatus {
    IDLE = 0,
    ACTIVE = 1,
}

impl From<RunStatus> for u8 {
    fn from(status: RunStatus) -> Self {
        status as u8
    }
}
