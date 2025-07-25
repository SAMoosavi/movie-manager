use either::Either;
use futures::future::join_all;
use rayon::prelude::*;
use std::path::PathBuf;
use tokio::{fs, task};
use walkdir::WalkDir;

use crate::{
    metadata_extractor::VideoFileData,
    sqlite::{
        get_all_video_files_from_db, get_video_file_by_path_from_db,
        remove_orphaned_video_metadata_from_db, remove_rows_by_paths,
    },
};

/// Supported video file extensions.
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi"];

/// Holds separated lists of video and subtitle file paths.
#[derive(Debug)]
pub struct FoundFiles {
    pub videos: Vec<PathBuf>,
    pub subtitles: Vec<PathBuf>,
}

/// Recursively scan a directory to find video and subtitle files.
pub async fn find_movies(root: PathBuf) -> FoundFiles {
    // Scan the file system in a blocking thread
    let all_files = task::spawn_blocking(move || {
        WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .collect::<Vec<_>>()
    })
    .await
    .expect("Failed to scan file system");

    // Classify files as videos or subtitles
    let (videos, mut subtitles): (Vec<_>, Vec<_>) = all_files
        .into_par_iter()
        .filter(|path| path.extension().is_some())
        .partition_map(|path| match path.extension().and_then(|ext| ext.to_str()) {
            Some(ext) => {
                let ext = ext.to_ascii_lowercase();
                if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
                    match get_video_file_by_path_from_db(path.clone()) {
                        Ok(None) => Either::Left(path),
                        _ => Either::Right(PathBuf::new()),
                    }
                } else if ext == "srt" {
                    Either::Right(path)
                } else {
                    Either::Right(PathBuf::new())
                }
            }
            None => Either::Right(PathBuf::new()),
        });

    // Filter out placeholder empty paths from ignored extensions
    subtitles.retain(|p| !p.as_os_str().is_empty());

    FoundFiles { videos, subtitles }
}

async fn find_non_existent_paths() -> Vec<VideoFileData> {
    let files = get_all_video_files_from_db()
        .unwrap()
        .into_iter()
        .map(|video| async move {
            let exists = fs::try_exists(&video.path).await.unwrap_or(false);
            if !exists { Some(video) } else { None }
        });

    join_all(files).await.into_iter().flatten().collect()
}

#[tauri::command]
pub async fn sync_files() {
    let paths: Vec<PathBuf> = find_non_existent_paths()
        .await
        .iter()
        .map(|video| video.path.clone())
        .collect();
    remove_rows_by_paths(&paths).unwrap();

    remove_orphaned_video_metadata_from_db().unwrap();
}

#[cfg(test)]
mod find_movies_tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_find_temp_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Create test files
        let video1 = root.join("movie1.mkv");
        let video2 = root.join("movie2.mp4");
        let subtitle1 = root.join("movie1.srt");
        let ignore_file = root.join("notes.txt");

        File::create(&video1).unwrap();
        File::create(&video2).unwrap();
        File::create(&subtitle1).unwrap();
        File::create(&ignore_file).unwrap();

        let result = find_movies(root.to_path_buf()).await;

        assert_eq!(result.videos.len(), 2, "Should detect two video files");
        assert_eq!(result.subtitles.len(), 1, "Should detect one subtitle file");

        assert!(result.videos.contains(&video1));
        assert!(result.videos.contains(&video2));
        assert!(result.subtitles.contains(&subtitle1));
        assert!(!result.videos.contains(&ignore_file));
    }
}
