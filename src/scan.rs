use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub root: PathBuf,
    pub files: Vec<FileEntry>,
    pub total_size: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("目录不存在或无法访问: {0}")]
    Io(#[from] std::io::Error),
    #[error("路径不是目录: {0}")]
    NotDir(String),
}

pub fn scan_one_level(root: &Path) -> Result<ScanResult, ScanError> {
    if !root.is_dir() {
        return Err(ScanError::NotDir(root.display().to_string()));
    }

    let mut files = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        files.push(FileEntry { name, size });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    let total_size = files.iter().map(|f| f.size).sum();
    Ok(ScanResult {
        root: root.to_path_buf(),
        files,
        total_size,
    })
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;
    let n = bytes as f64;
    if n >= TB {
        format!("{:.2} TB", n / TB)
    } else if n >= GB {
        format!("{:.2} GB", n / GB)
    } else if n >= MB {
        format!("{:.2} MB", n / MB)
    } else if n >= KB {
        format!("{:.2} KB", n / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scans_only_one_level_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.fastq.gz"), b"a").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub").join("nested.txt"), b"n").unwrap();

        let result = scan_one_level(dir.path()).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].name, "a.fastq.gz");
    }
}
