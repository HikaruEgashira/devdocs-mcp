use reqwest::Client;
use html2md::parse_html;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use mcp_core::ToolError;

/// DevDocs.io Client for fetching documentation from various languages and frameworks
#[derive(Clone)]
pub struct DevDocsClient {
    client: Client,
    cache: Arc<Mutex<std::collections::HashMap<String, String>>>,
}

impl Default for DevDocsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DevDocsClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Get cache entry
    pub async fn get_cache(&self, key: &str) -> Option<String> {
        let cache = self.cache.lock().await;
        cache.get(key).cloned()
    }

    /// Set cache entry
    pub async fn set_cache(&self, key: String, value: String) {
        let mut cache = self.cache.lock().await;
        cache.insert(key, value);
    }

    /// List available documentations on DevDocs.io
    pub async fn list_documentations(&self) -> Result<String, ToolError> {
        // Check cache first
        let cache_key = "devdocs:list";
        if let Some(cached) = self.get_cache(cache_key).await {
            return Ok(cached);
        }

        // Fetch the list from DevDocs.io
        let url = "https://devdocs.io/docs.json";
        let response = self.client
            .get(url)
            .header("User-Agent", "CodeNav-MCP/0.1.0")
            .send()
            .await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to fetch DevDocs list: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolError::ExecutionError(format!(
                "Failed to fetch DevDocs list. Status: {}",
                response.status()
            )));
        }

        let json_response = response.text().await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to read response: {}", e)))?;

        // Cache the result
        self.set_cache(cache_key.to_string(), json_response.clone()).await;

        Ok(json_response)
    }

    /// Get documentation for a specific documentation slug and entry
    pub async fn get_documentation(&self, slug: String, entry: Option<String>) -> Result<String, ToolError> {
        // Construct cache key
        let cache_key = if let Some(e) = &entry {
            format!("devdocs:{}:{}", slug, e)
        } else {
            format!("devdocs:{}", slug)
        };

        // Check cache first
        if let Some(cached) = self.get_cache(&cache_key).await {
            return Ok(cached);
        }

        // Determine URL based on whether an entry was specified
        let url = if let Some(entry_path) = entry {
            format!("https://devdocs.io/{}/{}", slug, entry_path)
        } else {
            format!("https://devdocs.io/{}", slug)
        };

        // Fetch the documentation
        let response = self.client
            .get(&url)
            .header("User-Agent", "CodeNav-MCP/0.1.0")
            .send()
            .await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to fetch DevDocs documentation: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolError::ExecutionError(format!(
                "Failed to fetch DevDocs documentation. Status: {}",
                response.status()
            )));
        }

        let html_body = response.text().await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to read response body: {}", e)))?;

        // Convert HTML to markdown
        let markdown_content = parse_html(&html_body);

        // Cache the result
        self.set_cache(cache_key, markdown_content.clone()).await;

        Ok(markdown_content)
    }

    /// Search for documentation entries
    pub async fn search_documentation(&self, slug: String, query: String) -> Result<String, ToolError> {
        // Form the cache key based on slug and query
        let cache_key = format!("devdocs:search:{}:{}", slug, query);

        // Check cache first
        if let Some(cached) = self.get_cache(&cache_key).await {
            return Ok(cached);
        }

        // DevDocs doesn't have a public API for search, so we'll use their website search
        // This is not ideal, but it works for now
        let url = format!("https://devdocs.io/{}/?q={}", slug, query);

        // Fetch the search results page
        let response = self.client
            .get(&url)
            .header("User-Agent", "CodeNav-MCP/0.1.0")
            .send()
            .await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to search DevDocs: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolError::ExecutionError(format!(
                "Failed to search DevDocs. Status: {}",
                response.status()
            )));
        }

        let html_body = response.text().await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to read response body: {}", e)))?;

        // Convert HTML to markdown
        let markdown_content = parse_html(&html_body);

        // Cache the result
        self.set_cache(cache_key, markdown_content.clone()).await;

        Ok(markdown_content)
    }
}