# Content Resolution System

A production-ready Rust library for fetching, resolving, and managing content from remote Git repositories with support for fallback sources, caching, and specialized content types (languages and skills).

## Features

- **Abstraction-First Design**: Clean trait-based architecture for extensibility
- **Multiple Source Support**: Fetch content from multiple repositories with ordered fallback
- **GitHub Integration**: Built-in support for GitHub repositories via raw content and REST API
- **Smart Caching**: Optional memory or disk-based caching to reduce network requests
- **Language Files**: High-level API for locale-based content with fallback chains
- **Skill Management**: Recursive download of multi-file skill bundles
- **Async-First**: Built on tokio and reqwest for efficient concurrent operations
- **Comprehensive Error Handling**: Explicit error types, no panics in normal operation
- **Production-Oriented**: Designed for long-lived systems with evolving content sources

## Architecture

```
┌─────────────────────────────────────────┐
│  High-Level Providers                   │
│  - LanguageProvider (locale resolution) │
│  - SkillProvider (bundle management)    │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│  ResourceResolver                        │
│  - Multi-source fallback                 │
│  - Optional caching layer               │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│  ContentSource Trait                     │
│  - fetch_file()                         │
│  - list_directory()                     │
└─────────────────┬───────────────────────┘
                  │
        ┌─────────┴─────────┐
        ▼                   ▼
┌───────────────┐   ┌───────────────┐
│ GitHubSource  │   │ Custom Source │
└───────────────┘   └───────────────┘
```

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
content-resolver = { path = "path/to/content-resolver" }
tokio = { version = "1.35", features = ["full"] }
```

### Basic Usage

```rust
use content_resolver::{GitHubSource, ResourceResolver};
use std::sync::Arc;

#[tokio::main]
async fn main() -> content_resolver::Result<()> {
    // Create a GitHub source
    let source = Arc::new(GitHubSource::new(
        "anthropics".to_string(),
        "anthropic-sdk-python".to_string(),
        "main".to_string(),
        "".to_string(),
    ));

    // Create resolver
    let resolver = Arc::new(ResourceResolver::new(vec![source]));

    // Fetch a file
    let content = resolver.fetch_file("README.md").await?;
    println!("Fetched {} bytes", content.content.len());

    Ok(())
}
```

## Core Concepts

### ContentSource Trait

The foundational abstraction for all content sources:

```rust
#[async_trait]
pub trait ContentSource: Send + Sync {
    async fn fetch_file(&self, path: &str) -> Result<FileContent>;
    async fn list_directory(&self, path: &str) -> Result<DirectoryListing>;
    fn identifier(&self) -> String;
    async fn file_exists(&self, path: &str) -> bool;
}
```

### ResourceResolver

Orchestrates multiple content sources with fallback logic:

```rust
// Create resolver with multiple sources (tried in order)
let resolver = ResourceResolver::new(vec![
    primary_source,
    fallback_source,
    last_resort_source,
]);

// Fetches from first source that succeeds
let content = resolver.fetch_file("path/to/file").await?;
```

### Caching

Reduce network requests with built-in caching:

```rust
use content_resolver::{MemoryCache, DiskCache};

// Memory cache
let cache = Arc::new(MemoryCache::new());
let resolver = ResourceResolver::with_cache(sources, cache);

// Disk cache
let cache = Arc::new(DiskCache::new("/tmp/cache".into()).await?);
let resolver = ResourceResolver::with_cache(sources, cache);
```

## Advanced Features

### Language Files

Fetch locale-specific content with automatic fallback:

```rust
use content_resolver::LanguageProvider;

let provider = LanguageProvider::new(resolver, "locales".to_string());

// Simple fetch
let content = provider.fetch_language("en").await?;

// With fallback
let content = provider.fetch_with_fallback("pt-BR", "pt").await?;

// Multiple fallbacks (tries pt-BR -> pt -> en)
let content = provider.fetch_with_fallbacks(&["pt-BR", "pt", "en"]).await?;

// List all available languages
let languages = provider.list_languages().await?;
```

### Skill Management

Download entire skill bundles recursively:

```rust
use content_resolver::SkillProvider;
use std::path::PathBuf;

let provider = SkillProvider::new(resolver, "skills".to_string());

// List available skills
let skills = provider.list_skills().await?;
for skill in skills {
    println!("Skill: {} at {}", skill.id, skill.path);
}

