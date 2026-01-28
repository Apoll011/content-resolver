/// Integration tests for the content resolution system
///
/// These tests demonstrate proper usage and verify behavior

use content_resolver::{
    Cache, ContentError, ContentSource, DirectoryEntry, DirectoryListing, DiskCache, EntryType,
    FileContent, GitHubSource, LanguageProvider, MemoryCache, ResourceResolver, SkillProvider,
};
use std::sync::Arc;
use tempfile::TempDir;

// Mock source for testing without network access
struct MockContentSource {
    files: std::collections::HashMap<String, Vec<u8>>,
    dirs: std::collections::HashMap<String, Vec<DirectoryEntry>>,
}

impl MockContentSource {
    fn new() -> Self {
        Self {
            files: std::collections::HashMap::new(),
            dirs: std::collections::HashMap::new(),
        }
    }

    fn add_file(&mut self, path: &str, content: &[u8]) {
        self.files.insert(path.to_string(), content.to_vec());
    }

    fn add_directory(&mut self, path: &str, entries: Vec<DirectoryEntry>) {
        self.dirs.insert(path.to_string(), entries);
    }
}

#[async_trait::async_trait]
impl ContentSource for MockContentSource {
    async fn fetch_file(&self, path: &str) -> content_resolver::Result<FileContent> {
        self.files
            .get(path)
            .map(|content| FileContent {
                content: bytes::Bytes::from(content.clone()),
                source_path: path.to_string(),
                etag: None,
            })
            .ok_or_else(|| ContentError::NotFound {
                path: path.to_string(),
            })
    }

    async fn list_directory(&self, path: &str) -> content_resolver::Result<DirectoryListing> {
        self.dirs
            .get(path)
            .map(|entries| DirectoryListing {
                path: path.to_string(),
                entries: entries.clone(),
            })
            .ok_or_else(|| ContentError::NotFound {
                path: path.to_string(),
            })
    }

    fn identifier(&self) -> String {
        "mock".to_string()
    }
}

#[tokio::test]
async fn test_basic_file_resolution() {
    let mut source = MockContentSource::new();
    source.add_file("test.txt", b"Hello, World!");

    let resolver = ResourceResolver::new(vec![Arc::new(source) as Arc<dyn ContentSource>]);

    let content = resolver.fetch_file("test.txt").await.unwrap();
    assert_eq!(content.content, bytes::Bytes::from("Hello, World!"));
}

#[tokio::test]
async fn test_fallback_resolution() {
    let mut primary = MockContentSource::new();
    primary.add_file("primary.txt", b"From primary");

    let mut fallback = MockContentSource::new();
    fallback.add_file("fallback.txt", b"From fallback");
    fallback.add_file("primary.txt", b"Should not be used");

    let resolver = ResourceResolver::new(vec![
        Arc::new(primary) as Arc<dyn ContentSource>,
        Arc::new(fallback) as Arc<dyn ContentSource>,
    ]);

    // File in primary source
    let content = resolver.fetch_file("primary.txt").await.unwrap();
    assert_eq!(content.content, bytes::Bytes::from("From primary"));

    // File only in fallback
    let content = resolver.fetch_file("fallback.txt").await.unwrap();
    assert_eq!(content.content, bytes::Bytes::from("From fallback"));

    // File in neither
    assert!(matches!(
        resolver.fetch_file("missing.txt").await,
        Err(ContentError::NotFound { .. })
    ));
}

#[tokio::test]
async fn test_memory_cache() {
    let mut source = MockContentSource::new();
    source.add_file("cached.txt", b"Cached content");

    let cache = Arc::new(MemoryCache::new());
    let resolver = ResourceResolver::with_cache(
        vec![Arc::new(source) as Arc<dyn ContentSource>],
        cache.clone(),
    );

    // First fetch - from source
    let content1 = resolver.fetch_file("cached.txt").await.unwrap();
    assert_eq!(content1.content, bytes::Bytes::from("Cached content"));

    // Verify cache was populated
    assert!(cache.contains("file:cached.txt").await);

    // Second fetch - from cache
    let content2 = resolver.fetch_file("cached.txt").await.unwrap();
    assert_eq!(content2.content, bytes::Bytes::from("Cached content"));
    assert_eq!(content2.source_path, "cache:cached.txt");
}

