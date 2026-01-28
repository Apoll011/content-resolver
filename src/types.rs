use serde::{Deserialize, Serialize};

/// Represents a file's content and metadata
#[derive(Debug, Clone)]
pub struct FileContent {
    /// The raw bytes of the file
    pub content: bytes::Bytes,
    /// The path where this file was found
    pub source_path: String,
    /// Optional ETag or version identifier for caching
    pub etag: Option<String>,
}

/// Represents an entry in a directory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryEntry {
    /// Name of the file or folder
    pub name: String,
    /// Path relative to the source root
    pub path: String,
    /// Type of entry
    pub entry_type: EntryType,
}

/// Type of directory entry
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    File,
    Dir,
}

/// Result of listing a directory
#[derive(Debug, Clone)]
pub struct DirectoryListing {
    /// The path that was listed
    pub path: String,
    /// Entries found in the directory
    pub entries: Vec<DirectoryEntry>,
}
