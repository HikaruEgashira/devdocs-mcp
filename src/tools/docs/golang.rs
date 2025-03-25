use mcp_core::ToolError;
use reqwest::Client;
use html2md::parse_html;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

#[derive(Clone)]
pub struct GoClient {
    client: Client,
    cache: Arc<Mutex<HashMap<String, String>>>,
}

impl Default for GoClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GoClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn lookup_package(&self, package_name: String, version: Option<String>) -> Result<String, ToolError> {
        let cache_key = if let Some(ver) = &version {
            format!("go:package:{}@{}", package_name, ver)
        } else {
            format!("go:package:{}", package_name)
        };

        if let Some(cached_doc) = self.cache.lock()
            .map_err(|e| ToolError::ExecutionError(format!("Cache lock error: {}", e)))?
            .get(&cache_key)
        {
            return Ok(cached_doc.clone());
        }

        let url = if let Some(ver) = version {
            format!("https://pkg.go.dev/{}@{}", package_name, ver)
        } else {
            format!("https://pkg.go.dev/{}", package_name)
        };
        
        let response = self.client.get(&url)
            .header("User-Agent", "CodeNav/0.1.0")
            .send()
            .await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to fetch Go package documentation: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolError::ExecutionError(format!(
                "Failed to fetch Go package documentation. Status: {}",
                response.status()
            )));
        }

        let html_body = response.text().await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to read response body: {}", e)))?;
        
        let markdown_body = parse_html(&html_body);

        self.cache.lock()
            .map_err(|e| ToolError::ExecutionError(format!("Cache lock error: {}", e)))?
            .insert(cache_key, markdown_body.clone());

        Ok(markdown_body)
    }

    pub async fn search_packages(&self, query: String, limit: Option<u32>) -> Result<String, ToolError> {
        let limit = limit.unwrap_or(10).min(100);
        let cache_key = format!("go:search:{}:{}", query, limit);

        if let Some(cached_results) = self.cache.lock()
            .map_err(|e| ToolError::ExecutionError(format!("Cache lock error: {}", e)))?
            .get(&cache_key)
        {
            return Ok(cached_results.clone());
        }

        let url = format!("https://pkg.go.dev/search?q={}&limit={}", query, limit);
        
        let response = self.client.get(&url)
            .header("User-Agent", "CodeNav/0.1.0")
            .send()
            .await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to search Go packages: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolError::ExecutionError(format!(
                "Failed to search Go packages. Status: {}",
                response.status()
            )));
        }

        let html_body = response.text().await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to read response body: {}", e)))?;
        
        let markdown_body = parse_html(&html_body);

        self.cache.lock()
            .map_err(|e| ToolError::ExecutionError(format!("Cache lock error: {}", e)))?
            .insert(cache_key, markdown_body.clone());

        Ok(markdown_body)
    }

    pub async fn lookup_item(&self, package_name: String, item_path: String, version: Option<String>) -> Result<String, ToolError> {
        let cache_key = if let Some(ver) = &version {
            format!("go:item:{}@{}#{}", package_name, ver, item_path)
        } else {
            format!("go:item:{}#{}", package_name, item_path)
        };

        if let Some(cached_doc) = self.cache.lock()
            .map_err(|e| ToolError::ExecutionError(format!("Cache lock error: {}", e)))?
            .get(&cache_key)
        {
            return Ok(cached_doc.clone());
        }

        let url = if let Some(ver) = version {
            format!("https://pkg.go.dev/{}@{}#{}", package_name, ver, item_path)
        } else {
            format!("https://pkg.go.dev/{}#{}", package_name, item_path)
        };

        let response = self.client.get(&url)
            .header("User-Agent", "CodeNav/0.1.0")
            .send()
            .await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to fetch Go item documentation: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolError::ExecutionError(format!(
                "Failed to fetch Go item documentation. Status: {}",
                response.status()
            )));
        }

        let html_body = response.text().await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to read response body: {}", e)))?;
        
        let markdown_body = parse_html(&html_body);

        self.cache.lock()
            .map_err(|e| ToolError::ExecutionError(format!("Cache lock error: {}", e)))?
            .insert(cache_key, markdown_body.clone());

        Ok(markdown_body)
    }
}