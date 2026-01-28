use async_trait::async_trait;
use crate::{error::Result, types::{DirectoryListing, FileContent}};

/// Core abstraction for content sources
/// 
/// Implementors provide read-only access to files and directories
/// from various backends (Git repositories, local filesystem, etc.)
#[async_trait]
pub trait ContentSource: Send + Sync {
    /// Fetch a single file by its path
    /// 
    /// Returns `ContentError::NotFound` if the file doesn't exist
    async fn fetch_file(&self, path: &str) -> Result<FileContent>;

    /// List the contents of a directory
    /// 
    /// Returns `ContentError::NotFound` if the directory doesn't exist
    async fn list_directory(&self, path: &str) -> Result<DirectoryListing>;

    /// Get a human-readable identifier for this source (for logging/debugging)
    fn identifier(&self) -> String;

    /// Check if a file exists without fetching it
    /// 
    /// Default implementation attempts to fetch and returns true if successful
    async fn file_exists(&self, path: &str) -> bool {
        self.fetch_file(path).await.is_ok()
    }
}
