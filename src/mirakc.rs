use serde::{Deserialize, Serialize};
use tracing::{info, warn};

// ── mirakc 3.x API types (Mirakurun compatible) ──

/// MirakurunProgram (mirakc 3.x API)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Program {
    pub id: u64,
    #[serde(default)]
    pub event_id: u16,
    #[serde(default)]
    pub service_id: u16,
    #[serde(default)]
    pub network_id: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<i64>,
    pub is_free: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extended: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<ProgramVideo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<ProgramAudio>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audios: Vec<ProgramAudio>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genres: Option<Vec<EpgGenre>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series: Option<ProgramSeries>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_items: Vec<ProgramRelatedItem>,
}

/// Video descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramVideo {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub video_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(default)]
    pub stream_content: Option<u8>,
    #[serde(default)]
    pub component_type: Option<u8>,
}

/// Audio descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramAudio {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_type: Option<String>,
    #[serde(default)]
    pub content_type: u8,
    #[serde(default)]
    pub sampling_rate: i32,
    #[serde(default)]
    pub bit_rate: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_component_flag: Option<bool>,
}

/// EPG genre
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgGenre {
    pub lv1: u8,
    pub lv2: u8,
    pub un1: u8,
    pub un2: u8,
}

/// Program series info (mirakc 3.x)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramSeries {
    pub id: u16,
    pub repeat: u8,
    pub pattern: u8,
    pub expire_at: i64,
    pub episode: u16,
    pub last_episode: u16,
    pub name: String,
}

/// Related program item
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramRelatedItem {
    #[serde(rename = "type")]
    pub group_type: String,
    #[serde(default)]
    pub network_id: Option<u16>,
    #[serde(default)]
    pub service_id: u16,
    #[serde(default)]
    pub event_id: u16,
}

/// MirakurunService (mirakc 3.x API)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub id: u64,
    pub service_id: u16,
    pub network_id: u16,
    #[serde(rename = "type")]
    pub service_type: u16,
    #[serde(default)]
    pub logo_id: i16,
    #[serde(default)]
    pub remote_control_key_id: u16,
    pub name: String,
    pub channel: ServiceChannel,
    pub has_logo_data: bool,
}

/// Service channel info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceChannel {
    #[serde(rename = "type")]
    pub channel_type: String,
    pub channel: String,
}

/// mirakc API から番組情報を取得
pub async fn fetch_program(api_base: &str, program_id: &str) -> Result<Program, reqwest::Error> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/programs/{}", api_base, program_id);
    info!("Fetching program info: {}", url);
    client.get(&url).send().await?.json().await
}

/// mirakc API からサービス情報を取得
pub async fn fetch_service(api_base: &str, service_id: u64) -> Result<Service, reqwest::Error> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/services/{}", api_base, service_id);
    info!("Fetching service info: {}", url);
    client.get(&url).send().await?.json().await
}

/// 録画ファイル用の番組情報（簡易ビュー）
#[derive(Debug, Clone, Serialize)]
pub struct ProgramInfo {
    pub program_id: String,
    pub name: String,
    pub service_id: u64,
    pub service_name: String,
    pub start_at: Option<u64>,
    pub duration: Option<u64>,
}

/// ファイル名からprogramIdを抽出して番組・サービス情報を取得
pub async fn fetch_program_info(
    api_base: &str,
    filename: &str,
) -> Option<ProgramInfo> {
    let program_id = extract_program_id_from_filename(filename)?;
    
    let program = fetch_program(api_base, &program_id).await.ok()?;
    let service = fetch_service(api_base, program.id).await.ok()?;
    
    Some(ProgramInfo {
        program_id: program.id.to_string(),
        name: program.name.unwrap_or_else(|| "不明".to_string()),
        service_id: program.id,
        service_name: service.name,
        start_at: program.start_at.map(|v| v as u64),
        duration: program.duration.map(|v| v as u64),
    })
}

// ── Recording Schedule types (mirakc 3.x) ──

/// 予約スケジュール入力 (mirakc 3.x: WebRecordingScheduleInput)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleInput {
    pub program_id: u64,
    #[serde(default)]
    pub options: ScheduleOptions,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 予約オプション (mirakc 3.x: RecordingOptions)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScheduleOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_tuner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
}

/// 予約スケジュール（mirakc 3.x: WebRecordingSchedule）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingSchedule {
    pub state: String,
    pub program: Program,
    #[serde(default)]
    pub options: ScheduleOptions,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed_reason: Option<String>,
}

/// 予約一覧取得
pub async fn fetch_schedules(
    api_base: &str,
) -> Result<Vec<RecordingSchedule>, reqwest::Error> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/recording/schedules", api_base);
    info!("Fetching schedules: {}", url);
    client.get(&url).send().await?.json().await
}

/// 予約作成
pub async fn create_schedule(
    api_base: &str,
    input: ScheduleInput,
) -> Result<RecordingSchedule, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/recording/schedules", api_base);
    let resp = client.post(&url).json(&input).send().await?;
    if resp.status().is_success() {
        let schedule: RecordingSchedule = resp.json().await?;
        Ok(schedule)
    } else {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        warn!("Failed to create schedule: {} - {}", status, body);
        Err(format!("schedule creation failed: {} - {}", status, body).into())
    }
}

/// 予約削除
pub async fn delete_schedule(
    api_base: &str,
    program_id: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/recording/schedules/{}", api_base, program_id);
    let resp = client.delete(&url).send().await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status().as_u16();
        warn!("Failed to delete schedule {}: {}", program_id, status);
        Err(format!("schedule deletion failed: {}", status).into())
    }
}

/// 番組一覧取得（期間指定）
pub async fn fetch_programs(
    api_base: &str,
    start_at: Option<u64>,
    end_at: Option<u64>,
) -> Result<Vec<Program>, reqwest::Error> {
    let client = reqwest::Client::new();
    let mut url = format!("{}/api/programs", api_base);
    if let (Some(s), Some(e)) = (start_at, end_at) {
        url.push_str(&format!("?startAt={}&endAt={}", s, e));
    }
    info!("Fetching programs: {}", url);
    client.get(&url).send().await?.json().await
}

/// サービス一覧取得
pub async fn fetch_services(api_base: &str) -> Result<Vec<Service>, reqwest::Error> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/services", api_base);
    info!("Fetching services: {}", url);
    client.get(&url).send().await?.json().await
}

/// ファイル名から programId を抽出
pub fn extract_program_id_from_filename(filename: &str) -> Option<String> {
    let filename = filename.trim_end_matches(".m2ts").trim_end_matches(".ts");
    let parts: Vec<&str> = filename.rsplitn(2, '_').collect();
    if parts.len() >= 2 {
        Some(parts[1].to_string())
    } else {
        None
    }
}
