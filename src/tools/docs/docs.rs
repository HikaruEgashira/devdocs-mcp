use std::{future::Future, pin::Pin, sync::Arc};

use mcp_core::{
    handler::{PromptError, ResourceError},
    prompt::Prompt,
    protocol::ServerCapabilities,
    Content, Resource, Tool, ToolError,
};
use mcp_server::router::CapabilitiesBuilder;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use html2md::parse_html;

use super::devdocs::DevDocsClient;
use super::npm::NpmClient;
use super::pypi::PyPIClient;
use super::golang::GoClient;

// Cache for documentation lookups to avoid repeated requests
#[derive(Clone)]
pub struct DocCache {
    cache: Arc<Mutex<std::collections::HashMap<String, String>>>,
}

impl Default for DocCache {
    fn default() -> Self {
        Self::new()
    }
}

impl DocCache {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        let cache = self.cache.lock().await;
        cache.get(key).cloned()
    }

    pub async fn set(&self, key: String, value: String) {
        let mut cache = self.cache.lock().await;
        cache.insert(key, value);
    }
}

#[derive(Clone)]
pub struct DocRouter {
    pub client: Client,
    pub cache: DocCache,
    pub devdocs_client: DevDocsClient,
    pub npm_client: NpmClient,
    pub pypi_client: PyPIClient,
    pub go_client: GoClient,
}

