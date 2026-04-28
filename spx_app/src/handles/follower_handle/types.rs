use crate::commands::ReplicateWriteCommand;
use spx_lib::task_dispatcher::{Executor, Handler};
use spx_protocol::ReplicateWriteResponse;
use tonic::{Response, Status};

pub type ReplicateWriteResult = Result<Response<ReplicateWriteResponse>, Status>;

pub type ReplicateWriteExecutor =
    Executor<ReplicateWriteCommand, (ReplicateWriteCommand, ReplicateWriteResult)>;

pub type ReplicateWriteHandler = Handler<(ReplicateWriteCommand, ReplicateWriteResult)>;
