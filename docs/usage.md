# DovDocs MCP Usage Guide

This guide explains how to use the DovDocs MCP server with different types of clients.

## Client Integration

### Using with MCP-compatible LLMs

Any LLM client that follows the Model Context Protocol (MCP) can connect to this documentation server. The LLM will gain the ability to:

1. Look up documentation for any Rust crate
2. Search the crates.io registry for libraries
3. Get documentation for specific items within crates

### Command-Line Client

For testing purposes, you can use a simple command-line client like this one:

```rust
use mcp_client::{Client, transport::StdioTransport};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create a client using stdio transport
    let transport = StdioTransport::new();
    let mut client = Client::new(transport);
    
    // Connect to the server
    client.connect().await?;
    
    // Example: Looking up the 'tokio' crate
    let response = client.call_tool(
        "lookup_crate", 
        serde_json::json!({
            "crate_name": "tokio"
        })
    ).await?;
    
    println!("Documentation response: {}", response[0].text());
    
    Ok(())
}
```

### Web Client

When using the Axum SSE mode, you can connect to the server using a simple web client:

```javascript
// Connect to the SSE endpoint
const eventSource = new EventSource('http://127.0.0.1:8080/sse');

// Get the session ID from the initial connection
let sessionId;
eventSource.addEventListener('endpoint', (event) => {
    sessionId = event.data.split('=')[1];
    console.log(`Connected with session ID: ${sessionId}`);
});

// Handle messages from the server
eventSource.addEventListener('message', (event) => {
    const data = JSON.parse(event.data);
    console.log('Received response:', data);
});

// Function to send a tool request
async function callTool(toolName, args) {
    const response = await fetch(`http://127.0.0.1:8080/sse?sessionId=${sessionId}`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify({
            jsonrpc: '2.0',
            method: 'call_tool',
            params: {
                name: toolName,
                arguments: args
            },
            id: 1,
        }),
    });
    
    return response.ok;
}

// Example: Search for async crates
callTool('search_crates', { query: 'async runtime', limit: 5 });
```

## Using the CLI

The DevDocs MCP server can be started using the unified CLI:

```bash
# Show help
cargo run --bin devdocs -- --help

# Run in STDIN/STDOUT mode
cargo run --bin devdocs stdio

# Run in HTTP/SSE mode with default settings
cargo run --bin devdocs http

# Run HTTP server on custom address and port
cargo run --bin devdocs http --address 0.0.0.0:3000

# Enable debug logging
cargo run --bin devdocs http --debug
```

## Example Workflows

### Helping an LLM Understand a New Crate

1. LLM client connects to the documentation server
2. User asks a question involving an unfamiliar crate
3. LLM uses `lookup_crate` to get general documentation
4. LLM uses `lookup_item` to get specific details on functions/types
5. LLM can now provide an accurate response about the crate

### Helping Find the Right Library

1. User asks "What's a good crate for async HTTP requests?"
2. LLM uses `search_crates` with relevant keywords
3. LLM reviews the top results and their descriptions
4. LLM uses `lookup_crate` to get more details on promising options
5. LLM provides a recommendation with supporting information