#[tokio::test]
async fn test_disk_cache() {
    let temp_dir = TempDir::new().unwrap();
    let cache_path = temp_dir.path().to_path_buf();

    let mut source = MockContentSource::new();
    source.add_file("file.txt", b"Test content");

    let cache = Arc::new(DiskCache::new(cache_path.clone()).await.unwrap());
    let resolver = ResourceResolver::with_cache(
        vec![Arc::new(source) as Arc<dyn ContentSource>],
        cache.clone(),
    );

    // Fetch and cache
    let content = resolver.fetch_file("file.txt").await.unwrap();
    assert_eq!(content.content, bytes::Bytes::from("Test content"));

    // Verify cache file exists on disk
    assert!(cache.contains("file:file.txt").await);

    // Create new resolver with same cache (simulates restart)
    let source2 = MockContentSource::new(); // Empty source
    let cache2 = Arc::new(DiskCache::new(cache_path).await.unwrap());
    let resolver2 = ResourceResolver::with_cache(
        vec![Arc::new(source2) as Arc<dyn ContentSource>],
        cache2,
    );

    // Should still be able to fetch from cache
    let cached_content = resolver2.fetch_file("file.txt").await.unwrap();
    assert_eq!(cached_content.content, bytes::Bytes::from("Test content"));
}

#[tokio::test]
async fn test_language_provider() {
    let mut source = MockContentSource::new();
    source.add_file("locales/en.lang", b"Hello");
    source.add_file("locales/pt.lang", b"Olá");
    source.add_file("locales/pt-BR.lang", b"Olá (Brasil)");

    let resolver = Arc::new(ResourceResolver::new(vec![
        Arc::new(source) as Arc<dyn ContentSource>
    ]));
    let provider = LanguageProvider::new(resolver, "locales".to_string());

    // Basic fetch
    assert_eq!(provider.fetch_language("en").await.unwrap(), "Hello");
    assert_eq!(provider.fetch_language("pt").await.unwrap(), "Olá");

    // Fallback
    assert_eq!(
        provider.fetch_with_fallback("pt-BR", "pt").await.unwrap(),
        "Olá (Brasil)"
    );

    // Fallback chain
    assert_eq!(
        provider
            .fetch_with_fallback("pt-PT", "pt")
            .await
            .unwrap(),
        "Olá"
    );

    // Multiple fallbacks
    assert_eq!(
        provider
            .fetch_with_fallbacks(&["fr", "es", "en"])
            .await
            .unwrap(),
        "Hello"
    );
}

#[tokio::test]
async fn test_skill_provider_list() {
    let mut source = MockContentSource::new();
    source.add_directory(
        "skills",
        vec![
            DirectoryEntry {
                name: "skill1".to_string(),
                path: "skills/skill1".to_string(),
                entry_type: EntryType::Dir,
            },
            DirectoryEntry {
                name: "skill2".to_string(),
                path: "skills/skill2".to_string(),
                entry_type: EntryType::Dir,
            },
            DirectoryEntry {
                name: "README.md".to_string(),
                path: "skills/README.md".to_string(),
                entry_type: EntryType::File,
            },
        ],
    );

    let resolver = Arc::new(ResourceResolver::new(vec![
        Arc::new(source) as Arc<dyn ContentSource>
    ]));
    let provider = SkillProvider::new(resolver, "skills".to_string());

    let skills = provider.list_skills().await.unwrap();
    assert_eq!(skills.len(), 2);
    assert_eq!(skills[0].id, "skill1");
    assert_eq!(skills[1].id, "skill2");
}

