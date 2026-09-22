use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json, Response},
    routing::{delete, get, post},
    Router,
};
use mirakc_webui::recorded;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

#[derive(Clone, Serialize, Deserialize)]
pub struct TrashInfo {
    pub original_path: String,
    pub original_thumb_path: Option<String>,
    pub deleted_at: u64, // epoch seconds
}

#[derive(Clone)]
pub struct AppState {
    pub recorded_dir: PathBuf,
    pub thumbnails_dir: PathBuf,
    pub mirakc_api: String,
    pub ffmpeg_path: PathBuf,
    pub thumbnail_offset: u64,
    pub trash_dir: PathBuf,
    pub trash_thumb_dir: PathBuf,
    pub trash_ttl_days: u64,
    pub cleanup_interval_secs: u64,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
    pub sort: Option<String>,
}

#[derive(Serialize)]
pub struct TrashFileView {
    pub id: String,
    pub filename: String,
    pub program_name: String,
    pub channel_name: String,
    pub recording_time: String,
    pub drop_count: u64,
    pub error_count: u64,
    pub total_packets: u64,
    pub filesize_mb: f64,
    pub drop_percentage: f64,
    pub thumbnail_url: Option<String>,
    pub deleted_at: u64,
    pub days_remaining: u64,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let recorded_dir = std::env::args()
        .find(|arg| arg.starts_with("-d="))
        .map(|arg| PathBuf::from(arg.trim_start_matches("-d=")))
        .unwrap_or_else(|| PathBuf::from("/var/lib/mirakc/recorded"));

    let thumbnails_dir = std::env::args()
        .find(|arg| arg.starts_with("-t="))
        .map(|arg| PathBuf::from(arg.trim_start_matches("-t=")))
        .unwrap_or_else(|| PathBuf::from("/var/lib/mirakc/thumbnails"));

    let mirakc_api = std::env::args()
        .find(|arg| arg.starts_with("-m="))
        .map(|arg| arg.trim_start_matches("-m=").to_string())
        .unwrap_or_else(|| "http://localhost:40772".to_string());

    let ffmpeg_path = std::env::args()
        .find(|arg| arg.starts_with("-f="))
        .map(|arg| PathBuf::from(arg.trim_start_matches("-f=")))
        .unwrap_or_else(|| PathBuf::from("/usr/bin/ffmpeg"));

    let thumbnail_offset = std::env::args()
        .find(|arg| arg.starts_with("-s="))
        .and_then(|arg| arg.trim_start_matches("-s=").parse::<u64>().ok())
        .unwrap_or_else(|| 5);

    let trash_dir = std::env::args()
        .find(|arg| arg.starts_with("-trash="))
        .map(|arg| PathBuf::from(arg.trim_start_matches("-trash=")))
        .unwrap_or_else(|| recorded_dir.join(".trash"));

    let trash_thumb_dir = std::env::args()
        .find(|arg| arg.starts_with("-tt="))
        .map(|arg| PathBuf::from(arg.trim_start_matches("-tt=")))
        .unwrap_or_else(|| thumbnails_dir.join(".trash"));

    let trash_ttl_days = std::env::args()
        .find(|arg| arg.starts_with("-ttl="))
        .and_then(|arg| arg.trim_start_matches("-ttl=").parse::<u64>().ok())
        .unwrap_or_else(|| 7);

    let cleanup_interval_secs = std::env::args()
        .find(|arg| arg.starts_with("-cleanup="))
        .and_then(|arg| arg.trim_start_matches("-cleanup=").parse::<u64>().ok())
        .unwrap_or_else(|| 3600); // 1 hour

    // Ensure trash dirs exist
    tokio::fs::create_dir_all(&trash_dir).await.unwrap();
    tokio::fs::create_dir_all(&trash_thumb_dir).await.unwrap();

