use std::sync::Arc;

use crate::{
    cache::Cache,
    error::{ContentError, Result},
    source::ContentSource,
    types::{DirectoryListing, FileContent},
};

/// Resolves content from multiple sources with fallback support
/// 
/// Searches sources in order and returns the first match.
/// Optionally caches results to reduce network requests.
pub struct ResourceResolver {
    sources: Vec<Arc<dyn ContentSource>>,
    cache: Option<Arc<dyn Cache>>,
}

impl ResourceResolver {
    /// Create a new resolver with the given sources
    pub fn new(sources: Vec<Arc<dyn ContentSource>>) -> Self {
        Self {
            sources,
            cache: None,
        }
    }

    /// Create a new resolver with caching enabled
    pub fn with_cache(
        sources: Vec<Arc<dyn ContentSource>>,
        cache: Arc<dyn Cache>,
    ) -> Self {
        Self {
            sources,
            cache: Some(cache),
        }
    }

    /// Fetch a file by path, searching sources in order
    /// 
    /// Returns the first successful match, or NotFound if none match
    pub async fn fetch_file(&self, path: &str) -> Result<FileContent> {
        // Generate cache key from path
        let cache_key = format!("file:{}", path);

        // Check cache first if enabled
        if let Some(cache) = &self.cache {
            if let Some(cached) = cache.get(&cache_key).await? {
                return Ok(FileContent {
                    content: cached,
                    source_path: format!("cache:{}", path),
                    etag: None,
                });
            }
        }

        // Try each source in order
        let mut last_error = None;

        for source in &self.sources {
            match source.fetch_file(path).await {
                Ok(content) => {
                    // Cache the result if caching is enabled
                    if let Some(cache) = &self.cache {
                        let _ = cache.set(&cache_key, content.content.clone()).await;
                    }
                    return Ok(content);
                }
                Err(ContentError::NotFound { .. }) => {
                    // Continue to next source on not found
                    continue;
                }
                Err(e) => {
                    // Store other errors but continue trying
                    last_error = Some(e);
                }
            }
        }

        // If we got a non-NotFound error, return it
        if let Some(error) = last_error {
            return Err(error);
        }

        // Nothing found in any source
        Err(ContentError::NotFound {
            path: path.to_string(),
        })
    }

    /// List directory contents, searching sources in order
    /// 
    /// Returns the first successful match
    pub async fn list_directory(&self, path: &str) -> Result<DirectoryListing> {
        let mut last_error = None;

        for source in &self.sources {
            match source.list_directory(path).await {
                Ok(listing) => return Ok(listing),
                Err(ContentError::NotFound { .. }) => {
                    continue;
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }

        if let Some(error) = last_error {
            return Err(error);
        }

        Err(ContentError::NotFound {
            path: path.to_string(),
        })
    }

    /// List directory contents across all sources, merging results
    /// 
    /// This aggregates entries from all sources that successfully list the directory
    pub async fn list_directory_merged(&self, path: &str) -> Result<DirectoryListing> {
        let mut all_entries = Vec::new();
        let mut found_any = false;

        for source in &self.sources {
            if let Ok(listing) = source.list_directory(path).await {
                found_any = true;
                all_entries.extend(listing.entries);
            }
        }

        if !found_any {
            return Err(ContentError::NotFound {
                path: path.to_string(),
            });
        }

        // Deduplicate by path
        all_entries.sort_by(|a, b| a.path.cmp(&b.path));
        all_entries.dedup_by(|a, b| a.path == b.path);

        Ok(DirectoryListing {
            path: path.to_string(),
            entries: all_entries,
        })
    }

    /// Check if a file exists in any source
    pub async fn file_exists(&self, path: &str) -> bool {
        for source in &self.sources {
            if source.file_exists(path).await {
                return true;
            }
        }
        false
    }

    /// Get the list of sources
    pub fn sources(&self) -> &[Arc<dyn ContentSource>] {
        &self.sources
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::MemoryCache;
    use crate::types::EntryType;
    use async_trait::async_trait;
    use bytes::Bytes;

    struct MockSource {
        files: Vec<(&'static str, &'static str)>,
    }

    #[async_trait]
    impl ContentSource for MockSource {
        async fn fetch_file(&self, path: &str) -> Result<FileContent> {
            for (file_path, content) in &self.files {
                if *file_path == path {
                    return Ok(FileContent {
                        content: Bytes::from(*content),
                        source_path: path.to_string(),
                        etag: None,
                    });
                }
            }
            Err(ContentError::NotFound {
                path: path.to_string(),
            })
        }

        async fn list_directory(&self, _path: &str) -> Result<DirectoryListing> {
            Err(ContentError::NotFound {
                path: "".to_string(),
            })
        }

        fn identifier(&self) -> String {
            "mock".to_string()
        }
    }

    #[tokio::test]
    async fn test_fallback_resolution() {
        let source1 = Arc::new(MockSource {
            files: vec![("file1.txt", "from source 1")],
        });
        let source2 = Arc::new(MockSource {
            files: vec![("file2.txt", "from source 2")],
        });

        let resolver = ResourceResolver::new(vec![
            source1 as Arc<dyn ContentSource>,
            source2 as Arc<dyn ContentSource>,
        ]);

        // File from first source
        let result = resolver.fetch_file("file1.txt").await.unwrap();
        assert_eq!(result.content, Bytes::from("from source 1"));

        // File from second source
        let result = resolver.fetch_file("file2.txt").await.unwrap();
        assert_eq!(result.content, Bytes::from("from source 2"));

        // File not in any source
        assert!(matches!(
            resolver.fetch_file("missing.txt").await,
            Err(ContentError::NotFound { .. })
        ));
    }

    #[tokio::test]
    async fn test_caching() {
        let source = Arc::new(MockSource {
            files: vec![("file.txt", "content")],
        });
        let cache = Arc::new(MemoryCache::new());

        let resolver = ResourceResolver::with_cache(
            vec![source as Arc<dyn ContentSource>],
            cache.clone(),
        );

        // First fetch - from source
        let result = resolver.fetch_file("file.txt").await.unwrap();
        assert_eq!(result.content, Bytes::from("content"));

        // Check cache was populated
        assert!(cache.contains("file:file.txt").await);

        // Second fetch - from cache
        let result = resolver.fetch_file("file.txt").await.unwrap();
        assert_eq!(result.content, Bytes::from("content"));
        assert_eq!(result.source_path, "cache:file.txt");
    }
}
