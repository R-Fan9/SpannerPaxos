use spx_core::{ClientWriteRequest, ClientWriteResponse};

pub fn client_write_request_from_proto(r: spx_protocol::ClientWriteRequest) -> ClientWriteRequest {
    ClientWriteRequest { value: r.value }
}

pub fn client_write_response_to_proto(r: ClientWriteResponse) -> spx_protocol::ClientWriteResponse {
    spx_protocol::ClientWriteResponse {
        success: r.success,
        error: r.error.unwrap_or_default(),
    }
}
