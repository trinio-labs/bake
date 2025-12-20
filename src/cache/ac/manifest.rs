use crate::cache::cas::BlobHash;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::SystemTime;

/// Result of a recipe execution (action result)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    /// Recipe fully qualified name (e.g., "cookbook:recipe")
    pub recipe: String,

    /// Exit code of the recipe execution
    pub exit_code: i32,

    /// Output files produced by the recipe
    pub outputs: Vec<OutputFile>,

    /// Digest of stdout content
    pub stdout_digest: BlobHash,

    /// Digest of stderr content
    pub stderr_digest: BlobHash,

    /// Metadata about the execution
    pub execution_metadata: ExecutionMetadata,
}

impl ActionResult {
    /// Create a new action result
    pub fn new(recipe: String) -> Self {
        Self {
            recipe,
            exit_code: 0,
            outputs: Vec::new(),
            stdout_digest: BlobHash::from_content(b""),
            stderr_digest: BlobHash::from_content(b""),
            execution_metadata: ExecutionMetadata::new(),
        }
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    /// Get total size of all outputs
    pub fn total_output_size(&self) -> u64 {
        self.outputs.iter().map(|o| o.size).sum()
    }

    /// Check if execution was successful
    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }
}

/// An output file produced by a recipe
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputFile {
    /// Path relative to project root
    pub path: PathBuf,

    /// Content hash of the file
    pub digest: BlobHash,

    /// File size in bytes
    pub size: u64,

    /// Whether the file is executable
    #[serde(default)]
    pub is_executable: bool,

    /// Special node properties (for symlinks, directories, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_properties: Option<NodeProperties>,
}

impl OutputFile {
    /// Create a new output file entry
    pub fn new(path: PathBuf, digest: BlobHash, size: u64) -> Self {
        Self {
            path,
            digest,
            size,
            is_executable: false,
            node_properties: None,
        }
    }

    /// Set as executable
    pub fn with_executable(mut self, executable: bool) -> Self {
        self.is_executable = executable;
        self
    }

    /// Set node properties
    pub fn with_properties(mut self, properties: NodeProperties) -> Self {
        self.node_properties = Some(properties);
        self
    }
}

/// Special properties for nodes (symlinks, directories, etc.)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum NodeProperties {
    /// Symbolic link
    Symlink {
        /// Target of the symlink
        target: PathBuf,
    },
    /// Directory
    Directory,
    /// Special file (device, pipe, etc.)
    Special {
        /// Description of the special file
        description: String,
    },
}

/// Metadata about recipe execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetadata {
    /// When the execution started
    pub started_at: SystemTime,

    /// When the execution completed
    pub completed_at: SystemTime,

    /// Hostname where execution occurred
    pub hostname: String,

    /// Bake version used for execution
    pub bake_version: String,

    /// Additional metadata
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

impl ExecutionMetadata {
    /// Create new execution metadata with current time
    pub fn new() -> Self {
        let now = SystemTime::now();
        Self {
            started_at: now,
            completed_at: now,
            hostname: hostname::get()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| "unknown".to_string()),
            bake_version: env!("CARGO_PKG_VERSION").to_string(),
            extra: None,
        }
    }

    /// Set completion time to now
    pub fn complete(mut self) -> Self {
        self.completed_at = SystemTime::now();
        self
    }

    /// Get execution duration
    ///
    /// Returns the duration between started_at and completed_at.
    /// If completed_at is before started_at (e.g., due to system clock adjustments),
    /// returns Duration::default() (zero) instead of panicking.
    pub fn duration(&self) -> std::time::Duration {
        self.completed_at
            .duration_since(self.started_at)
            .unwrap_or_default()
    }

    /// Set extra metadata
    pub fn with_extra(mut self, extra: serde_json::Value) -> Self {
        self.extra = Some(extra);
        self
    }
}

impl Default for ExecutionMetadata {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_result_new() {
        let result = ActionResult::new("test:recipe".to_string());
        assert_eq!(result.recipe, "test:recipe");
        assert_eq!(result.exit_code, 0);
        assert!(result.outputs.is_empty());
        assert!(result.is_success());
    }

