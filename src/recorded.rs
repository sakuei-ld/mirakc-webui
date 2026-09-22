use crate::mirakc;
use crate::thumbnail;
use crate::tsdropcheck;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Debug, Clone, Serialize)]
pub struct RecordedFile {
    pub id: String,
    pub filename: String,
    pub path: String,
    pub filesize: u64,
    pub program: Option<mirakc::ProgramInfo>,
    pub recording_time: Option<String>,
    pub drop_count: u64,
    pub error_count: u64,
    pub total_packets: u64,
    pub drop_percentage: f64,
    pub thumbnail_url: Option<String>,
    pub checked: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordedFileView {
    pub id: String,
    pub filename: String,
    pub path: String,
    pub program_name: String,
    pub channel_name: String,
    pub recording_time: String,
    pub drop_count: u64,
    pub error_count: u64,
    pub total_packets: u64,
    pub filesize_mb: f64,
    pub drop_percentage: f64,
    pub thumbnail_url: Option<String>,
    pub checked: bool,
}

/// 録画済みファイルの一覧を取得
pub async fn get_recorded_files(
    base_dir: &Path,
    thumbnails_dir: &Path,
    mirakc_api: &str,
    ffmpeg_path: &Path,
    thumbnail_offset: u64,
) -> Result<Vec<RecordedFileView>, Box<dyn std::error::Error>> {
    // .ts と .m2ts ファイルを検索
    let mut files = Vec::new();
    find_ts_files(base_dir, &mut files).await?;

    let mut result = Vec::new();

    for file_path in files {
        let metadata = tokio::fs::metadata(&file_path).await?;
        let filename = file_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let id = filename.clone();

        let mut recorded = RecordedFile {
            id: id.clone(),
            filename: filename.clone(),
            path: file_path.to_string_lossy().to_string(),
            filesize: metadata.len(),
            program: None,
            recording_time: None,
            drop_count: 0,
            error_count: 0,
            total_packets: 0,
            drop_percentage: 0.0,
            thumbnail_url: None,
            checked: false,
        };

        // 番組情報を取得
        if let Some(program_info) =
            mirakc::fetch_program_info(mirakc_api, &filename).await
        {
            recorded.program = Some(program_info);
        }

        // ドロップチェックを実行
        match tsdropcheck::check_ts_file(&file_path, &tsdropcheck::CheckOptions::default()) {
            Ok(drop_result) => {
                recorded.drop_count = drop_result.total_drops;
                recorded.error_count = drop_result.total_errors;
                recorded.total_packets = drop_result.total_packets;
                recorded.recording_time = Some(drop_result.duration);
                recorded.drop_percentage = if drop_result.total_packets > 0 {
                    (drop_result.total_drops as f64
                        / drop_result.total_packets as f64)
                        * 100.0
                } else {
                    0.0
                };
                recorded.checked = true;
                info!(
                    "Checked {}: drops={}, errors={}, packets={}",
                    filename, drop_result.total_drops, drop_result.total_errors, drop_result.total_packets
                );
            }
            Err(e) => {
                tracing::warn!("Failed to check {}: {}", filename, e);
            }
        }

        // サムネイルを生成
        let basename = filename.trim_end_matches(".m2ts").trim_end_matches(".ts");
        let thumbnail_path = thumbnails_dir.join(format!("{}.jpg", basename));

        if !thumbnail_path.exists() {
            if let Ok(url) =
                thumbnail::generate_thumbnail(ffmpeg_path, &file_path, &thumbnail_path, thumbnail_offset)
            {
                recorded.thumbnail_url = Some(url);
            } else {
                tracing::warn!("Failed to generate thumbnail for {}", filename);
            }
        } else {
            recorded.thumbnail_url = Some(thumbnail::thumbnail_url(&filename));
        }

        result.push(RecordedFileView {
            id: recorded.id,
            filename: recorded.filename,
            path: recorded.path,
            program_name: recorded
                .program
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "不明".to_string()),
            channel_name: recorded
                .program
                .as_ref()
                .map(|p| p.service_name.clone())
                .unwrap_or_else(|| "不明".to_string()),
            recording_time: recorded
                .recording_time
                .unwrap_or_else(|| "00:00:00.00".to_string()),
            drop_count: recorded.drop_count,
            error_count: recorded.error_count,
            total_packets: recorded.total_packets,
            filesize_mb: recorded.filesize as f64 / 1024.0 / 1024.0,
            drop_percentage: recorded.drop_percentage,
            thumbnail_url: recorded.thumbnail_url,
            checked: recorded.checked,
        });
    }

    // 録画時間順にソート（新しい順）
    result.sort_by(|a, b| b.filename.cmp(&a.filename));

    Ok(result)
}

/// ディレクトリ内の TS/M2TS ファイルを再帰的に検索
async fn find_ts_files(
    dir: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut entries = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            Box::pin(find_ts_files(&path, files)).await?;
        } else if let Some(ext) = path.extension() {
            if ext == "ts" || ext == "m2ts" {
                files.push(path);
            }
        }
    }
    Ok(())
}

/// ファイルIDから録画ファイルを取得
pub fn get_recorded_file_by_id(
    files: &[RecordedFileView],
    id: &str,
) -> Option<RecordedFileView> {
    files.iter().find(|f| f.id == id).cloned()
}
