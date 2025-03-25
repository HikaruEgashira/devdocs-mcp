# Development Guide

This guide provides information for developers who want to contribute to or modify the DovDocs MCP server.

## Architecture Overview

The server consists of several key components:

1. **DocRouter** (`src/docs.rs`):
   - Core implementation of the MCP Router trait
   - Handles tool calls for documentation lookup
   - Implements caching to avoid redundant API requests

2. **Transport Implementations**:
   - STDIN/STDOUT server (`src/bin/stdio_server.rs`)
   - HTTP/SSE server (`src/bin/axum_docs.rs`)

3. **Utilities**:
   - JSON-RPC frame codec for byte stream handling

## Adding New Features

### Adding a New Tool

To add a new tool to the documentation server:

1. Add the implementation function in `DocRouter` struct
2. Add the tool definition to the `list_tools()` method
3. Add the tool handler in the `call_tool()` match statement

Example:

```rust
// 1. Add the implementation function
async fn get_crate_examples(&self, crate_name: String, limit: Option<u32>) -> Result<String, ToolError> {
    // Implementation details...
}

// 2. In list_tools() add:
Tool::new(
    "get_crate_examples".to_string(),
    "Get usage examples for a Rust crate".to_string(),
    json!({
        "type": "object",
        "properties": {
            "crate_name": {
                "type": "string",
                "description": "The name of the crate"
            },
            "limit": {
                "type": "integer",
                "description": "Maximum number of examples to return"
            }
        },
        "required": ["crate_name"]
    }),
),

// 3. In call_tool() match statement:
"get_crate_examples" => {
    let crate_name = arguments
        .get("crate_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidParameters("crate_name is required".to_string()))?
        .to_string();
    
    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    
    let examples = this.get_crate_examples(crate_name, limit).await?;
    Ok(vec![Content::text(examples)])
}
```

### Enhancing the Cache

The current cache implementation is basic. To enhance it:

1. Add TTL (Time-To-Live) for cache entries
2. Add cache size limits to prevent memory issues
3. Consider using a more sophisticated caching library

## Testing

Create test files that implement basic tests for the server:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::test;

    #[test]
    async fn test_search_crates() {
        let router = DocRouter::new();
        let result = router.search_crates("tokio".to_string(), Some(2)).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert!(data.contains("crates"));
    }

    #[test]
    async fn test_lookup_crate() {
        let router = DocRouter::new();
        let result = router.lookup_crate("serde".to_string(), None).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert!(data.contains("serde"));
    }
}
```

## Deployment

For production deployment, consider:

1. Rate limiting to prevent abuse
2. Authentication for sensitive documentation
3. HTTPS for secure communication
4. Docker containerization for easier deployment

Example Dockerfile:

```dockerfile
FROM rust:1.74-slim as builder
WORKDIR /usr/src/app
COPY . .
RUN cargo build --release

FROM debian:stable-slim
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/app/target/release/axum-docs /usr/local/bin/

EXPOSE 8080
CMD ["axum-docs"]
```