use reqwest::Client;
use serde_json::Value;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use mcp_core::ToolError;

/// NPM Client for fetching package documentation from npm registry
#[derive(Clone)]
pub struct NpmClient {
    client: Client,
    cache: Arc<Mutex<std::collections::HashMap<String, String>>>,
}

impl Default for NpmClient {
    fn default() -> Self {
        Self::new()
    }
}

impl NpmClient {
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

    /// Lookup NPM package information
    pub async fn lookup_package(&self, package_name: String, version: Option<String>) -> Result<String, ToolError> {
        // Create cache key
        let cache_key = if let Some(ver) = &version {
            format!("npm:{}:{}", package_name, ver)
        } else {
            format!("npm:{}", package_name)
        };

        // Check cache
        if let Some(cached) = self.get_cache(&cache_key).await {
            return Ok(cached);
        }

        // Construct the URL for package info
        let url = if let Some(ver) = &version {
            format!("https://registry.npmjs.org/{}/{}", package_name, ver)
        } else {
            format!("https://registry.npmjs.org/{}", package_name)
        };

        // Fetch package information
        let response = self.client
            .get(&url)
            .header("User-Agent", "CodeNav-MCP/0.1.0")
            .send()
            .await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to fetch npm package: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolError::ExecutionError(format!(
                "Failed to fetch npm package. Status: {}",
                response.status()
            )));
        }

        let json_body = response.text().await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to read response body: {}", e)))?;

        // Process and convert to markdown
        let parsed_json: Value = serde_json::from_str(&json_body)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to parse JSON: {}", e)))?;

        // Format package information as markdown
        let mut markdown = String::new();
        
        // Package name and version
        markdown.push_str(&format!("# {}\n\n", package_name));
        
        if let Some(version) = parsed_json.get("version").and_then(|v| v.as_str()) {
            markdown.push_str(&format!("**Version:** {}\n\n", version));
        }
        
        // Description
        if let Some(description) = parsed_json.get("description").and_then(|v| v.as_str()) {
            markdown.push_str(&format!("{}\n\n", description));
        }
        
        // Keywords
        if let Some(keywords) = parsed_json.get("keywords").and_then(|v| v.as_array()) {
            markdown.push_str("**Keywords:** ");
            for (i, keyword) in keywords.iter().enumerate() {
                if let Some(k) = keyword.as_str() {
                    if i > 0 {
                        markdown.push_str(", ");
                    }
                    markdown.push_str(k);
                }
            }
            markdown.push_str("\n\n");
        }
        
        // Homepage
        if let Some(homepage) = parsed_json.get("homepage").and_then(|v| v.as_str()) {
            markdown.push_str(&format!("**Homepage:** {}\n\n", homepage));
        }
        
        // Repository
        if let Some(repo) = parsed_json.get("repository").and_then(|v| v.get("url")).and_then(|v| v.as_str()) {
            markdown.push_str(&format!("**Repository:** {}\n\n", repo));
        }
        
        // License
        if let Some(license) = parsed_json.get("license").and_then(|v| v.as_str()) {
            markdown.push_str(&format!("**License:** {}\n\n", license));
        }
        
        // Dependencies
        if let Some(deps) = parsed_json.get("dependencies").and_then(|v| v.as_object()) {
            markdown.push_str("## Dependencies\n\n");
            for (dep_name, dep_version) in deps {
                if let Some(ver) = dep_version.as_str() {
                    markdown.push_str(&format!("- **{}**: {}\n", dep_name, ver));
                }
            }
            markdown.push_str("\n");
        }
        
        // Dev dependencies
        if let Some(deps) = parsed_json.get("devDependencies").and_then(|v| v.as_object()) {
            markdown.push_str("## Dev Dependencies\n\n");
            for (dep_name, dep_version) in deps {
                if let Some(ver) = dep_version.as_str() {
                    markdown.push_str(&format!("- **{}**: {}\n", dep_name, ver));
                }
            }
            markdown.push_str("\n");
        }
        
        // README (if available)
        if let Some(readme) = parsed_json.get("readme").and_then(|v| v.as_str()) {
            markdown.push_str("## Documentation\n\n");
            markdown.push_str(readme);
        }
        
        // Cache the result
        self.set_cache(cache_key, markdown.clone()).await;
        
        Ok(markdown)
    }

    /// Search NPM packages
    pub async fn search_packages(&self, query: String, limit: Option<u32>) -> Result<String, ToolError> {
        let limit = limit.unwrap_or(10).min(100); // Cap at 100 results
        let cache_key = format!("npm:search:{}:{}", query, limit);
        
        // Check cache
        if let Some(cached) = self.get_cache(&cache_key).await {
            return Ok(cached);
        }
        
        // Construct search URL
        let url = format!("https://registry.npmjs.org/-/v1/search?text={}&size={}", query, limit);
        
        // Fetch search results
        let response = self.client
            .get(&url)
            .header("User-Agent", "CodeNav-MCP/0.1.0")
            .send()
            .await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to search npm packages: {}", e)))?;
            
        if !response.status().is_success() {
            return Err(ToolError::ExecutionError(format!(
                "Failed to search npm packages. Status: {}",
                response.status()
            )));
        }
        
        let json_body = response.text().await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to read response body: {}", e)))?;
            
        // Parse JSON response
        let parsed_json: Value = serde_json::from_str(&json_body)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to parse JSON: {}", e)))?;
            
        // Format search results as markdown
        let mut markdown = String::new();
        
        markdown.push_str(&format!("# NPM Search Results for '{}'\n\n", query));
        
        if let Some(objects) = parsed_json.get("objects").and_then(|v| v.as_array()) {
            for (i, pkg) in objects.iter().enumerate() {
                if let Some(package_info) = pkg.get("package") {
                    let name = package_info.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown");
                    let version = package_info.get("version").and_then(|v| v.as_str()).unwrap_or("Unknown");
                    let description = package_info.get("description").and_then(|v| v.as_str()).unwrap_or("No description");
                    
                    markdown.push_str(&format!("## {}. {} (v{})\n\n", i + 1, name, version));
                    markdown.push_str(&format!("{}\n\n", description));
                    
                    // Add links
                    if let Some(links) = package_info.get("links") {
                        if let Some(npm) = links.get("npm").and_then(|v| v.as_str()) {
                            markdown.push_str(&format!("- [NPM]({})\n", npm));
                        }
                        if let Some(homepage) = links.get("homepage").and_then(|v| v.as_str()) {
                            markdown.push_str(&format!("- [Homepage]({})\n", homepage));
                        }
                        if let Some(repository) = links.get("repository").and_then(|v| v.as_str()) {
                            markdown.push_str(&format!("- [Repository]({})\n", repository));
                        }
                        markdown.push_str("\n");
                    }
                }
            }
        } else {
            markdown.push_str("No packages found.\n");
        }
        
        // Cache the result
        self.set_cache(cache_key, markdown.clone()).await;
        
        Ok(markdown)
    }
}