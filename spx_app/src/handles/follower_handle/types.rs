use crate::commands::ReplicateWriteCommand;
use spx_lib::task_dispatcher::Handler;
use spx_protocol::ReplicateWriteResponse;
use tonic::{Response, Status};

pub type ReplicateWriteResult = Result<Response<ReplicateWriteResponse>, Status>;

pub type ReplicateWriteHandler = Handler<(ReplicateWriteCommand, ReplicateWriteResult)>;
