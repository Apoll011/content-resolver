use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;

use crate::error::{ContentError, Result};

/// Cache interface for storing content
#[async_trait]
pub trait Cache: Send + Sync {
    /// Get cached content by key
    async fn get(&self, key: &str) -> Result<Option<Bytes>>;

    /// Store content in cache
    async fn set(&self, key: &str, value: Bytes) -> Result<()>;

    /// Check if a key exists in the cache
    async fn contains(&self, key: &str) -> bool;

    /// Remove a key from the cache
    async fn remove(&self, key: &str) -> Result<()>;

    /// Clear all cached content
    async fn clear(&self) -> Result<()>;
}

/// In-memory cache implementation
pub struct MemoryCache {
    store: Arc<RwLock<HashMap<String, Bytes>>>,
}

impl MemoryCache {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Cache for MemoryCache {
    async fn get(&self, key: &str) -> Result<Option<Bytes>> {
        let store = self.store.read().await;
        Ok(store.get(key).cloned())
    }

    async fn set(&self, key: &str, value: Bytes) -> Result<()> {
        let mut store = self.store.write().await;
        store.insert(key.to_string(), value);
        Ok(())
    }

    async fn contains(&self, key: &str) -> bool {
        let store = self.store.read().await;
        store.contains_key(key)
    }

    async fn remove(&self, key: &str) -> Result<()> {
        let mut store = self.store.write().await;
        store.remove(key);
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        let mut store = self.store.write().await;
        store.clear();
        Ok(())
    }
}

/// Disk-based cache implementation
pub struct DiskCache {
    root_dir: PathBuf,
}

impl DiskCache {
    /// Create a new disk cache at the specified directory
    pub async fn new(root_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root_dir).await?;
        Ok(Self { root_dir })
    }

    /// Convert a cache key to a safe file path
    fn key_to_path(&self, key: &str) -> PathBuf {
        // Use SHA-256 hash to create a safe filename
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let hash = hasher.finalize();
        let hash_str = format!("{:x}", hash);
        
        self.root_dir.join(&hash_str[..2]).join(&hash_str[2..])
    }
}

#[async_trait]
impl Cache for DiskCache {
    async fn get(&self, key: &str) -> Result<Option<Bytes>> {
        let path = self.key_to_path(key);
        
        match fs::read(&path).await {
            Ok(data) => Ok(Some(Bytes::from(data))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ContentError::Cache {
                message: format!("Failed to read from disk cache: {}", e),
            }),
        }
    }

    async fn set(&self, key: &str, value: Bytes) -> Result<()> {
        let path = self.key_to_path(key);
        
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        
        fs::write(&path, &value).await.map_err(|e| ContentError::Cache {
            message: format!("Failed to write to disk cache: {}", e),
        })
    }

    async fn contains(&self, key: &str) -> bool {
        let path = self.key_to_path(key);
        path.exists()
    }

    async fn remove(&self, key: &str) -> Result<()> {
        let path = self.key_to_path(key);
        
        match fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(ContentError::Cache {
                message: format!("Failed to remove from disk cache: {}", e),
            }),
        }
    }

    async fn clear(&self) -> Result<()> {
        // Remove the entire cache directory and recreate it
        fs::remove_dir_all(&self.root_dir).await?;
        fs::create_dir_all(&self.root_dir).await?;
        Ok(())
    }
}

/// No-op cache that doesn't cache anything
pub struct NoCache;

#[async_trait]
impl Cache for NoCache {
    async fn get(&self, _key: &str) -> Result<Option<Bytes>> {
        Ok(None)
    }

    async fn set(&self, _key: &str, _value: Bytes) -> Result<()> {
        Ok(())
    }

    async fn contains(&self, _key: &str) -> bool {
        false
    }

    async fn remove(&self, _key: &str) -> Result<()> {
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_cache() {
        let cache = MemoryCache::new();
        let key = "test_key";
        let value = Bytes::from("test_value");

        // Initially empty
        assert!(!cache.contains(key).await);
        assert!(cache.get(key).await.unwrap().is_none());

        // Set and get
        cache.set(key, value.clone()).await.unwrap();
        assert!(cache.contains(key).await);
        assert_eq!(cache.get(key).await.unwrap().unwrap(), value);

        // Remove
        cache.remove(key).await.unwrap();
        assert!(!cache.contains(key).await);

        // Clear
        cache.set("key1", Bytes::from("val1")).await.unwrap();
        cache.set("key2", Bytes::from("val2")).await.unwrap();
        cache.clear().await.unwrap();
        assert!(!cache.contains("key1").await);
        assert!(!cache.contains("key2").await);
    }
}
