use crate::auth::AuthState;
use crate::config::{Config, DEFAULT_OPENCODE_VERSION, MAX_BODY_SIZE};
use crate::proxy::context::RequestCtx;

use async_trait::async_trait;
use bytes::Bytes;
use hickory_resolver::{Resolver, TokioResolver};
use pingora::http::RequestHeader;
use pingora::prelude::*;
use pingora::proxy::{ProxyHttp, Session};
use std::net::SocketAddr;
use std::sync::Arc;
use subtle::ConstantTimeEq;
use tokio::sync::RwLock;

/// Supported endpoints (allowlist)
const ALLOWED_ENDPOINTS: &[(&str, &str)] = &[
    ("POST", "/chat/completions"),
    ("POST", "/responses"),
    ("POST", "/v1/messages"),
];

pub struct CopilotProxy {
    pub auth_state: Arc<RwLock<Option<AuthState>>>,
    pub config: Config,
    pub resolver: TokioResolver,
}

impl CopilotProxy {
    pub fn new(auth_state: Arc<RwLock<Option<AuthState>>>, config: Config) -> anyhow::Result<Self> {
        // Create DNS resolver with system config (respects /etc/resolv.conf)
        // The resolver caches records according to their TTL
        let resolver = Resolver::builder_tokio()
            .map_err(|err| anyhow::anyhow!("Failed to read system DNS config: {err}"))?
            .build()?;
        Ok(Self {
            auth_state,
            config,
            resolver,
        })
    }

    /// Validate API key using constant-time comparison
    fn validate_api_key(&self, provided: &str) -> bool {
        match &self.config.api_key {
            Some(expected) => {
                let expected_bytes = expected.as_bytes();
                let provided_bytes = provided.as_bytes();
                expected_bytes.ct_eq(provided_bytes).into()
            }
            None => true, // No API key configured, allow all
        }
    }

    /// Extract API key from request headers
    fn extract_api_key(req: &RequestHeader) -> Option<String> {
        // Try Authorization: Bearer <key>
        if let Some(auth) = req.headers.get("authorization") {
            if let Ok(auth_str) = auth.to_str() {
                if let Some(token) = auth_str.strip_prefix("Bearer ") {
                    return Some(token.to_string());
                }
            }
        }

        // Try x-api-key header
        if let Some(key) = req.headers.get("x-api-key") {
            if let Ok(key_str) = key.to_str() {
                return Some(key_str.to_string());
            }
        }

        None
    }

    /// Check if endpoint is in allowlist
    fn is_endpoint_allowed(method: &str, path: &str) -> bool {
        // Normalize path - reject suspicious patterns
        if path.contains("..") || path.contains("//") {
            return false;
        }

        ALLOWED_ENDPOINTS
            .iter()
            .any(|(m, p)| m.eq_ignore_ascii_case(method) && *p == path)
    }

    /// Strip `/v1/` prefix from path.
    fn strip_v1_prefix(path: &str) -> Option<String> {
        match path {
            "/v1/chat/completions" => Some("/chat/completions".to_string()),
            "/v1/responses" => Some("/responses".to_string()),
            _ => None,
        }
    }

