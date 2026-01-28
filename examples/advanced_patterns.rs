/// Advanced patterns and best practices for the content resolution system
///
/// This example demonstrates:
/// - Custom content source implementation
/// - Advanced error handling and retry logic
/// - Content validation and transformation
/// - Metrics and observability
/// - Production deployment patterns

use content_resolver::{
    Cache, ContentError, ContentSource, DirectoryListing, FileContent, MemoryCache,
    ResourceResolver,
};
use async_trait::async_trait;
use bytes::Bytes;
use std::sync::Arc;
use std::time::{Duration, Instant};

// ============================================================================
// Custom Content Source: Local Filesystem
// ============================================================================

/// A content source that reads from the local filesystem
/// This is useful for development or for reading local configuration overrides
pub struct LocalFileSource {
    root_path: std::path::PathBuf,
}

impl LocalFileSource {
    pub fn new(root_path: std::path::PathBuf) -> Self {
        Self { root_path }
    }

    fn resolve_path(&self, path: &str) -> std::path::PathBuf {
        self.root_path.join(path.trim_start_matches('/'))
    }
}

#[async_trait]
impl ContentSource for LocalFileSource {
    async fn fetch_file(&self, path: &str) -> content_resolver::Result<FileContent> {
        let full_path = self.resolve_path(path);

        let content = tokio::fs::read(&full_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ContentError::NotFound {
                    path: path.to_string(),
                }
            } else {
                ContentError::Io(e)
            }
        })?;

        Ok(FileContent {
            content: Bytes::from(content),
            source_path: full_path.to_string_lossy().to_string(),
            etag: None,
        })
    }

    async fn list_directory(&self, path: &str) -> content_resolver::Result<DirectoryListing> {
        let full_path = self.resolve_path(path);

        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&full_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ContentError::NotFound {
                    path: path.to_string(),
                }
            } else {
                ContentError::Io(e)
            }
        })?;

        while let Some(entry) = read_dir.next_entry().await? {
            let metadata = entry.metadata().await?;
            let name = entry.file_name().to_string_lossy().to_string();
            let entry_path = format!("{}/{}", path.trim_end_matches('/'), name);

            entries.push(content_resolver::DirectoryEntry {
                name,
                path: entry_path,
                entry_type: if metadata.is_dir() {
                    content_resolver::EntryType::Dir
                } else {
                    content_resolver::EntryType::File
                },
            });
        }

        Ok(DirectoryListing {
            path: path.to_string(),
            entries,
        })
    }

    fn identifier(&self) -> String {
        format!("local://{}", self.root_path.display())
    }
}

// ============================================================================
// Metrics and Observability
// ============================================================================

/// Wrapper that tracks metrics for a content source
pub struct InstrumentedSource {
    inner: Arc<dyn ContentSource>,
    fetch_count: std::sync::atomic::AtomicU64,
    list_count: std::sync::atomic::AtomicU64,
    error_count: std::sync::atomic::AtomicU64,
}

