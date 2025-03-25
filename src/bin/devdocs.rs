use anyhow::Result;
use clap::{Parser, Subcommand};
use devdocs_mcp::tools::DocRouter;
use mcp_core::Content;
use mcp_server::router::RouterService;
use mcp_server::{ByteTransport, Router, Server};
use serde_json::json;
use std::net::SocketAddr;
use tokio::io::{stdin, stdout};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{self, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(author, version = "0.1.0", about, long_about = None)]
#[command(propagate_version = true)]
#[command(disable_version_flag = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the server in stdin/stdout mode
    Stdio {
        /// Enable debug logging
        #[arg(short, long)]
        debug: bool,
    },
    /// Run the server with HTTP/SSE interface
    Http {
        /// Address to bind the HTTP server to
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        address: String,
        
        /// Enable debug logging
        #[arg(short, long)]
        debug: bool,
    },
    /// Test tools directly from the CLI
    Test {
        /// The tool to test (lookup_crate, search_crates, lookup_item)
        #[arg(long, default_value = "lookup_crate")]
        tool: String,
        
        /// Crate name for lookup_crate and lookup_item
        #[arg(long)]
        crate_name: Option<String>,
        
        /// Item path for lookup_item (e.g., std::vec::Vec)
        #[arg(long)]
        item_path: Option<String>,
        
        /// Search query for search_crates
        #[arg(long)]
        query: Option<String>,
        
        /// Crate version (optional)
        #[arg(long)]
        version: Option<String>,
        
        /// Result limit for search_crates
        #[arg(long)]
        limit: Option<u32>,
        
        /// Output format (markdown, text, json)
        #[arg(long, default_value = "markdown")]
        format: Option<String>,
        
        /// Output file path (if not specified, results will be printed to stdout)
        #[arg(long)]
        output: Option<String>,
        
        /// Enable debug logging
        #[arg(short, long)]
        debug: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Stdio { debug } => run_stdio_server(debug).await,
        Commands::Http { address, debug } => run_http_server(address, debug).await,
        Commands::Test { 
            tool, 
            crate_name, 
            item_path, 
            query, 
            version, 
            limit,
            format,
            output,
            debug 
        } => run_test_tool(TestToolConfig {
            tool,
            crate_name,
            item_path,
            query,
            version,
            limit,
            format,
            output,
            debug
        }).await,
    }
}

async fn run_stdio_server(debug: bool) -> Result<()> {
    // Set up file appender for logging
    let file_appender = RollingFileAppender::new(Rotation::DAILY, "logs", "stdio-server.log");

    // Initialize the tracing subscriber with file logging
    let level = if debug { tracing::Level::DEBUG } else { tracing::Level::INFO };
    
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(level.into()))
        .with_writer(file_appender)
        .with_target(false)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    tracing::info!("Starting MCP documentation server in STDIN/STDOUT mode");

    // Create an instance of our documentation router
    let router = RouterService(DocRouter::new());

    // Create and run the server
    let server = Server::new(router);
    let transport = ByteTransport::new(stdin(), stdout());

    tracing::info!("Documentation server initialized and ready to handle requests");
    Ok(server.run(transport).await?)
}

async fn run_http_server(address: String, debug: bool) -> Result<()> {
    // Setup tracing
    let level = if debug { "debug" } else { "info" };
    
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{},{}", level, env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Parse socket address
    let addr: SocketAddr = address.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::debug!("Rust Documentation Server listening on {}", listener.local_addr()?);
    tracing::info!("Access the Rust Documentation Server at http://{}/sse", addr);
    
    // Create app and run server
    let app = devdocs_mcp::transport::http_sse_server::App::new();
    axum::serve(listener, app.router()).await?;
    
    Ok(())
}

/// Configuration for the test tool
struct TestToolConfig {
    tool: String,
    crate_name: Option<String>,
    item_path: Option<String>,
    query: Option<String>,
    version: Option<String>,
    limit: Option<u32>,
    format: Option<String>,
    output: Option<String>,
    debug: bool,
}

