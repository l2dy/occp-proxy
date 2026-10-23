# occp-proxy

A GitHub Copilot authentication and API proxy server using OpenCode's auth.

## Features

- OAuth device flow authentication with GitHub Copilot
- HTTP proxy for Copilot API requests
- Support for both github.com and GitHub Enterprise
- Optional API key authentication for the proxy itself

## Building

```bash
cargo build --release
```

## Usage

### Login

```bash
# For github.com
occp-proxy login

# For GitHub Enterprise
occp-proxy login your-enterprise.github.com
```

### Start the proxy

```bash
# Basic usage
occp-proxy serve

# With custom port and API key
occp-proxy serve --port 8080 --api-key "your-secret-key"

# Allow up to 2 minutes for in-flight requests to finish after SIGTERM
occp-proxy serve --grace-period-seconds 120

# API key can also be set via environment variable
OCCP_PROXY_API_KEY="your-secret-key" occp-proxy serve
```

The graceful shutdown grace period defaults to 300 seconds (5 minutes) and can be
customized with `--grace-period-seconds`. This applies to graceful shutdowns such
as `SIGTERM`; `SIGINT` continues to use Pingora's quick-shutdown behavior.

### Other commands

```bash
occp-proxy status   # Show authentication status
occp-proxy logout   # Remove saved authentication
occp-proxy config   # Show configuration directory
```

## Proxy endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | `/chat/completions` | Chat completions API |
| POST | `/responses` | Responses API (for codex models) |

## Troubleshooting

If you receive an error like `"The requested model is not supported"`, make sure the model is enabled in your GitHub Copilot settings.

## License

AGPL-3.0