impl InstrumentedSource {
    pub fn new(source: Arc<dyn ContentSource>) -> Self {
        Self {
            inner: source,
            fetch_count: std::sync::atomic::AtomicU64::new(0),
            list_count: std::sync::atomic::AtomicU64::new(0),
            error_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn metrics(&self) -> SourceMetrics {
        SourceMetrics {
            fetch_count: self.fetch_count.load(std::sync::atomic::Ordering::Relaxed),
            list_count: self.list_count.load(std::sync::atomic::Ordering::Relaxed),
            error_count: self.error_count.load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceMetrics {
    pub fetch_count: u64,
    pub list_count: u64,
    pub error_count: u64,
}

#[async_trait]
impl ContentSource for InstrumentedSource {
    async fn fetch_file(&self, path: &str) -> content_resolver::Result<FileContent> {
        self.fetch_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        match self.inner.fetch_file(path).await {
            Ok(content) => Ok(content),
            Err(e) => {
                self.error_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Err(e)
            }
        }
    }

    async fn list_directory(&self, path: &str) -> content_resolver::Result<DirectoryListing> {
        self.list_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        match self.inner.list_directory(path).await {
            Ok(listing) => Ok(listing),
            Err(e) => {
                self.error_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Err(e)
            }
        }
    }

    fn identifier(&self) -> String {
        format!("instrumented({})", self.inner.identifier())
    }
}

// ============================================================================
// Advanced Retry Logic
// ============================================================================

/// Retry configuration
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            backoff_factor: 2.0,
        }
    }
}

/// Fetch with exponential backoff retry
pub async fn fetch_with_retry(
    resolver: &ResourceResolver,
    path: &str,
    config: &RetryConfig,
) -> content_resolver::Result<FileContent> {
    let mut attempts = 0;
    let mut delay = config.initial_delay;

    loop {
        attempts += 1;

        match resolver.fetch_file(path).await {
            Ok(content) => return Ok(content),
            Err(ContentError::Network(_)) | Err(ContentError::RateLimited { .. })
                if attempts < config.max_attempts =>
            {
                println!(
                    "Attempt {}/{} failed, retrying in {:?}...",
                    attempts, config.max_attempts, delay
                );

                tokio::time::sleep(delay).await;

                // Exponential backoff
                delay = std::cmp::min(
                    Duration::from_secs_f64(delay.as_secs_f64() * config.backoff_factor),
                    config.max_delay,
                );
            }
            Err(e) => return Err(e),
        }
    }
}

// ============================================================================
// Content Validation
// ============================================================================

/// Validator for content
pub trait ContentValidator: Send + Sync {
    fn validate(&self, content: &[u8]) -> Result<(), String>;
}

/// Validate that content is valid UTF-8
pub struct Utf8Validator;

impl ContentValidator for Utf8Validator {
    fn validate(&self, content: &[u8]) -> Result<(), String> {
        std::str::from_utf8(content)
            .map(|_| ())
            .map_err(|e| format!("Invalid UTF-8: {}", e))
    }
}

/// Validate JSON content
pub struct JsonValidator;

impl ContentValidator for JsonValidator {
    fn validate(&self, content: &[u8]) -> Result<(), String> {
        serde_json::from_slice::<serde_json::Value>(content)
            .map(|_| ())
            .map_err(|e| format!("Invalid JSON: {}", e))
    }
}

/// Validate content size
pub struct SizeValidator {
    max_size: usize,
}

impl SizeValidator {
    pub fn new(max_size: usize) -> Self {
        Self { max_size }
    }
}

impl ContentValidator for SizeValidator {
    fn validate(&self, content: &[u8]) -> Result<(), String> {
        if content.len() > self.max_size {
            Err(format!(
                "Content too large: {} bytes (max: {})",
                content.len(),
                self.max_size
            ))
        } else {
            Ok(())
        }
    }
}

/// Fetch and validate content
pub async fn fetch_and_validate(
    resolver: &ResourceResolver,
    path: &str,
    validators: &[&dyn ContentValidator],
) -> content_resolver::Result<FileContent> {
    let content = resolver.fetch_file(path).await?;

    for validator in validators {
        validator.validate(&content.content).map_err(|msg| {
            ContentError::InvalidStructure {
                message: format!("Validation failed for {}: {}", path, msg),
            }
        })?;
    }

    Ok(content)
}

// ============================================================================
// Content Transformation Pipeline
// ============================================================================

/// Transform content after fetching
pub trait ContentTransformer: Send + Sync {
    fn transform(&self, content: Bytes) -> content_resolver::Result<Bytes>;
}

/// Decompress gzipped content
pub struct GzipDecompressor;

impl ContentTransformer for GzipDecompressor {
    fn transform(&self, content: Bytes) -> content_resolver::Result<Bytes> {
        // In a real implementation, use flate2 crate
        // For this example, just pass through
        Ok(content)
    }
}

/// Parse and prettify JSON
pub struct JsonPrettifier;

impl ContentTransformer for JsonPrettifier {
    fn transform(&self, content: Bytes) -> content_resolver::Result<Bytes> {
        let value: serde_json::Value = serde_json::from_slice(&content)?;
        let pretty = serde_json::to_vec_pretty(&value)?;
        Ok(Bytes::from(pretty))
    }
}

// ============================================================================
// Production Deployment Pattern
// ============================================================================

/// Production-ready content system configuration
pub struct ProductionContentSystem {
    resolver: Arc<ResourceResolver>,
    metrics: Vec<Arc<InstrumentedSource>>,
}

impl ProductionContentSystem {
    pub async fn new(
        sources: Vec<Arc<dyn ContentSource>>,
        cache_dir: Option<std::path::PathBuf>,
    ) -> content_resolver::Result<Self> {
        // Wrap all sources with instrumentation
        let instrumented: Vec<Arc<InstrumentedSource>> = sources
            .into_iter()
            .map(|s| Arc::new(InstrumentedSource::new(s)))
            .collect();

        let sources_for_resolver: Vec<Arc<dyn ContentSource>> = instrumented
            .iter()
            .map(|s| s.clone() as Arc<dyn ContentSource>)
            .collect();

        // Set up cache
        let resolver = if let Some(cache_path) = cache_dir {
            let cache = Arc::new(content_resolver::DiskCache::new(cache_path).await?);
            Arc::new(ResourceResolver::with_cache(sources_for_resolver, cache))
        } else {
            let cache = Arc::new(MemoryCache::new());
            Arc::new(ResourceResolver::with_cache(sources_for_resolver, cache))
        };

        Ok(Self {
            resolver,
            metrics: instrumented,
        })
    }

    /// Fetch with all production safeguards
    pub async fn fetch_safe(
        &self,
        path: &str,
        max_size: usize,
    ) -> content_resolver::Result<FileContent> {
        let validators: Vec<&dyn ContentValidator> = vec![&SizeValidator::new(max_size)];

        fetch_with_retry(
            &self.resolver,
            path,
            &RetryConfig {
                max_attempts: 3,
                initial_delay: Duration::from_millis(500),
                max_delay: Duration::from_secs(30),
                backoff_factor: 2.0,
            },
        )
        .await
        .and_then(|content| {
            for validator in &validators {
                validator.validate(&content.content).map_err(|msg| {
                    ContentError::InvalidStructure {
                        message: format!("Validation failed: {}", msg),
                    }
                })?;
            }
            Ok(content)
        })
    }

    /// Get aggregated metrics from all sources
    pub fn get_metrics(&self) -> Vec<(String, SourceMetrics)> {
        self.metrics
            .iter()
            .map(|s| (s.identifier(), s.metrics()))
            .collect()
    }

    /// Health check
    pub async fn health_check(&self) -> bool {
        // Try fetching a known health check file
        self.resolver.fetch_file("health").await.is_ok()
    }
}

// ============================================================================
// Example Usage
// ============================================================================

#[tokio::main]
async fn main() -> content_resolver::Result<()> {
    println!("=== Advanced Patterns Example ===\n");

    // 1. Local filesystem source
    println!("1. Local Filesystem Source");
    println!("--------------------------");
    
    let local_source = Arc::new(LocalFileSource::new(
        std::path::PathBuf::from("/tmp")
    )) as Arc<dyn ContentSource>;
    
    println!("   Local source identifier: {}\n", local_source.identifier());

    // 2. Instrumented sources with metrics
    println!("2. Metrics and Observability");
    println!("----------------------------");

    let source = Arc::new(content_resolver::GitHubSource::new(
        "anthropics".to_string(),
        "anthropic-sdk-python".to_string(),
        "main".to_string(),
        "".to_string(),
    )) as Arc<dyn ContentSource>;

    let instrumented = Arc::new(InstrumentedSource::new(source));
    let resolver = Arc::new(ResourceResolver::new(vec![
        instrumented.clone() as Arc<dyn ContentSource>
    ]));

    // Make some requests
    let _ = resolver.fetch_file("README.md").await;
    let _ = resolver.fetch_file("LICENSE").await;
    let _ = resolver.fetch_file("nonexistent.txt").await;

    let metrics = instrumented.metrics();
    println!("   Fetch count: {}", metrics.fetch_count);
    println!("   Error count: {}\n", metrics.error_count);

    // 3. Content validation
    println!("3. Content Validation");
    println!("---------------------");

    let validators: Vec<&dyn ContentValidator> = vec![
        &Utf8Validator,
        &SizeValidator::new(1_000_000),
    ];

    match fetch_and_validate(&resolver, "README.md", &validators).await {
        Ok(content) => {
            println!("   ✓ Content validated successfully");
            println!("   Size: {} bytes\n", content.content.len());
        }
        Err(e) => {
            println!("   ✗ Validation failed: {}\n", e);
        }
    }

    // 4. Production system
    println!("4. Production Deployment");
    println!("------------------------");

    let prod_sources: Vec<Arc<dyn ContentSource>> = vec![
        Arc::new(content_resolver::GitHubSource::new(
            "anthropics".to_string(),
            "anthropic-sdk-python".to_string(),
            "main".to_string(),
            "".to_string(),
        )),
    ];

    match ProductionContentSystem::new(prod_sources, None).await {
        Ok(system) => {
            println!("   ✓ Production system initialized");

            // Safe fetch with all safeguards
            match system.fetch_safe("README.md", 10_000_000).await {
                Ok(_) => println!("   ✓ Safe fetch succeeded"),
                Err(e) => println!("   ✗ Safe fetch failed: {}", e),
            }

            // Display metrics
            println!("\n   Metrics:");
            for (source, metrics) in system.get_metrics() {
                println!("     {}", source);
                println!("       Fetches: {}", metrics.fetch_count);
                println!("       Errors: {}", metrics.error_count);
            }
        }
        Err(e) => {
            println!("   ✗ Failed to initialize: {}", e);
        }
    }

    println!("\n=== Example Complete ===");
    Ok(())
}
