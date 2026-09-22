use std::path::Path;
use std::process::Command;
use tracing::{info, warn};

/// サムネイルを抽出する
/// ffmpeg を使って TS/M2TS ファイルの指定時刻から静止画を抽出
pub fn generate_thumbnail(
    ffmpeg_path: &Path,
    input_file: &Path,
    output_path: &Path,
    offset_seconds: u64,
) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new(ffmpeg_path)
        .args([
            "-ss", &offset_seconds.to_string(),
            "-i", input_file.to_str().unwrap(),
            "-vframes", "1",
            "-f", "image2",
            "-y",
            output_path.to_str().unwrap(),
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(
            "ffmpeg failed for {}: {}",
            input_file.display(),
            stderr
        );
        return Err(format!("ffmpeg failed: {}", stderr).into());
    }

    info!(
        "Thumbnail generated: {} -> {}",
        input_file.display(),
        output_path.display()
    );

    // URLパスを生成
    let url = format!(
        "/thumbnails/{}",
        output_path.file_name().unwrap().to_string_lossy()
    );
    Ok(url)
}

/// サムネイルURLを生成
pub fn thumbnail_url(filename: &str) -> String {
    let basename = filename.trim_end_matches(".m2ts").trim_end_matches(".ts");
    format!("/thumbnails/{}.jpg", basename)
}
