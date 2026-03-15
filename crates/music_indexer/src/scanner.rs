use crate::error::{IndexerError, Result};
use crate::metadata::is_audio_file;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Scans a directory recursively and returns all audio file paths
pub fn scan_directory(dir_path: &Path) -> Result<Vec<PathBuf>> {
    if !dir_path.exists() {
        return Err(IndexerError::InvalidPath(format!(
            "Directory does not exist: {}",
            dir_path.to_string_lossy()
        )));
    }

    if !dir_path.is_dir() {
        return Err(IndexerError::InvalidPath(format!(
            "Path is not a directory: {}",
            dir_path.to_string_lossy()
        )));
    }

    let mut audio_files = Vec::new();

    for entry in WalkDir::new(dir_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip hidden files and directories
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with('.'))
            .unwrap_or(false)
        {
            continue;
        }

        // Check if it's a regular file
        if !path.is_file() {
            continue;
        }

        // Check if it's a supported audio format
        if is_audio_file(path) {
            // Convert to absolute path
            if let Ok(abs_path) = path.canonicalize() {
                audio_files.push(abs_path);
            }
        }
    }

    Ok(audio_files)
}

/// Scans multiple directories and returns all audio file paths
pub fn scan_directories(dir_paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut all_files = Vec::new();

    for dir_path in dir_paths {
        match scan_directory(dir_path) {
            Ok(files) => all_files.extend(files),
            Err(e) => {
                // Log error but continue with other directories
                eprintln!("Warning: Failed to scan {:?}: {}", dir_path, e);
            }
        }
    }

    // Remove duplicates
    all_files.sort();
    all_files.dedup();

    Ok(all_files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_nonexistent_directory() {
        let result = scan_directory(Path::new("/nonexistent/path/that/does/not/exist"));
        assert!(result.is_err());
    }

    #[test]
    fn test_scan_file_instead_of_directory() {
        // Use /tmp as a test path that should exist
        let result = scan_directory(Path::new("/etc/hosts"));
        assert!(result.is_err());
    }
}
