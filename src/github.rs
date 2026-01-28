use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::Deserialize;

use crate::{
    error::{ContentError, Result},
    source::ContentSource,
    types::{DirectoryEntry, DirectoryListing, EntryType, FileContent},
};

/// GitHub-backed content source
/// 
/// Fetches content from a GitHub repository using:
/// - raw.githubusercontent.com for file downloads
/// - GitHub REST API for directory listings
#[derive(Clone)]
pub struct GitHubSource {
    client: Client,
    owner: String,
    repo: String,
    branch: String,
    base_path: String,
}

#[derive(Deserialize)]
struct GitHubApiEntry {
    name: String,
    path: String,
    #[serde(rename = "type")]
    entry_type: String,
}

impl GitHubSource {
    /// Create a new GitHub source
    /// 
    /// # Arguments
    /// * `owner` - Repository owner (user or organization)
    /// * `repo` - Repository name
    /// * `branch` - Branch or ref to fetch from
    /// * `base_path` - Base path inside the repository (empty string for root)
    pub fn new(owner: String, repo: String, branch: String, base_path: String) -> Self {
        let client = Client::builder()
            .user_agent("content-resolver/0.1")
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            owner,
            repo,
            branch,
            base_path,
        }
    }

    /// Build the raw content URL for a file
    fn raw_url(&self, path: &str) -> String {
        let full_path = self.join_path(path);
        format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{}",
            self.owner, self.repo, self.branch, full_path
        )
    }

    /// Build the API URL for directory listings
    fn api_url(&self, path: &str) -> String {
        let full_path = self.join_path(path);
        format!(
            "https://api.github.com/repos/{}/{}/contents/{}?ref={}",
            self.owner, self.repo, full_path, self.branch
        )
    }

    /// Join base_path with a relative path
    fn join_path(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        if self.base_path.is_empty() {
            path.to_string()
        } else {
            format!("{}/{}", self.base_path.trim_end_matches('/'), path)
        }
    }

    /// Check if an error is a rate limit error
    fn is_rate_limit_error(&self, status: StatusCode) -> bool {
        status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS
    }
}

#[async_trait]
impl ContentSource for GitHubSource {
    async fn fetch_file(&self, path: &str) -> Result<FileContent> {
        let url = self.raw_url(path);
        
        let response = self.client.get(&url).send().await?;
        
        match response.status() {
            StatusCode::OK => {
                let etag = response
                    .headers()
                    .get("etag")
                    .and_then(|v| v.to_str().ok())
                    .map(String::from);
                
                let content = response.bytes().await?;
                
                Ok(FileContent {
                    content,
                    source_path: url.clone(),
                    etag,
                })
            }
            StatusCode::NOT_FOUND => Err(ContentError::NotFound {
                path: path.to_string(),
            }),
            status if self.is_rate_limit_error(status) => {
                let message = response.text().await.unwrap_or_else(|_| {
                    "GitHub API rate limit exceeded".to_string()
                });
                Err(ContentError::RateLimited { message })
            }
            status => {
                let message = format!("Unexpected status {}: {}", status, 
                    response.text().await.unwrap_or_default());
                Err(ContentError::InvalidStructure { message })
            }
        }
    }

    async fn list_directory(&self, path: &str) -> Result<DirectoryListing> {
        let url = self.api_url(path);
        
        let response = self.client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await?;
        
        match response.status() {
            StatusCode::OK => {
                let api_entries: Vec<GitHubApiEntry> = response.json().await?;
                
                let entries = api_entries
                    .into_iter()
                    .map(|e| DirectoryEntry {
                        name: e.name,
                        path: e.path,
                        entry_type: match e.entry_type.as_str() {
                            "file" => EntryType::File,
                            "dir" => EntryType::Dir,
                            _ => EntryType::File, // Default to file for unknown types
                        },
                    })
                    .collect();
                
                Ok(DirectoryListing {
                    path: path.to_string(),
                    entries,
                })
            }
            StatusCode::NOT_FOUND => Err(ContentError::NotFound {
                path: path.to_string(),
            }),
            status if self.is_rate_limit_error(status) => {
                let message = response.text().await.unwrap_or_else(|_| {
                    "GitHub API rate limit exceeded".to_string()
                });
                Err(ContentError::RateLimited { message })
            }
            status => {
                let message = format!("Unexpected status {}: {}", status,
                    response.text().await.unwrap_or_default());
                Err(ContentError::InvalidStructure { message })
            }
        }
    }

    fn identifier(&self) -> String {
        format!("github://{}/{}/{}/{}", 
            self.owner, self.repo, self.branch, self.base_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_path() {
        let source = GitHubSource::new(
            "owner".to_string(),
            "repo".to_string(),
            "main".to_string(),
            "base/path".to_string(),
        );

        assert_eq!(source.join_path("file.txt"), "base/path/file.txt");
        assert_eq!(source.join_path("/file.txt"), "base/path/file.txt");
    }

    #[test]
    fn test_join_path_empty_base() {
        let source = GitHubSource::new(
            "owner".to_string(),
            "repo".to_string(),
            "main".to_string(),
            "".to_string(),
        );

        assert_eq!(source.join_path("file.txt"), "file.txt");
        assert_eq!(source.join_path("/file.txt"), "file.txt");
    }
}