impl Default for DocRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl DocRouter {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            cache: DocCache::new(),
            devdocs_client: DevDocsClient::new(),
            npm_client: NpmClient::new(),
            pypi_client: PyPIClient::new(),
            go_client: GoClient::new(),
        }
    }

    // Fetch crate documentation from docs.rs
    async fn lookup_crate(&self, crate_name: String, version: Option<String>) -> Result<String, ToolError> {
        // Check cache first
        let cache_key = if let Some(ver) = &version {
            format!("{}:{}", crate_name, ver)
        } else {
            crate_name.clone()
        };

        if let Some(doc) = self.cache.get(&cache_key).await {
            return Ok(doc);
        }

        // Construct the docs.rs URL for the crate
        let url = if let Some(ver) = version {
            format!("https://docs.rs/crate/{}/{}/", crate_name, ver)
        } else {
            format!("https://docs.rs/crate/{}/", crate_name)
        };

        // Fetch the documentation page
        let response = self.client.get(&url)
            .header("User-Agent", "CodeNav/0.1.0 (https://github.com/HikaruEgashira/codenav-mcp)")
            .send()
            .await
            .map_err(|e| {
                ToolError::ExecutionError(format!("Failed to fetch documentation: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(ToolError::ExecutionError(format!(
                "Failed to fetch documentation. Status: {}",
                response.status()
            )));
        }

        let html_body = response.text().await.map_err(|e| {
            ToolError::ExecutionError(format!("Failed to read response body: {}", e))
        })?;
        
        // Convert HTML to markdown
        let markdown_body = parse_html(&html_body);
        // Cache the markdown result
        self.cache.set(cache_key, markdown_body.clone()).await;
        
        Ok(markdown_body)
    }

    // Search crates.io for crates matching a query
    async fn search_crates(&self, query: String, limit: Option<u32>) -> Result<String, ToolError> {
        let limit = limit.unwrap_or(10).min(100); // Cap at 100 results
        
        let url = format!("https://crates.io/api/v1/crates?q={}&per_page={}", query, limit);
        
        let response = self.client.get(&url)
            .header("User-Agent", "CodeNav/0.1.0 (https://github.com/HikaruEgashira/codenav-mcp)")
            .send()
            .await
            .map_err(|e| {
                ToolError::ExecutionError(format!("Failed to search crates.io: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(ToolError::ExecutionError(format!(
                "Failed to search crates.io. Status: {}",
                response.status()
            )));
        }

        let body = response.text().await.map_err(|e| {
            ToolError::ExecutionError(format!("Failed to read response body: {}", e))
        })?;
        
        // Check if response is JSON (API response) or HTML (web page)
        if body.trim().starts_with('{') {
            // This is likely JSON data, return as is
            Ok(body)
        } else {
            // This is likely HTML, convert to markdown
            Ok(parse_html(&body))
        }
    }

    // Get documentation for a specific item in a crate
    async fn lookup_item(&self, crate_name: String, mut item_path: String, version: Option<String>) -> Result<String, ToolError> {
        // Strip crate name prefix from the item path if it exists
        let crate_prefix = format!("{}::", crate_name);
        if item_path.starts_with(&crate_prefix) {
            item_path = item_path[crate_prefix.len()..].to_string();
        }

        // Check cache first
        let cache_key = if let Some(ver) = &version {
            format!("{}:{}:{}", crate_name, ver, item_path)
        } else {
            format!("{}:{}", crate_name, item_path)
        };

        if let Some(doc) = self.cache.get(&cache_key).await {
            return Ok(doc);
        }

        // Process the item path to determine the item type
        // Format: module::path::ItemName
        // Need to split into module path and item name, and guess item type
        let parts: Vec<&str> = item_path.split("::").collect();
        
        if parts.is_empty() {
            return Err(ToolError::InvalidParameters(
                "Invalid item path. Expected format: module::path::ItemName".to_string()
            ));
        }
        
        let item_name = parts.last().unwrap().to_string();
        let module_path = if parts.len() > 1 {
            parts[..parts.len()-1].join("/")
        } else {
            String::new()
        };
        
        // Try different item types (struct, enum, trait, fn)
        let item_types = ["struct", "enum", "trait", "fn", "macro"];
        let mut last_error = None;
        
        for item_type in item_types.iter() {
            // Construct the docs.rs URL for the specific item
            let url = if let Some(ver) = version.clone() {
                if module_path.is_empty() {
                    format!("https://docs.rs/{}/{}/{}/{}.{}.html", crate_name, ver, crate_name, item_type, item_name)
                } else {
                    format!("https://docs.rs/{}/{}/{}/{}/{}.{}.html", crate_name, ver, crate_name, module_path, item_type, item_name)
                }
            } else {
                if module_path.is_empty() {
                    format!("https://docs.rs/{}/latest/{}/{}.{}.html", crate_name, crate_name, item_type, item_name)
                } else {
                    format!("https://docs.rs/{}/latest/{}/{}/{}.{}.html", crate_name, crate_name, module_path, item_type, item_name)
                }
            };
            
            // Try to fetch the documentation page
            let response = match self.client.get(&url)
                .header("User-Agent", "CodeNav/0.1.0 (https://github.com/HikaruEgashira/codenav-mcp)")
                .send().await {
                Ok(resp) => resp,
                Err(e) => {
                    last_error = Some(e.to_string());
                    continue;
                }
            };
            
            // If found, process and return
            if response.status().is_success() {
                let html_body = response.text().await.map_err(|e| {
                    ToolError::ExecutionError(format!("Failed to read response body: {}", e))
                })?;
                
                // Convert HTML to markdown
                let markdown_body = parse_html(&html_body);
                
                // Cache the markdown result
                self.cache.set(cache_key, markdown_body.clone()).await;
                
                return Ok(markdown_body);
            }
            
            last_error = Some(format!("Status code: {}", response.status()));
        }
        
        // If we got here, none of the item types worked
        Err(ToolError::ExecutionError(format!(
            "Failed to fetch item documentation. No matching item found. Last error: {}",
            last_error.unwrap_or_else(|| "Unknown error".to_string())
        )))
    }

    // DevDocs.io support methods
    
    // List available documentation sets in DevDocs.io
    async fn list_devdocs_documentations(&self) -> Result<String, ToolError> {
        self.devdocs_client.list_documentations().await
    }
    
    // Get documentation for a specific slug in DevDocs.io
    async fn get_devdocs_documentation(&self, slug: String, entry: Option<String>) -> Result<String, ToolError> {
        self.devdocs_client.get_documentation(slug, entry).await
    }
    
    // Search documentation in DevDocs.io
    async fn search_devdocs_documentation(&self, slug: String, query: String) -> Result<String, ToolError> {
        self.devdocs_client.search_documentation(slug, query).await
    }
    
    // NPM support methods
    
    // Get npm package information
    async fn lookup_npm_package(&self, package_name: String, version: Option<String>) -> Result<String, ToolError> {
        self.npm_client.lookup_package(package_name, version).await
    }
    
    // Search npm packages
    async fn search_npm_packages(&self, query: String, limit: Option<u32>) -> Result<String, ToolError> {
        self.npm_client.search_packages(query, limit).await
    }
    
    // PyPI support methods
    
    // Get Python package information from PyPI
    async fn lookup_pypi_package(&self, package_name: String, version: Option<String>) -> Result<String, ToolError> {
        self.pypi_client.lookup_package(package_name, version).await
    }
    
    // Search Python packages on PyPI
    async fn search_pypi_packages(&self, query: String, limit: Option<u32>) -> Result<String, ToolError> {
        self.pypi_client.search_packages(query, limit).await
    }
    
    // Go support methods
    
    // Get Go package information from pkg.go.dev
    async fn lookup_go_package(&self, package_name: String, version: Option<String>) -> Result<String, ToolError> {
        self.go_client.lookup_package(package_name, version).await
    }
    
    // Search Go packages on pkg.go.dev
    async fn search_go_packages(&self, query: String, limit: Option<u32>) -> Result<String, ToolError> {
        self.go_client.search_packages(query, limit).await
    }
    
    // Look up documentation for a specific symbol in a Go package
    async fn lookup_go_symbol(&self, package_name: String, symbol_name: String, version: Option<String>) -> Result<String, ToolError> {
        self.go_client.lookup_item(package_name, symbol_name, version).await
    }

    async fn lookup_go_item(&self, package_name: String, item_path: String, version: Option<String>) -> Result<String, ToolError> {
        self.go_client.lookup_item(package_name, item_path, version).await
    }
}