    /// Inspect request body for vision and agent detection
    fn inspect_body(endpoint: &str, body: &[u8]) -> (bool, bool) {
        let Ok(json): Result<serde_json::Value, _> = serde_json::from_slice(body) else {
            return (false, false);
        };

        match endpoint {
            "/chat/completions" => {
                // Vision: messages[].content[].type == "image_url"
                let is_vision = json
                    .get("messages")
                    .and_then(|m| m.as_array())
                    .map(|messages| {
                        messages.iter().any(|msg| {
                            msg.get("content")
                                .and_then(|c| c.as_array())
                                .map(|parts| {
                                    parts.iter().any(|part| {
                                        part.get("type").and_then(|t| t.as_str())
                                            == Some("image_url")
                                    })
                                })
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false);

                // Agent: messages[-1].role != "user"
                let is_agent = json
                    .get("messages")
                    .and_then(|m| m.as_array())
                    .and_then(|messages| messages.last())
                    .and_then(|last| last.get("role"))
                    .and_then(|role| role.as_str())
                    .map(|role| role != "user")
                    .unwrap_or(false);

                (is_vision, is_agent)
            }
            "/responses" => {
                // Vision: input[].content[].type == "input_image"
                let is_vision = json
                    .get("input")
                    .and_then(|i| i.as_array())
                    .map(|input| {
                        input.iter().any(|item| {
                            item.get("content")
                                .and_then(|c| c.as_array())
                                .map(|parts| {
                                    parts.iter().any(|part| {
                                        part.get("type").and_then(|t| t.as_str())
                                            == Some("input_image")
                                    })
                                })
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false);

                // Agent: input[-1].role != "user"
                let is_agent = json
                    .get("input")
                    .and_then(|i| i.as_array())
                    .and_then(|input| input.last())
                    .and_then(|last| last.get("role"))
                    .and_then(|role| role.as_str())
                    .map(|role| role != "user")
                    .unwrap_or(false);

                (is_vision, is_agent)
            }
            _ => (false, false),
        }
    }
}

#[async_trait]
impl ProxyHttp for CopilotProxy {
    type CTX = RequestCtx;

    fn new_ctx(&self) -> Self::CTX {
        RequestCtx::default()
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        let req = session.req_header();

        // Reject Transfer-Encoding to prevent request smuggling
        if req.headers.get("transfer-encoding").is_some() {
            return session
                .respond_error(400)
                .await
                .map(|_| true)
                .map_err(|e| e.into_in());
        }

        // Validate proxy API key if configured
        if self.config.api_key.is_some() {
            let provided_key = Self::extract_api_key(req);
            match provided_key {
                Some(key) if self.validate_api_key(&key) => {}
                _ => {
                    return session
                        .respond_error(401)
                        .await
                        .map(|_| true)
                        .map_err(|e| e.into_in());
                }
            }
        }

        // Get method and path
        let method = req.method.as_str();
        let path = req.uri.path();
        let endpoint = Self::strip_v1_prefix(path).unwrap_or_else(|| path.to_string());

        // Check endpoint allowlist
        if !Self::is_endpoint_allowed(method, &endpoint) {
            return session
                .respond_error(404)
                .await
                .map(|_| true)
                .map_err(|e| e.into_in());
        }

        // Check GitHub OAuth authentication
        let auth = self.auth_state.read().await;
        if auth.is_none() {
            return session
                .respond_error(401)
                .await
                .map(|_| true)
                .map_err(|e| e.into_in());
        }

        // Store endpoint for body inspection
        ctx.endpoint = endpoint;

        Ok(false)
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let auth = self.auth_state.read().await;
        let auth_state = auth.as_ref().expect("Auth state checked in request_filter");

        let host = auth_state.domain.api_host();

        // Resolve DNS with TTL-based caching (hickory-resolver handles TTL automatically)
        let response = self
            .resolver
            .lookup_ip(&host)
            .await
            .map_err(|e| Error::explain(ErrorType::InternalError, format!("DNS resolution failed: {}", e)))?;

        let ip = response
            .iter()
            .next()
            .ok_or_else(|| Error::explain(ErrorType::InternalError, "No IP addresses found"))?;

        let addr = SocketAddr::new(ip, 443);

        let mut peer = HttpPeer::new(addr, true, host);
        peer.options.set_http_version(2, 1);

        Ok(Box::new(peer))
    }

    async fn request_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<Bytes>,
        _end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        if let Some(data) = body {
            // Check body size limit
            if ctx.body_buffer.len() + data.len() > MAX_BODY_SIZE {
                return Err(Error::explain(
                    ErrorType::InvalidHTTPHeader,
                    "Request body too large",
                ));
            }
            ctx.body_buffer.extend_from_slice(data);
        }

        Ok(())
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Self::CTX,
    ) -> Result<()> {
        if let Some(stripped_path) = Self::strip_v1_prefix(upstream_request.uri.path()) {
            let path_and_query = match upstream_request.uri.query() {
                Some(query) => format!("{stripped_path}?{query}"),
                None => stripped_path,
            };
            let uri = path_and_query.parse().map_err(|_| {
                Error::explain(ErrorType::InvalidHTTPHeader, "invalid rewritten URI")
            })?;
            upstream_request.set_uri(uri);
        }

        let auth = self.auth_state.read().await;
        let auth_state = auth.as_ref().expect("Auth state checked in request_filter");

        // Inspect body for vision/agent detection
        let (is_vision, is_agent) = Self::inspect_body(&ctx.endpoint, &ctx.body_buffer);
        ctx.is_vision = is_vision;
        ctx.is_agent = is_agent;

        // Step 1: Save headers we want to preserve from client
        let content_type = upstream_request
            .headers
            .get("content-type")
            .map(|v| v.to_str().unwrap_or("application/json").to_string());
        let content_length = upstream_request
            .headers
            .get("content-length")
            .map(|v| v.to_str().unwrap_or("0").to_string());
        let accept = upstream_request
            .headers
            .get("accept")
            .map(|v| v.to_str().unwrap_or("*/*").to_string());

        // Step 2: Collect all header names to remove
        let headers_to_remove: Vec<String> = upstream_request
            .headers
            .keys()
            .map(|k| k.as_str().to_string())
            .collect();

        // Step 3: Remove all client headers
        for header in headers_to_remove {
            let _ = upstream_request.remove_header(&header);
        }

        // Step 4: Restore preserved client headers
        if let Some(v) = content_type {
            upstream_request.insert_header("Content-Type", v)?;
        }
        if let Some(v) = content_length {
            upstream_request.insert_header("Content-Length", v)?;
        }
        if let Some(v) = accept {
            upstream_request.insert_header("Accept", v)?;
        }

        // Step 5: Add proxy-controlled headers
        let host = auth_state.domain.api_host();
        upstream_request.insert_header("Host", &host)?;
        upstream_request.insert_header(
            "User-Agent",
            format!("opencode/{}", DEFAULT_OPENCODE_VERSION),
        )?;
        upstream_request.insert_header(
            "Authorization",
            format!("Bearer {}", auth_state.github_token.access_token),
        )?;
        upstream_request.insert_header("Openai-Intent", "conversation-edits")?;
        upstream_request.insert_header(
            "X-Initiator",
            if ctx.is_agent { "agent" } else { "user" },
        )?;

        if ctx.is_vision {
            upstream_request.insert_header("Copilot-Vision-Request", "true")?;
        }

        Ok(())
    }

    async fn logging(
        &self,
        session: &mut Session,
        _e: Option<&Error>,
        ctx: &mut Self::CTX,
    ) {
        let response_code = session
            .response_written()
            .map(|r| r.status.as_u16())
            .unwrap_or(0);

        // Safe to log: request paths, status codes, timing
        // Never log: tokens, API keys, Authorization headers
        log::info!(
            "{} {} - {} (vision={}, agent={})",
            session.req_header().method,
            ctx.endpoint,
            response_code,
            ctx.is_vision,
            ctx.is_agent
        );
    }
}

#[cfg(test)]
mod tests {
    use super::CopilotProxy;

    #[test]
    fn normalizes_chat_completions_path() {
        assert_eq!(
            CopilotProxy::strip_v1_prefix("/v1/chat/completions").as_deref(),
            Some("/chat/completions")
        );
        assert_eq!(CopilotProxy::strip_v1_prefix("/chat/completions"), None);
        assert_eq!(CopilotProxy::strip_v1_prefix("/v1/messages"), None);
        assert_eq!(
            CopilotProxy::strip_v1_prefix("/v1/responses").as_deref(),
            Some("/responses")
        );
    }

    #[test]
    fn allows_v1_messages_endpoint() {
        assert!(CopilotProxy::is_endpoint_allowed("POST", "/v1/messages"));
    }
}
