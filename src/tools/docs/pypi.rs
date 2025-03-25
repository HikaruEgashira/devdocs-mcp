use reqwest::Client;
use serde_json::Value;
use html2md::parse_html;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use mcp_core::ToolError;

/// PyPI Client for fetching Python package documentation from PyPI
#[derive(Clone)]
pub struct PyPIClient {
    client: Client,
    cache: Arc<Mutex<std::collections::HashMap<String, String>>>,
}

impl Default for PyPIClient {
    fn default() -> Self {
        Self::new()
    }
}

impl PyPIClient {
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

    /// Lookup Python package metadata
    pub async fn lookup_package(&self, package_name: String, version: Option<String>) -> Result<String, ToolError> {
        // Create cache key
        let cache_key = if let Some(ver) = &version {
            format!("pypi:{}:{}", package_name, ver)
        } else {
            format!("pypi:{}", package_name)
        };

        // Check cache
        if let Some(cached) = self.get_cache(&cache_key).await {
            return Ok(cached);
        }

        // Construct the URL for package info
        let url = match &version {
            Some(ver) => format!("https://pypi.org/pypi/{}/{}/json", package_name, ver),
            None => format!("https://pypi.org/pypi/{}/json", package_name),
        };

        // Fetch package information
        let response = self.client
            .get(&url)
            .header("User-Agent", "CodeNav-MCP/0.1.0")
            .send()
            .await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to fetch PyPI package: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolError::ExecutionError(format!(
                "Failed to fetch PyPI package. Status: {}",
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
        
        // Package name
        markdown.push_str(&format!("# {}\n\n", package_name));
        
        // Extract info
        if let Some(info) = parsed_json.get("info") {
            // Version
            if let Some(version) = info.get("version").and_then(|v| v.as_str()) {
                markdown.push_str(&format!("**Version:** {}\n\n", version));
            }
            
            // Summary
            if let Some(summary) = info.get("summary").and_then(|v| v.as_str()) {
                markdown.push_str(&format!("{}\n\n", summary));
            }
            
            // Author
            if let Some(author) = info.get("author").and_then(|v| v.as_str()) {
                if let Some(author_email) = info.get("author_email").and_then(|v| v.as_str()) {
                    markdown.push_str(&format!("**Author:** {} ({})\n\n", author, author_email));
                } else {
                    markdown.push_str(&format!("**Author:** {}\n\n", author));
                }
            }
            
            // Homepage
            if let Some(homepage) = info.get("home_page").and_then(|v| v.as_str()) {
                markdown.push_str(&format!("**Homepage:** {}\n\n", homepage));
            }
            
            // License
            if let Some(license) = info.get("license").and_then(|v| v.as_str()) {
                markdown.push_str(&format!("**License:** {}\n\n", license));
            }
            
            // Classifiers
            if let Some(classifiers) = info.get("classifiers").and_then(|v| v.as_array()) {
                markdown.push_str("**Classifiers:**\n\n");
                for classifier in classifiers {
                    if let Some(c) = classifier.as_str() {
                        markdown.push_str(&format!("- {}\n", c));
                    }
                }
                markdown.push_str("\n");
            }
            
            // Project URLs
            if let Some(project_urls) = info.get("project_urls").and_then(|v| v.as_object()) {
                markdown.push_str("**Project Links:**\n\n");
                for (name, url) in project_urls {
                    if let Some(u) = url.as_str() {
                        markdown.push_str(&format!("- {}: {}\n", name, u));
                    }
                }
                markdown.push_str("\n");
            }
            
            // Description
            if let Some(description) = info.get("description").and_then(|v| v.as_str()) {
                markdown.push_str("## Description\n\n");
                
                // Check if description is already markdown
                if info.get("description_content_type")
                    .and_then(|v| v.as_str())
                    .map_or(false, |t| t.contains("markdown") || t.contains("md"))
                {
                    markdown.push_str(description);
                } else if info.get("description_content_type")
                    .and_then(|v| v.as_str())
                    .map_or(false, |t| t.contains("text/html") || t.contains("html"))
                {
                    // Convert HTML to markdown
                    markdown.push_str(&parse_html(description));
                } else {
                    // Plain text, preserve as is
                    markdown.push_str("```\n");
                    markdown.push_str(description);
                    markdown.push_str("\n```\n");
                }
            }
            
            // Requirements
            if let Some(requires_dist) = info.get("requires_dist").and_then(|v| v.as_array()) {
                markdown.push_str("\n## Requirements\n\n");
                for req in requires_dist {
                    if let Some(r) = req.as_str() {
                        markdown.push_str(&format!("- {}\n", r));
                    }
                }
                markdown.push_str("\n");
            }
        }
        
        // Fetch and add documentation URL if available
        if let Some(info) = parsed_json.get("info") {
            if let Some(docs_url) = info.get("project_urls")
                .and_then(|urls| urls.get("Documentation"))
                .and_then(|v| v.as_str())
            {
                markdown.push_str(&format!("\n## Documentation\n\nFor full documentation, visit: {}\n\n", docs_url));
            } else if let Some(docs_url) = info.get("documentation_url").and_then(|v| v.as_str()) {
                markdown.push_str(&format!("\n## Documentation\n\nFor full documentation, visit: {}\n\n", docs_url));
            }
        }
        
        // Cache the result
        self.set_cache(cache_key, markdown.clone()).await;
        
        Ok(markdown)
    }

    /// Search PyPI packages
    pub async fn search_packages(&self, query: String, limit: Option<u32>) -> Result<String, ToolError> {
        let limit = limit.unwrap_or(10).min(100); // Cap at 100 results
        let cache_key = format!("pypi:search:{}:{}", query, limit);
        
        // Check cache
        if let Some(cached) = self.get_cache(&cache_key).await {
            return Ok(cached);
        }
        
        // Search PyPI using their JSON API
        let url = format!("https://pypi.org/search/?q={}&format=json", query);
        
        // Fetch search results
        let response = self.client
            .get(&url)
            .header("User-Agent", "CodeNav-MCP/0.1.0")
            .send()
            .await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to search PyPI packages: {}", e)))?;
            
        if !response.status().is_success() {
            // PyPI doesn't have a well-documented JSON search API
            // As a fallback, we'll parse the HTML search page
            return self.scrape_search_results(query, Some(limit)).await;
        }
        
        let json_body = response.text().await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to read response body: {}", e)))?;
            
        // Try to parse JSON response
        match serde_json::from_str::<serde_json::Value>(&json_body) {
            Ok(parsed_json) => {
                // Format search results as markdown
                let mut markdown = String::new();
                
                markdown.push_str(&format!("# PyPI Search Results for '{}'\n\n", query));
                
                if let Some(results) = parsed_json.get("results").and_then(|v| v.as_array()) {
                    for (i, pkg) in results.iter().enumerate().take(limit as usize) {
                        let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown");
                        let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("Unknown");
                        let description = pkg.get("description").and_then(|v| v.as_str()).unwrap_or("No description");
                        
                        markdown.push_str(&format!("## {}. {} (v{})\n\n", i + 1, name, version));
                        markdown.push_str(&format!("{}\n\n", description));
                        
                        let pkg_url = format!("https://pypi.org/project/{}", name);
                        markdown.push_str(&format!("- [PyPI Page]({})\n\n", pkg_url));
                    }
                } else {
                    markdown.push_str("No packages found.\n");
                }
                
                // Cache the result
                self.set_cache(cache_key, markdown.clone()).await;
                
                Ok(markdown)
            },
            Err(_) => {
                // JSON parsing failed, fall back to HTML scraping
                self.scrape_search_results(query, Some(limit)).await
            }
        }
    }
    
    /// Fallback method to scrape search results from the HTML page
    async fn scrape_search_results(&self, query: String, limit: Option<u32>) -> Result<String, ToolError> {
        let limit = limit.unwrap_or(10).min(100);
        let cache_key = format!("pypi:search:{}:{}", query, limit);
        
        // Fetch the HTML search page
        let url = format!("https://pypi.org/search/?q={}", query);
        
        let response = self.client
            .get(&url)
            .header("User-Agent", "CodeNav-MCP/0.1.0")
            .send()
            .await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to search PyPI packages: {}", e)))?;
            
        if !response.status().is_success() {
            return Err(ToolError::ExecutionError(format!(
                "Failed to search PyPI packages. Status: {}",
                response.status()
            )));
        }
        
        let html_body = response.text().await
            .map_err(|e| ToolError::ExecutionError(format!("Failed to read response body: {}", e)))?;
            
        // Convert HTML to markdown
        let markdown = parse_html(&html_body);
        
        // Cache the result
        self.set_cache(cache_key, markdown.clone()).await;
        
        Ok(markdown)
    }
}