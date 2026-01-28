# Quick Start Guide

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
content-resolver = { path = "path/to/content-resolver" }
tokio = { version = "1.35", features = ["full"] }
```

## 5-Minute Tutorial

### 1. Fetch a File from GitHub

```rust
use content_resolver::{GitHubSource, ResourceResolver};
use std::sync::Arc;

#[tokio::main]
async fn main() -> content_resolver::Result<()> {
    // Create a GitHub source
    let source = Arc::new(GitHubSource::new(
        "torvalds".to_string(),      // owner
        "linux".to_string(),          // repository
        "master".to_string(),         // branch
        "".to_string(),               // base path (empty = root)
    ));

    // Create resolver
    let resolver = Arc::new(ResourceResolver::new(vec![source]));

    // Fetch README
    let content = resolver.fetch_file("README").await?;
    let text = String::from_utf8_lossy(&content.content);
    
    println!("Fetched {} bytes", content.content.len());
    println!("Preview: {}...", &text[..200]);

    Ok(())
}
```

### 2. Multiple Sources with Fallback

```rust
use content_resolver::{GitHubSource, ResourceResolver, ContentSource};
use std::sync::Arc;

#[tokio::main]
async fn main() -> content_resolver::Result<()> {
    // Primary source (tried first)
    let primary = Arc::new(GitHubSource::new(
        "myorg".to_string(),
        "content-dev".to_string(),
        "main".to_string(),
        "".to_string(),
    ));

    // Fallback source (tried if primary fails)
    let fallback = Arc::new(GitHubSource::new(
        "myorg".to_string(),
        "content-prod".to_string(),
        "main".to_string(),
        "".to_string(),
    ));

    let resolver = Arc::new(ResourceResolver::new(vec![
        primary as Arc<dyn ContentSource>,
        fallback as Arc<dyn ContentSource>,
    ]));

    // Will try primary first, then fallback
    let content = resolver.fetch_file("config.json").await?;
    println!("Found in: {}", content.source_path);

    Ok(())
}
```

### 3. Add Caching

```rust
use content_resolver::{GitHubSource, ResourceResolver, MemoryCache, ContentSource};
use std::sync::Arc;

#[tokio::main]
async fn main() -> content_resolver::Result<()> {
    let source = Arc::new(GitHubSource::new(
        "torvalds".to_string(),
        "linux".to_string(),
        "master".to_string(),
        "".to_string(),
    ));

    // Add memory cache
    let cache = Arc::new(MemoryCache::new());
    let resolver = Arc::new(ResourceResolver::with_cache(
        vec![source as Arc<dyn ContentSource>],
        cache,
    ));

    // First fetch - from network
    println!("First fetch (network):");
    let start = std::time::Instant::now();
    let _ = resolver.fetch_file("README").await?;
    println!("  Time: {:?}", start.elapsed());

    // Second fetch - from cache
    println!("\nSecond fetch (cache):");
    let start = std::time::Instant::now();
    let content = resolver.fetch_file("README").await?;
    println!("  Time: {:?}", start.elapsed());
    println!("  Source: {}", content.source_path);  // Will show "cache:README"

    Ok(())
}
```

### 4. Disk Cache for Persistence

```rust
use content_resolver::{GitHubSource, ResourceResolver, DiskCache, ContentSource};
use std::sync::Arc;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> content_resolver::Result<()> {
    let source = Arc::new(GitHubSource::new(
        "torvalds".to_string(),
        "linux".to_string(),
        "master".to_string(),
        "".to_string(),
    ));

    // Create disk cache
    let cache_dir = PathBuf::from("/tmp/content-cache");
    let cache = Arc::new(DiskCache::new(cache_dir).await?);
    
    let resolver = Arc::new(ResourceResolver::with_cache(
        vec![source as Arc<dyn ContentSource>],
        cache,
    ));

    // Fetch will be cached to disk
    let _ = resolver.fetch_file("README").await?;
    println!("Content cached to disk");

    // Even after restart, cache persists!

    Ok(())
}
```

## Common Patterns

### Error Handling

```rust
use content_resolver::ContentError;

match resolver.fetch_file("file.txt").await {
    Ok(content) => {
        // Success
        println!("Got {} bytes", content.content.len());
    }
    Err(ContentError::NotFound { path }) => {
        println!("File not found: {}", path);
    }
    Err(ContentError::RateLimited { message }) => {
        println!("Rate limited: {}", message);
        // Implement backoff/retry
    }
    Err(e) => {
        println!("Error: {}", e);
    }
}
```

### Concurrent Fetching

```rust
use tokio::try_join;

let (file1, file2, file3) = try_join!(
    resolver.fetch_file("file1.txt"),
    resolver.fetch_file("file2.txt"),
    resolver.fetch_file("file3.txt"),
)?;

println!("Fetched all files concurrently!");
```

### Custom Error Recovery

```rust
async fn fetch_with_retry(
    resolver: &ResourceResolver,
    path: &str,
    max_attempts: u32,
) -> content_resolver::Result<content_resolver::FileContent> {
    let mut attempts = 0;
    
    loop {
        attempts += 1;
        
        match resolver.fetch_file(path).await {
            Ok(content) => return Ok(content),
            Err(ContentError::Network(_)) if attempts < max_attempts => {
                println!("Attempt {} failed, retrying...", attempts);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            Err(e) => return Err(e),
        }
    }
}
```

## Next Steps

- Read [README.md](README.md) for full documentation
- Browse [examples/](examples/) for more complex usage
- Review [tests/](tests/) for test patterns

## Running Examples

```bash
# Full feature demonstration
cargo run --example full_example

# Real-world application pattern
cargo run --example practical_usage

# Advanced patterns and extensions
cargo run --example advanced_patterns
```

## Running Tests

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_basic_file_resolution
```

## Troubleshooting

### GitHub Rate Limits

If you hit rate limits (60 requests/hour):
- Add caching to reduce requests
- Use authenticated requests (future feature)
- Implement retry with exponential backoff

### File Not Found

Check:
- Repository owner and name are correct
- Branch name is correct
- File path is correct (case-sensitive)
- Base path is configured correctly

### Network Errors

- Check internet connectivity
- Verify GitHub is accessible
- Check for firewall/proxy issues
- Implement retry logic for transient failures

## Tips

1. **Always use caching in production** - reduces network calls dramatically
2. **Use disk cache for persistence** - survives restarts
3. **Implement retry logic** - handle transient failures gracefully
4. **Monitor rate limits** - track API usage
5. **Test with mock sources** - avoid hitting real APIs during tests
6. **Use multiple sources** - provides redundancy and fallback
