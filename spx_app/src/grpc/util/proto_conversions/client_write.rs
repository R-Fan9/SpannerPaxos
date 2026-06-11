use spx_core::{ClientWriteRequest, ClientWriteResponse};

pub fn client_write_request_from_proto(r: spx_protocol::ClientWriteRequest) -> ClientWriteRequest {
    ClientWriteRequest { value: r.value }
}

pub fn client_write_request_to_proto(r: ClientWriteRequest) -> spx_protocol::ClientWriteRequest {
    spx_protocol::ClientWriteRequest { value: r.value }
}

pub fn client_write_response_to_proto(r: ClientWriteResponse) -> spx_protocol::ClientWriteResponse {
    spx_protocol::ClientWriteResponse {
        success: r.success,
        error: r.error.unwrap_or_default(),
    }
}

pub fn client_write_response_from_proto(r: spx_protocol::ClientWriteResponse) -> ClientWriteResponse {
    ClientWriteResponse {
        success: r.success,
        error: if r.error.is_empty() { None } else { Some(r.error) },
    }
}