impl mcp_server::Router for DocRouter {
    fn name(&self) -> String {
        "codenav-docs".to_string()
    }

    fn instructions(&self) -> String {
        "This server provides tools for looking up documentation for various programming languages and frameworks. \
        Supported documentation sources include Rust crates (docs.rs), JavaScript/TypeScript packages (npm), \
        Python packages (PyPI), and various other languages and frameworks via DevDocs.io. \
        You can search for packages, lookup documentation for specific packages or items within packages. \
        Use these tools to find information about libraries you are not familiar with. \
        All HTML documentation is automatically converted to markdown for better compatibility with language models.".to_string()
    }

    fn capabilities(&self) -> ServerCapabilities {
        CapabilitiesBuilder::new()
            .with_tools(true)
            .with_resources(false, false)
            .with_prompts(false)
            .build()
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            // Rust documentation tools
            Tool::new(
                "lookup_crate".to_string(),
                "Look up documentation for a Rust crate (returns markdown)".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "crate_name": {
                            "type": "string",
                            "description": "The name of the crate to look up"
                        },
                        "version": {
                            "type": "string",
                            "description": "The version of the crate (optional, defaults to latest)"
                        }
                    },
                    "required": ["crate_name"]
                }),
            ),
            Tool::new(
                "search_crates".to_string(),
                "Search for Rust crates on crates.io (returns JSON or markdown)".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results to return (optional, defaults to 10, max 100)"
                        }
                    },
                    "required": ["query"]
                }),
            ),
            Tool::new(
                "lookup_item".to_string(),
                "Look up documentation for a specific item in a Rust crate (returns markdown)".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "crate_name": {
                            "type": "string",
                            "description": "The name of the crate"
                        },
                        "item_path": {
                            "type": "string",
                            "description": "Path to the item (e.g., 'vec::Vec' or 'crate_name::vec::Vec' - crate prefix will be automatically stripped)"
                        },
                        "version": {
                            "type": "string",
                            "description": "The version of the crate (optional, defaults to latest)"
                        }
                    },
                    "required": ["crate_name", "item_path"]
                }),
            ),
            
            // DevDocs.io tools
            Tool::new(
                "list_devdocs_documentations".to_string(),
                "List available documentation sets in DevDocs.io".to_string(),
                json!({
                    "type": "object",
                    "properties": {}
                }),
            ),
            Tool::new(
                "get_devdocs_documentation".to_string(),
                "Get documentation for a specific slug in DevDocs.io".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "slug": {
                            "type": "string",
                            "description": "The documentation slug (e.g., 'javascript', 'python', 'react')"
                        },
                        "entry": {
                            "type": "string",
                            "description": "Optional entry path within the documentation"
                        }
                    },
                    "required": ["slug"]
                }),
            ),
            Tool::new(
                "search_devdocs_documentation".to_string(),
                "Search within a specific documentation in DevDocs.io".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "slug": {
                            "type": "string",
                            "description": "The documentation slug (e.g., 'javascript', 'python', 'react')"
                        },
                        "query": {
                            "type": "string",
                            "description": "The search query"
                        }
                    },
                    "required": ["slug", "query"]
                }),
            ),
            
            // NPM tools
            Tool::new(
                "lookup_npm_package".to_string(),
                "Look up documentation for an NPM package".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "package_name": {
                            "type": "string",
                            "description": "The name of the npm package"
                        },
                        "version": {
                            "type": "string",
                            "description": "The version of the package (optional, defaults to latest)"
                        }
                    },
                    "required": ["package_name"]
                }),
            ),
            Tool::new(
                "search_npm_packages".to_string(),
                "Search for NPM packages".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results to return (optional, defaults to 10, max 100)"
                        }
                    },
                    "required": ["query"]
                }),
            ),
            
            // PyPI tools
            Tool::new(
                "lookup_pypi_package".to_string(),
                "Look up documentation for a Python package on PyPI".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "package_name": {
                            "type": "string",
                            "description": "The name of the Python package"
                        },
                        "version": {
                            "type": "string",
                            "description": "The version of the package (optional, defaults to latest)"
                        }
                    },
                    "required": ["package_name"]
                }),
            ),
            Tool::new(
                "search_pypi_packages".to_string(),
                "Search for Python packages on PyPI".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results to return (optional, defaults to 10, max 100)"
                        }
                    },
                    "required": ["query"]
                }),
            ),
            
            // Go tools
            Tool::new(
                "lookup_go_package".to_string(),
                "Look up documentation for a Go package on pkg.go.dev".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "package_name": {
                            "type": "string",
                            "description": "The name of the Go package"
                        },
                        "version": {
                            "type": "string",
                            "description": "The version of the package (optional, defaults to latest)"
                        }
                    },
                    "required": ["package_name"]
                }),
            ),
            Tool::new(
                "search_go_packages".to_string(),
                "Search for Go packages on pkg.go.dev".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results to return (optional, defaults to 10, max 100)"
                        }
                    },
                    "required": ["query"]
                }),
            ),
            Tool::new(
                "lookup_go_symbol".to_string(),
                "Look up documentation for a specific symbol in a Go package on pkg.go.dev".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "package_name": {
                            "type": "string",
                            "description": "The name of the Go package"
                        },
                        "symbol_name": {
                            "type": "string",
                            "description": "The name of the symbol"
                        },
                        "version": {
                            "type": "string",
                            "description": "The version of the package (optional, defaults to latest)"
                        }
                    },
                    "required": ["package_name", "symbol_name"]
                }),
            ),
            Tool::new(
                "lookup_go_item".to_string(),
                "Look up documentation for a specific item in a Go package".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "package_name": {
                            "type": "string",
                            "description": "The name of the Go package"
                        },
                        "item_path": {
                            "type": "string",
                            "description": "The path to the item in the package"
                        },
                        "version": {
                            "type": "string",
                            "description": "The version of the package (optional, defaults to latest)"
                        }
                    },
                    "required": ["package_name", "item_path"]
                }),
            ),
        ]
    }

    fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Content>, ToolError>> + Send + 'static>> {
        let this = self.clone();
        let tool_name = tool_name.to_string();
        let arguments = arguments.clone();
        Box::pin(async move {
            match tool_name.as_str() {
                // Rust documentation tools
                "lookup_crate" => {
                    let crate_name = arguments
                        .get("crate_name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("crate_name is required".to_string()))?
                        .to_string();
                    
                    let version = arguments
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    
                    let doc = this.lookup_crate(crate_name, version).await?;
                    Ok(vec![Content::text(doc)])
                }
                "search_crates" => {
                    let query = arguments
                        .get("query")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("query is required".to_string()))?
                        .to_string();
                    
                    let limit = arguments
                        .get("limit")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32);
                    
                    let results = this.search_crates(query, limit).await?;
                    Ok(vec![Content::text(results)])
                }
                "lookup_item" => {
                    let crate_name = arguments
                        .get("crate_name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("crate_name is required".to_string()))?
                        .to_string();
                    
                    let item_path = arguments
                        .get("item_path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("item_path is required".to_string()))?
                        .to_string();
                    
                    let version = arguments
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    
                    let doc = this.lookup_item(crate_name, item_path, version).await?;
                    Ok(vec![Content::text(doc)])
                },
                
                // DevDocs.io tools
                "list_devdocs_documentations" => {
                    let docs = this.list_devdocs_documentations().await?;
                    Ok(vec![Content::text(docs)])
                },
                "get_devdocs_documentation" => {
                    let slug = arguments
                        .get("slug")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("slug is required".to_string()))?
                        .to_string();
                    
                    let entry = arguments
                        .get("entry")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    
                    let doc = this.get_devdocs_documentation(slug, entry).await?;
                    Ok(vec![Content::text(doc)])
                },
                "search_devdocs_documentation" => {
                    let slug = arguments
                        .get("slug")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("slug is required".to_string()))?
                        .to_string();
                    
                    let query = arguments
                        .get("query")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("query is required".to_string()))?
                        .to_string();
                    
                    let results = this.search_devdocs_documentation(slug, query).await?;
                    Ok(vec![Content::text(results)])
                },
                
                // NPM tools
                "lookup_npm_package" => {
                    let package_name = arguments
                        .get("package_name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("package_name is required".to_string()))?
                        .to_string();
                    
                    let version = arguments
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    
                    let doc = this.lookup_npm_package(package_name, version).await?;
                    Ok(vec![Content::text(doc)])
                },
                "search_npm_packages" => {
                    let query = arguments
                        .get("query")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("query is required".to_string()))?
                        .to_string();
                    
                    let limit = arguments
                        .get("limit")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32);
                    
                    let results = this.search_npm_packages(query, limit).await?;
                    Ok(vec![Content::text(results)])
                },
                
                // PyPI tools
                "lookup_pypi_package" => {
                    let package_name = arguments
                        .get("package_name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("package_name is required".to_string()))?
                        .to_string();
                    
                    let version = arguments
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    
                    let doc = this.lookup_pypi_package(package_name, version).await?;
                    Ok(vec![Content::text(doc)])
                },
                "search_pypi_packages" => {
                    let query = arguments
                        .get("query")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("query is required".to_string()))?
                        .to_string();
                    
                    let limit = arguments
                        .get("limit")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32);
                    
                    let results = this.search_pypi_packages(query, limit).await?;
                    Ok(vec![Content::text(results)])
                },
                
                // Go tools
                "lookup_go_package" => {
                    let package_name = arguments
                        .get("package_name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("package_name is required".to_string()))?
                        .to_string();
                    
                    let version = arguments
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    
                    let doc = this.lookup_go_package(package_name, version).await?;
                    Ok(vec![Content::text(doc)])
                },
                "search_go_packages" => {
                    let query = arguments
                        .get("query")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("query is required".to_string()))?
                        .to_string();
                    
                    let limit = arguments
                        .get("limit")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32);
                    
                    let results = this.search_go_packages(query, limit).await?;
                    Ok(vec![Content::text(results)])
                },
                "lookup_go_symbol" => {
                    let package_name = arguments
                        .get("package_name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("package_name is required".to_string()))?
                        .to_string();
                    
                    let symbol_name = arguments
                        .get("symbol_name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("symbol_name is required".to_string()))?
                        .to_string();
                    
                    let version = arguments
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    
                    let doc = this.lookup_go_symbol(package_name, symbol_name, version).await?;
                    Ok(vec![Content::text(doc)])
                },
                "lookup_go_item" => {
                    let package_name = arguments
                        .get("package_name")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("package_name is required".to_string()))?
                        .to_string();

                    let item_path = arguments
                        .get("item_path")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ToolError::InvalidParameters("item_path is required".to_string()))?
                        .to_string();

                    let version = arguments
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let doc = this.lookup_go_item(package_name, item_path, version).await?;
                    Ok(vec![Content::text(doc)])
                },
                
                _ => Err(ToolError::NotFound(format!("Tool {} not found", tool_name))),
            }
        })
    }

    fn list_resources(&self) -> Vec<Resource> {
        vec![]
    }

    fn read_resource(
        &self,
        _uri: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, ResourceError>> + Send + 'static>> {
        Box::pin(async move {
            Err(ResourceError::NotFound("Resource not found".to_string()))
        })
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        vec![]
    }

    fn get_prompt(
        &self,
        prompt_name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<String, PromptError>> + Send + 'static>> {
        let prompt_name = prompt_name.to_string();
        Box::pin(async move {
            Err(PromptError::NotFound(format!(
                "Prompt {} not found",
                prompt_name
            )))
        })
    }
}