/// Run a direct test of a documentation tool from the CLI
async fn run_test_tool(config: TestToolConfig) -> Result<()> {
    let TestToolConfig {
        tool,
        crate_name,
        item_path,
        query,
        version,
        limit,
        format,
        output,
        debug,
    } = config;
    // Print help information if the tool is "help"
    if tool == "help" {
        println!("devdocs CLI Tool Tester\n");
        println!("Usage examples:");
        println!("  cargo run --bin devdocs -- test --tool lookup_crate --crate-name serde");
        println!("  cargo run --bin devdocs -- test --tool lookup_crate --crate-name tokio --version 1.35.0");
        println!("  cargo run --bin devdocs -- test --tool lookup_item --crate-name tokio --item-path sync::mpsc::Sender");
        println!("  cargo run --bin devdocs -- test --tool lookup_item --crate-name serde --item-path Serialize --version 1.0.147");
        println!("  cargo run --bin devdocs -- test --tool search_crates --query logger --limit 5");
        println!("  cargo run --bin devdocs -- test --tool search_crates --query logger --format json");
        println!("  cargo run --bin devdocs -- test --tool lookup_crate --crate-name tokio --output tokio-docs.md");
        println!("\nAvailable tools:");
        println!("  lookup_crate   - Look up documentation for a Rust crate");
        println!("  lookup_item    - Look up documentation for a specific item in a crate");
        println!("                   Format: 'module::path::ItemName' (e.g., 'sync::mpsc::Sender')");
        println!("                   The tool will try to detect if it's a struct, enum, trait, fn, or macro");
        println!("  search_crates  - Search for crates on crates.io");
        println!("  help           - Show this help information");
        println!("\nOutput options:");
        println!("  --format       - Output format: markdown (default), text, json");
        println!("  --output       - Write output to a file instead of stdout");
        return Ok(());
    }
    
    // Set up console logging
    let level = if debug { tracing::Level::DEBUG } else { tracing::Level::INFO };
    
    tracing_subscriber::fmt()
        .with_max_level(level)
        .without_time()
        .with_target(false)
        .init();

    // Create router instance
    let router = DocRouter::new();
    
    tracing::info!("Testing tool: {}", tool);
    
    // Get format option (default to markdown)
    let format = format.unwrap_or_else(|| "markdown".to_string());
    
    // Prepare arguments based on the tool being tested
    let arguments = match tool.as_str() {
        "lookup_crate" => {
            let crate_name = crate_name.ok_or_else(|| 
                anyhow::anyhow!("--crate-name is required for lookup_crate tool"))?;
            
            json!({
                "crate_name": crate_name,
                "version": version,
            })
        },
        "lookup_item" => {
            let crate_name = crate_name.ok_or_else(|| 
                anyhow::anyhow!("--crate-name is required for lookup_item tool"))?;
            let item_path = item_path.ok_or_else(|| 
                anyhow::anyhow!("--item-path is required for lookup_item tool"))?;
            
            json!({
                "crate_name": crate_name,
                "item_path": item_path,
                "version": version,
            })
        },
        "search_crates" => {
            let query = query.ok_or_else(|| 
                anyhow::anyhow!("--query is required for search_crates tool"))?;
            
            json!({
                "query": query,
                "limit": limit,
            })
        },
        _ => return Err(anyhow::anyhow!("Unknown tool: {}", tool)),
    };
    
    // Call the tool and get results
    tracing::debug!("Calling {} with arguments: {}", tool, arguments);
    println!("Executing {} tool...", tool);
    
    let result = match router.call_tool(&tool, arguments).await {
        Ok(result) => result,
        Err(e) => {
            eprintln!("\nERROR: {}", e);
            eprintln!("\nTip: Try these suggestions:");
            eprintln!("  - For crate docs: cargo run --bin devdocs -- test --tool lookup_crate --crate-name tokio");
            eprintln!("  - For item lookup: cargo run --bin devdocs -- test --tool lookup_item --crate-name tokio --item-path sync::mpsc::Sender");
            eprintln!("  - For item lookup with version: cargo run --bin devdocs -- test --tool lookup_item --crate-name serde --item-path Serialize --version 1.0.147");
            eprintln!("  - For crate search: cargo run --bin devdocs -- test --tool search_crates --query logger --limit 5");
            eprintln!("  - For output format: cargo run --bin devdocs -- test --tool search_crates --query logger --format json");
            eprintln!("  - For file output: cargo run --bin devdocs -- test --tool lookup_crate --crate-name tokio --output tokio-docs.md");
            eprintln!("  - For help: cargo run --bin devdocs -- test --tool help");
            return Ok(());
        }
    };
    
    // Process and output results
    if !result.is_empty() {
        for content in result {
            if let Content::Text(text) = content {
                let content_str = text.text;
                let formatted_output = match format.as_str() {
                    "json" => {
                        // For search_crates, which may return JSON content
                        if tool == "search_crates" && content_str.trim().starts_with('{') {
                            // If content is already valid JSON, pretty print it
                            match serde_json::from_str::<serde_json::Value>(&content_str) {
                                Ok(json_value) => serde_json::to_string_pretty(&json_value)
                                    .unwrap_or_else(|_| content_str.clone()),
                                Err(_) => {
                                    // If it's not JSON, wrap it in a simple JSON object
                                    json!({ "content": content_str }).to_string()
                                }
                            }
                        } else {
                            // For non-JSON content, wrap in a JSON object
                            json!({ "content": content_str }).to_string()
                        }
                    },
                    "text" => {
                        // For JSON content, try to extract plain text
                        if content_str.trim().starts_with('{') && tool == "search_crates" {
                            match serde_json::from_str::<serde_json::Value>(&content_str) {
                                Ok(json_value) => {
                                    // Try to create a simple text representation of search results
                                    if let Some(crates) = json_value.get("crates").and_then(|v| v.as_array()) {
                                        let mut text_output = String::from("Search Results:\n\n");
                                        for (i, crate_info) in crates.iter().enumerate() {
                                            let name = crate_info.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown");
                                            let description = crate_info.get("description").and_then(|v| v.as_str()).unwrap_or("No description");
                                            let downloads = crate_info.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
                                            
                                            text_output.push_str(&format!("{}. {} - {} (Downloads: {})\n", 
                                                i + 1, name, description, downloads));
                                        }
                                        text_output
                                    } else {
                                        content_str
                                    }
                                },
                                Err(_) => content_str,
                            }
                        } else {
                            // For markdown content, use a simple approach to convert to plain text
                            // This is a very basic conversion - more sophisticated would need a proper markdown parser
                            content_str
                                .replace("# ", "")
                                .replace("## ", "")
                                .replace("### ", "")
                                .replace("#### ", "")
                                .replace("##### ", "")
                                .replace("###### ", "")
                                .replace("**", "")
                                .replace("*", "")
                                .replace("`", "")
                        }
                    },
                    _ => content_str, // Default to original markdown for "markdown" or any other format
                };
                
                // Output to file or stdout
                match &output {
                    Some(file_path) => {
                        use std::fs;
                        use std::io::Write;
                        
                        tracing::info!("Writing output to file: {}", file_path);
                        
                        // Ensure parent directory exists
                        if let Some(parent) = std::path::Path::new(file_path).parent() {
                            if !parent.exists() {
                                fs::create_dir_all(parent)?;
                            }
                        }
                        
                        let mut file = fs::File::create(file_path)?;
                        file.write_all(formatted_output.as_bytes())?;
                        println!("Results written to file: {}", file_path);
                    },
                    None => {
                        // Print to stdout
                        println!("\n--- TOOL RESULT ---\n");
                        println!("{}", formatted_output);
                        println!("\n--- END RESULT ---");
                    }
                }
            } else {
                println!("Received non-text content");
            }
        }
    } else {
        println!("Tool returned no results");
    }
    
    Ok(())
}