// Download a complete skill
let output_dir = PathBuf::from("/local/skills/my-skill");
let result = provider.download_skill("my-skill", &output_dir).await?;
println!("Downloaded {} files ({} bytes)", 
    result.files_written.len(), 
    result.total_bytes
);

// Get skill structure without downloading
let structure = provider.get_skill_structure("my-skill").await?;
```

### Multiple Repository Configuration

```rust
// Development repository (checked first)
let dev_source = Arc::new(GitHubSource::new(
    "myorg".to_string(),
    "content-dev".to_string(),
    "main".to_string(),
    "".to_string(),
));

// Production repository (fallback)
let prod_source = Arc::new(GitHubSource::new(
    "myorg".to_string(),
    "content-prod".to_string(),
    "main".to_string(),
    "".to_string(),
));

// Legacy repository (last resort)
let legacy_source = Arc::new(GitHubSource::new(
    "myorg".to_string(),
    "content-legacy".to_string(),
    "v1".to_string(),
    "content".to_string(),  // Files in /content subdirectory
));

let resolver = ResourceResolver::new(vec![
    dev_source as Arc<dyn ContentSource>,
    prod_source as Arc<dyn ContentSource>,
    legacy_source as Arc<dyn ContentSource>,
]);
```

## Error Handling

All operations return `Result<T, ContentError>`:

```rust
use content_resolver::ContentError;

match resolver.fetch_file("file.txt").await {
    Ok(content) => {
        // Process content
    }
    Err(ContentError::NotFound { path }) => {
        println!("File not found: {}", path);
    }
    Err(ContentError::RateLimited { message }) => {
        println!("Rate limited: {}", message);
        // Implement backoff
    }
    Err(ContentError::Network(e)) => {
        println!("Network error: {}", e);
        // Retry logic
    }
    Err(e) => {
        println!("Other error: {}", e);
    }
}
```

Error types:
- `NotFound`: Resource doesn't exist
- `Network`: Network/HTTP errors
- `RateLimited`: API rate limit exceeded
- `InvalidStructure`: Unexpected remote structure
- `Io`: Local I/O errors
- `Cache`: Cache operation failures
- `InvalidConfig`: Configuration errors

## Custom Content Sources

Implement the `ContentSource` trait for custom backends:

```rust
use async_trait::async_trait;
use content_resolver::{ContentSource, Result, FileContent, DirectoryListing};

struct S3Source {
    bucket: String,
    region: String,
}

#[async_trait]
impl ContentSource for S3Source {
    async fn fetch_file(&self, path: &str) -> Result<FileContent> {
        // Implement S3 fetch logic
        todo!()
    }

    async fn list_directory(&self, path: &str) -> Result<DirectoryListing> {
        // Implement S3 list logic
        todo!()
    }

    fn identifier(&self) -> String {
        format!("s3://{}", self.bucket)
    }
}
```

## Performance Considerations

### Caching Strategy

```rust
// For read-heavy workloads with stable content
let memory_cache = Arc::new(MemoryCache::new());

// For large content or shared cache across processes
let disk_cache = Arc::new(DiskCache::new("/var/cache/content".into()).await?);

// For testing or when freshness is critical
// (no cache - every request hits the network)
let resolver = ResourceResolver::new(sources);
```

### Concurrent Operations

All operations are async and can be executed concurrently:

```rust
use tokio::try_join;

let (file1, file2, file3) = try_join!(
    resolver.fetch_file("file1.txt"),
    resolver.fetch_file("file2.txt"),
    resolver.fetch_file("file3.txt"),
)?;
```

## Testing

Run tests with:

```bash
cargo test
```

Run examples:

```bash
cargo run --example full_example
```

## Design Principles

1. **No Git Binaries**: Pure HTTP-based access, no Git installation required
2. **Read-Only**: Sources are immutable, no write operations
3. **Async-First**: Non-blocking I/O throughout
4. **Explicit Errors**: No panics in normal operation
5. **Composable**: Mix and match sources, caches, and providers
6. **Production-Ready**: Proper error handling, logging hooks, and extensibility

## Limitations

- GitHub API rate limits apply (60 requests/hour unauthenticated, 5000/hour authenticated)
- No authentication support in current GitHub implementation (can be extended)
- Directory listings use GitHub API (not available for all Git hosts)
- No Git history or branch operations (read-only content access)

## Future Extensions

Potential additions for future versions:
- GitLab and Bitbucket sources
- Authentication support for private repositories
- Webhook-based cache invalidation
- Content validation and checksums
- Compression for cached content
- Metrics and observability hooks

## License

This is example code for educational purposes.