    #[test]
    fn test_action_result_json_roundtrip() {
        let mut result = ActionResult::new("test:recipe".to_string());
        result.exit_code = 0;
        result.outputs.push(OutputFile::new(
            PathBuf::from("output.txt"),
            BlobHash::from_content(b"test"),
            100,
        ));

        let json = result.to_json().unwrap();
        let parsed = ActionResult::from_json(&json).unwrap();

        assert_eq!(result.recipe, parsed.recipe);
        assert_eq!(result.exit_code, parsed.exit_code);
        assert_eq!(result.outputs.len(), parsed.outputs.len());
    }

    #[test]
    fn test_output_file() {
        let output = OutputFile::new(
            PathBuf::from("test.txt"),
            BlobHash::from_content(b"content"),
            42,
        );

        assert_eq!(output.path, PathBuf::from("test.txt"));
        assert_eq!(output.size, 42);
        assert!(!output.is_executable);
        assert!(output.node_properties.is_none());
    }

    #[test]
    fn test_output_file_with_executable() {
        let output = OutputFile::new(
            PathBuf::from("script.sh"),
            BlobHash::from_content(b"#!/bin/bash"),
            100,
        )
        .with_executable(true);

        assert!(output.is_executable);
    }

    #[test]
    fn test_output_file_with_symlink() {
        let output = OutputFile::new(PathBuf::from("link"), BlobHash::from_content(b""), 0)
            .with_properties(NodeProperties::Symlink {
                target: PathBuf::from("target"),
            });

        assert!(output.node_properties.is_some());
        match output.node_properties.unwrap() {
            NodeProperties::Symlink { target } => {
                assert_eq!(target, PathBuf::from("target"));
            }
            _ => panic!("Expected symlink"),
        }
    }

    #[test]
    fn test_execution_metadata() {
        let metadata = ExecutionMetadata::new();

        assert!(!metadata.hostname.is_empty());
        assert_eq!(metadata.bake_version, env!("CARGO_PKG_VERSION"));
        assert!(metadata.duration().as_secs() < 1);
    }

    #[test]
    fn test_execution_metadata_complete() {
        let metadata = ExecutionMetadata::new();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let metadata = metadata.complete();

        assert!(metadata.duration().as_millis() >= 10);
    }

    #[test]
    fn test_execution_metadata_with_extra() {
        let extra = serde_json::json!({"custom": "value"});
        let metadata = ExecutionMetadata::new().with_extra(extra.clone());

        assert_eq!(metadata.extra, Some(extra));
    }

    #[test]
    fn test_total_output_size() {
        let mut result = ActionResult::new("test:recipe".to_string());
        result.outputs.push(OutputFile::new(
            PathBuf::from("file1.txt"),
            BlobHash::from_content(b"a"),
            100,
        ));
        result.outputs.push(OutputFile::new(
            PathBuf::from("file2.txt"),
            BlobHash::from_content(b"b"),
            200,
        ));

        assert_eq!(result.total_output_size(), 300);
    }

    #[test]
    fn test_is_success() {
        let mut result = ActionResult::new("test:recipe".to_string());
        assert!(result.is_success());

        result.exit_code = 1;
        assert!(!result.is_success());

        result.exit_code = 0;
        assert!(result.is_success());
    }

    #[test]
    fn test_node_properties_directory() {
        let output = OutputFile::new(PathBuf::from("dir"), BlobHash::from_content(b""), 0)
            .with_properties(NodeProperties::Directory);

        match output.node_properties.unwrap() {
            NodeProperties::Directory => {}
            _ => panic!("Expected directory"),
        }
    }

    #[test]
    fn test_node_properties_special() {
        let output = OutputFile::new(PathBuf::from("device"), BlobHash::from_content(b""), 0)
            .with_properties(NodeProperties::Special {
                description: "character device".to_string(),
            });

        match output.node_properties.unwrap() {
            NodeProperties::Special { description } => {
                assert_eq!(description, "character device");
            }
            _ => panic!("Expected special file"),
        }
    }
}