#[tokio::test]
async fn test_skill_provider_download() {
    let mut source = MockContentSource::new();

    // Set up skill directory structure
    source.add_directory(
        "skills/test_skill",
        vec![
            DirectoryEntry {
                name: "main.py".to_string(),
                path: "skills/test_skill/main.py".to_string(),
                entry_type: EntryType::File,
            },
            DirectoryEntry {
                name: "config".to_string(),
                path: "skills/test_skill/config".to_string(),
                entry_type: EntryType::Dir,
            },
        ],
    );

    source.add_directory(
        "skills/test_skill/config",
        vec![DirectoryEntry {
            name: "settings.json".to_string(),
            path: "skills/test_skill/config/settings.json".to_string(),
            entry_type: EntryType::File,
        }],
    );

    // Add file contents
    source.add_file("skills/test_skill/main.py", b"print('Hello')");
    source.add_file(
        "skills/test_skill/config/settings.json",
        b"{\"key\": \"value\"}",
    );

    let resolver = Arc::new(ResourceResolver::new(vec![
        Arc::new(source) as Arc<dyn ContentSource>
    ]));
    let provider = SkillProvider::new(resolver, "skills".to_string());

    let temp_dir = TempDir::new().unwrap();
    let result = provider
        .download_skill("test_skill", temp_dir.path())
        .await
        .unwrap();

    assert_eq!(result.files_written.len(), 2);
    assert!(result.total_bytes > 0);

    // Verify files were written correctly
    let main_content = tokio::fs::read_to_string(temp_dir.path().join("main.py"))
        .await
        .unwrap();
    assert_eq!(main_content, "print('Hello')");

    let config_content =
        tokio::fs::read_to_string(temp_dir.path().join("config/settings.json"))
            .await
            .unwrap();
    assert_eq!(config_content, "{\"key\": \"value\"}");
}

#[tokio::test]
async fn test_merged_directory_listing() {
    let mut source1 = MockContentSource::new();
    source1.add_directory(
        "dir",
        vec![DirectoryEntry {
            name: "file1.txt".to_string(),
            path: "dir/file1.txt".to_string(),
            entry_type: EntryType::File,
        }],
    );

    let mut source2 = MockContentSource::new();
    source2.add_directory(
        "dir",
        vec![
            DirectoryEntry {
                name: "file2.txt".to_string(),
                path: "dir/file2.txt".to_string(),
                entry_type: EntryType::File,
            },
            DirectoryEntry {
                name: "file1.txt".to_string(),
                path: "dir/file1.txt".to_string(),
                entry_type: EntryType::File,
            },
        ],
    );

    let resolver = ResourceResolver::new(vec![
        Arc::new(source1) as Arc<dyn ContentSource>,
        Arc::new(source2) as Arc<dyn ContentSource>,
    ]);

    let listing = resolver.list_directory_merged("dir").await.unwrap();

    // Should have both files, with file1.txt deduplicated
    assert_eq!(listing.entries.len(), 2);
    let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"file1.txt"));
    assert!(names.contains(&"file2.txt"));
}

#[tokio::test]
async fn test_error_propagation() {
    let source = MockContentSource::new(); // Empty source
    let resolver = ResourceResolver::new(vec![Arc::new(source) as Arc<dyn ContentSource>]);

    // Not found error
    match resolver.fetch_file("missing.txt").await {
        Err(ContentError::NotFound { path }) => {
            assert_eq!(path, "missing.txt");
        }
        _ => panic!("Expected NotFound error"),
    }
}

#[tokio::test]
async fn test_concurrent_operations() {
    let mut source = MockContentSource::new();
    source.add_file("file1.txt", b"Content 1");
    source.add_file("file2.txt", b"Content 2");
    source.add_file("file3.txt", b"Content 3");

    let resolver = Arc::new(ResourceResolver::new(vec![
        Arc::new(source) as Arc<dyn ContentSource>
    ]));

    // Fetch multiple files concurrently
    let resolver1 = resolver.clone();
    let resolver2 = resolver.clone();
    let resolver3 = resolver.clone();

    let (result1, result2, result3) = tokio::join!(
        async move { resolver1.fetch_file("file1.txt").await },
        async move { resolver2.fetch_file("file2.txt").await },
        async move { resolver3.fetch_file("file3.txt").await },
    );

    assert_eq!(result1.unwrap().content, bytes::Bytes::from("Content 1"));
    assert_eq!(result2.unwrap().content, bytes::Bytes::from("Content 2"));
    assert_eq!(result3.unwrap().content, bytes::Bytes::from("Content 3"));
}

#[test]
fn test_github_source_path_joining() {
    let source = GitHubSource::new(
        "owner".to_string(),
        "repo".to_string(),
        "main".to_string(),
        "base/path".to_string(),
    );

    // The identifier should include all configuration
    let id = source.identifier();
    assert!(id.contains("owner"));
    assert!(id.contains("repo"));
    assert!(id.contains("main"));
    assert!(id.contains("base/path"));
}