    let state = AppState {
        recorded_dir,
        thumbnails_dir,
        mirakc_api,
        ffmpeg_path,
        thumbnail_offset,
        trash_dir,
        trash_thumb_dir,
        trash_ttl_days,
        cleanup_interval_secs,
    };

    // Spawn background cleanup task
    let cleanup_state = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(cleanup_state.cleanup_interval_secs)).await;
            cleanup_expired(&cleanup_state).await;
        }
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/api/recorded", get(get_recorded_files))
        .route("/api/recorded/{id}", get(get_recorded_file))
        .route("/api/recorded/{id}", delete(delete_recorded_file))
        .route("/api/recorded/{id}/download", get(download_recorded_file))
        .route("/api/trash", get(get_trash_files))
        .route("/api/trash/{id}", get(get_trash_file))
        .route("/api/trash/{id}", delete(permanent_delete_trash))
        .route("/api/trash/{id}/restore", post(restore_trash_file))
        .route("/api/trash/cleanup", post(manual_cleanup))
        .route("/api/trash/thumbnails/{filename}", get(serve_trash_thumbnail))
        .route("/thumbnails/{filename}", get(serve_thumbnail))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    info!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn index() -> Html<String> {
    let html = include_str!("../public/index.html");
    Html(html.to_string())
}

