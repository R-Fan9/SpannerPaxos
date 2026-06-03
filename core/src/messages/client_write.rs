#[derive(Clone)]
pub struct ClientWriteRequest {
    pub value: String,
}

pub struct ClientWriteResponse {
    pub success: bool,
    pub error: Option<String>,
}
