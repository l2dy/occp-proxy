/// Per-request context for the proxy
#[derive(Debug, Default)]
pub struct RequestCtx {
    /// Request path (set in request_filter)
    pub endpoint: String,
    /// Request contains images
    pub is_vision: bool,
    /// Last message role != "user"
    pub is_agent: bool,
    /// Accumulated request body for inspection
    pub body_buffer: Vec<u8>,
}