async fn get_recorded_files(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<Vec<recorded::RecordedFileView>>, StatusCode> {
    let files = recorded::get_recorded_files(
        &state.recorded_dir,
        &state.thumbnails_dir,
        &state.mirakc_api,
        &state.ffmpeg_path,
        state.thumbnail_offset,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to get recorded files: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // 検索フィルター
    let files = if let Some(q) = params.q {
        files
            .into_iter()
            .filter(|f| {
                f.program_name.contains(&q) || f.channel_name.contains(&q)
            })
            .collect::<Vec<_>>()
    } else {
        files
    };

    // ソート
    let mut sorted_files = files;
    if let Some(sort) = params.sort {
        match sort.as_str() {
            "drops" => sorted_files.sort_by(|a, b| b.drop_count.cmp(&a.drop_count)),
            "size" => sorted_files.sort_by(|a, b| b.filesize_mb.partial_cmp(&a.filesize_mb).unwrap()),
            _ => {} // デフォルトはファイル名順
        }
    }

    Ok(Json(sorted_files))
}

async fn get_recorded_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<recorded::RecordedFileView>, StatusCode> {
    let files = recorded::get_recorded_files(
        &state.recorded_dir,
        &state.thumbnails_dir,
        &state.mirakc_api,
        &state.ffmpeg_path,
        state.thumbnail_offset,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    recorded::get_recorded_file_by_id(&files, &id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn download_recorded_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, StatusCode> {
    let files = recorded::get_recorded_files(
        &state.recorded_dir,
        &state.thumbnails_dir,
        &state.mirakc_api,
        &state.ffmpeg_path,
        state.thumbnail_offset,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let file = recorded::get_recorded_file_by_id(&files, &id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let file_path = PathBuf::from(&file.path);
    if !file_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    let content = tokio::fs::read(&file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Disposition",
        format!(
            "attachment; filename=\"{}\"",
            id.replace('"', "")
        )
        .parse()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );
    headers.insert(
        "Content-Type",
        "video/mp2t".parse().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );

    Ok((headers, content).into_response())
}

async fn serve_thumbnail(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<Response, StatusCode> {
    let thumbnail_path = state.thumbnails_dir.join(&filename);

    if !thumbnail_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    let content = tokio::fs::read(&thumbnail_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((
        [
            ("Content-Type", "image/jpeg"),
        ],
        content,
    ).into_response())
}

async fn delete_recorded_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let files = recorded::get_recorded_files(
        &state.recorded_dir,
        &state.thumbnails_dir,
        &state.mirakc_api,
        &state.ffmpeg_path,
        state.thumbnail_offset,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let file = recorded::get_recorded_file_by_id(&files, &id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let file_path = PathBuf::from(&file.path);
    if !file_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    // 削除時刻
    let deleted_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // TrashInfo 作成
    let thumb_basename = id.trim_end_matches(".m2ts").trim_end_matches(".ts");
    let original_thumb_path = if id.ends_with(".m2ts") || id.ends_with(".ts") {
        let p = state.thumbnails_dir.join(format!("{}.jpg", thumb_basename));
        if p.exists() { Some(p.to_string_lossy().to_string()) } else { None }
    } else { None };

    let trash_info = TrashInfo {
        original_path: file_path.to_string_lossy().to_string(),
        original_thumb_path: original_thumb_path.clone(),
        deleted_at,
    };

    // .trash-info ファイル作成
    let info_path = state.trash_dir.join(format!("{}.info", id));
    let info_json = serde_json::to_string_pretty(&trash_info).unwrap();
    tokio::fs::write(&info_path, info_json).await.map_err(|e| {
        tracing::error!("Failed to write trash info: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // ファイルをゴミ箱に移動
    let trash_file_path = state.trash_dir.join(&id);
    tokio::fs::rename(&file_path, &trash_file_path).await.map_err(|e| {
        tracing::error!("Failed to move file to trash: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // サムネイルもゴミ箱に移動
    if let Some(ref thumb_path_str) = original_thumb_path {
        let thumb_path = PathBuf::from(thumb_path_str);
        let trash_thumb_path = state.trash_thumb_dir.join(format!("{}.jpg", thumb_basename));
        if thumb_path.exists() {
            tokio::fs::rename(&thumb_path, &trash_thumb_path).await.ok();
        }
    }

    info!("Moved to trash: {} (expires in {} days)", id, state.trash_ttl_days);

    Ok(Json(serde_json::json!({
        "success": true,
        "id": id,
        "deleted_at": deleted_at,
        "expires_in_days": state.trash_ttl_days,
    })))
}

async fn get_trash_files(
    State(state): State<AppState>,
) -> Result<Json<Vec<TrashFileView>>, StatusCode> {
    let mut entries = match tokio::fs::read_dir(&state.trash_dir).await {
        Ok(e) => e,
        Err(_) => return Ok(Json(Vec::new())),
    };

    let mut result = Vec::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let ttl_secs = state.trash_ttl_days * 24 * 3600;

    while let Some(entry) = entries.next_entry().await.ok() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let id = path.file_name().unwrap().to_string_lossy().to_string();
        
        // .info ファイルを読み込む
        let info_path = state.trash_dir.join(format!("{}.info", id));
        let trash_info: TrashInfo = match tokio::fs::read_to_string(&info_path).await {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(info) => info,
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        let metadata = match tokio::fs::metadata(&path).await {
            Ok(m) => m,
            Err(_) => continue,
        };

        // ドロップチェックを直接実行
        let (dc, ec, tp, dp, rt) = match mirakc_webui::tsdropcheck::check_ts_file(&path, &mirakc_webui::tsdropcheck::CheckOptions::default()) {
            Ok(r) => (r.total_drops, r.total_errors, r.total_packets,
                      if r.total_packets > 0 { (r.total_drops as f64 / r.total_packets as f64) * 100.0 } else { 0.0 },
                      r.duration),
            Err(_) => (0, 0, 0, 0.0, "00:00:00.00".to_string()),
        };

        // 番組情報を取得
        let (program_name, channel_name) = {
            let filename = id.trim_end_matches(".m2ts").trim_end_matches(".ts");
            let parts: Vec<&str> = filename.rsplitn(2, '_').collect();
            if parts.len() >= 2 {
                match recorded::fetch_program_info(&state.mirakc_api, parts[1]).await {
                    Some(info) => (info.name, info.service_name),
                    None => ("不明".to_string(), "不明".to_string()),
                }
            } else {
                ("不明".to_string(), "不明".to_string())
            }
        };

        let thumb_basename = id.trim_end_matches(".m2ts").trim_end_matches(".ts");
        let thumb_path = state.trash_thumb_dir.join(format!("{}.jpg", thumb_basename));
        let thumbnail_url = if thumb_path.exists() {
            Some(format!("/api/trash/thumbnails/{}.jpg", thumb_basename))
        } else {
            None
        };

        let expired = now - trash_info.deleted_at > ttl_secs;

        result.push(TrashFileView {
            id: id.clone(),
            filename: id,
            program_name,
            channel_name,
            recording_time: rt,
            drop_count: dc,
            error_count: ec,
            total_packets: tp,
            filesize_mb: metadata.len() as f64 / 1024.0 / 1024.0,
            drop_percentage: dp,
            thumbnail_url,
            deleted_at: trash_info.deleted_at,
            days_remaining: if expired { 0 } else {
                ((ttl_secs - (now - trash_info.deleted_at)) / (24 * 3600)) as u64
            },
        });
    }

    // 削除時刻でソート（新しい順）
    result.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));

    Ok(Json(result))
}

async fn restore_trash_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let trash_file_path = state.trash_dir.join(&id);
    if !trash_file_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    // .trash-info 読み込み
    let info_path = state.trash_dir.join(format!("{}.info", id));
    let trash_info: TrashInfo = match tokio::fs::read_to_string(&info_path).await {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(info) => info,
            Err(_) => return Err(StatusCode::BAD_REQUEST),
        },
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    };

    // ファイルを元の場所に復元
    let original_path = PathBuf::from(&trash_info.original_path);
    if let Some(parent) = original_path.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::rename(&trash_file_path, &original_path).await.map_err(|e| {
        tracing::error!("Failed to restore file: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // .info ファイル削除
    tokio::fs::remove_file(&info_path).await.ok();

    // サムネイルも復元
    if let Some(ref thumb_path_str) = trash_info.original_thumb_path {
        let thumb_path = PathBuf::from(thumb_path_str);
        let trash_thumb_path = state.trash_thumb_dir.join(format!("{}.jpg", id.trim_end_matches(".m2ts").trim_end_matches(".ts")));
        if trash_thumb_path.exists() {
            if let Some(parent) = thumb_path.parent() {
                tokio::fs::create_dir_all(parent).await.ok();
            }
            tokio::fs::rename(&trash_thumb_path, &thumb_path).await.ok();
        }
    }

    info!("Restored: {} -> {}", id, original_path.display());

    Ok(Json(serde_json::json!({
        "success": true,
        "id": id,
        "restored_to": trash_info.original_path,
    })))
}

async fn permanent_delete_trash(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let trash_file_path = state.trash_dir.join(&id);
    if !trash_file_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    tokio::fs::remove_file(&trash_file_path).await.map_err(|e| {
        tracing::error!("Failed to permanently delete: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // .info ファイル削除
    let info_path = state.trash_dir.join(format!("{}.info", id));
    tokio::fs::remove_file(&info_path).await.ok();

    // サムネイル削除
    let thumb_basename = id.trim_end_matches(".m2ts").trim_end_matches(".ts");
    let trash_thumb_path = state.trash_thumb_dir.join(format!("{}.jpg", thumb_basename));
    tokio::fs::remove_file(&trash_thumb_path).await.ok();

    info!("Permanently deleted: {}", id);

    Ok(Json(serde_json::json!({
        "success": true,
        "id": id,
    })))
}

async fn manual_cleanup(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let cleaned = cleanup_expired(&state).await;
    Ok(Json(serde_json::json!({
        "success": true,
        "cleaned_count": cleaned,
    })))
}

async fn get_trash_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TrashFileView>, StatusCode> {
    let trash_file_path = state.trash_dir.join(&id);
    if !trash_file_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    let info_path = state.trash_dir.join(format!("{}.info", id));
    let trash_info: TrashInfo = match tokio::fs::read_to_string(&info_path).await {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(info) => info,
            Err(_) => return Err(StatusCode::NOT_FOUND),
        },
        Err(_) => return Err(StatusCode::NOT_FOUND),
    };

    let metadata = match tokio::fs::metadata(&trash_file_path).await {
        Ok(m) => m,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // ドロップチェック
    let (dc, ec, tp, dp, rt) = match mirakc_webui::tsdropcheck::check_ts_file(&trash_file_path, &mirakc_webui::tsdropcheck::CheckOptions::default()) {
        Ok(r) => (r.total_drops, r.total_errors, r.total_packets,
                  if r.total_packets > 0 { (r.total_drops as f64 / r.total_packets as f64) * 100.0 } else { 0.0 },
                  r.duration),
        Err(_) => (0, 0, 0, 0.0, "00:00:00.00".to_string()),
    };

    // 番組情報
    let (program_name, channel_name) = {
        let filename = id.trim_end_matches(".m2ts").trim_end_matches(".ts");
        let parts: Vec<&str> = filename.rsplitn(2, '_').collect();
        if parts.len() >= 2 {
            match recorded::fetch_program_info(&state.mirakc_api, parts[1]).await {
                Some(info) => (info.name, info.service_name),
                None => ("不明".to_string(), "不明".to_string()),
            }
        } else {
            ("不明".to_string(), "不明".to_string())
        }
    };

    let thumb_basename = id.trim_end_matches(".m2ts").trim_end_matches(".ts");
    let thumb_path = state.trash_thumb_dir.join(format!("{}.jpg", thumb_basename));
    let thumbnail_url = if thumb_path.exists() {
        Some(format!("/api/trash/thumbnails/{}.jpg", thumb_basename))
    } else {
        None
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let ttl_secs = state.trash_ttl_days * 24 * 3600;
    let expired = now - trash_info.deleted_at > ttl_secs;

    Ok(Json(TrashFileView {
        id: id.clone(),
        filename: id,
        program_name,
        channel_name,
        recording_time: rt,
        drop_count: dc,
        error_count: ec,
        total_packets: tp,
        filesize_mb: metadata.len() as f64 / 1024.0 / 1024.0,
        drop_percentage: dp,
        thumbnail_url,
        deleted_at: trash_info.deleted_at,
        days_remaining: if expired { 0 } else {
            ((ttl_secs - (now - trash_info.deleted_at)) / (24 * 3600)) as u64
        },
    }))
}

async fn serve_trash_thumbnail(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<Response, StatusCode> {
    let thumbnail_path = state.trash_thumb_dir.join(&filename);

    if !thumbnail_path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    let content = tokio::fs::read(&thumbnail_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((
        [("Content-Type", "image/jpeg")],
        content,
    ).into_response())
}

async fn cleanup_expired(state: &AppState) -> usize {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let ttl_secs = state.trash_ttl_days * 24 * 3600;

    let mut cleaned = 0;

    let mut entries = match tokio::fs::read_dir(&state.trash_dir).await {
        Ok(e) => e,
        Err(_) => return 0,
    };

    while let Some(entry) = entries.next_entry().await.ok() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let id = path.file_name().unwrap().to_string_lossy().to_string();
        let info_path = state.trash_dir.join(format!("{}.info", id));

        let trash_info: TrashInfo = match tokio::fs::read_to_string(&info_path).await {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(info) => info,
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        if now - trash_info.deleted_at > ttl_secs {
            // 期限切れ → 永久削除
            tokio::fs::remove_file(&path).await.ok();
            tokio::fs::remove_file(&info_path).await.ok();

            let thumb_basename = id.trim_end_matches(".m2ts").trim_end_matches(".ts");
            let trash_thumb_path = state.trash_thumb_dir.join(format!("{}.jpg", thumb_basename));
            tokio::fs::remove_file(&trash_thumb_path).await.ok();

            cleaned += 1;
            info!("Cleanup: expired file deleted: {}", id);
        }
    }

    cleaned
}
