use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub start_at: u64,
    pub end_at: u64,
    pub duration: u64,
    pub service_id: u32,
    pub channel: String,
    pub genre: Option<Genre>,
    pub is_free: bool,
    pub is_paid: bool,
    pub is_original: bool,
    pub is_live: bool,
    pub is_recording: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Genre {
    pub genre1: String,
    pub genre1_sub: String,
    pub genre2: String,
    pub event: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub id: u32,
    pub name: String,
    pub service_type: String,
    pub service_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProgramInfo {
    pub program_id: String,
    pub name: String,
    pub service_id: u32,
    pub service_name: String,
    pub start_at: Option<u64>,
    pub duration: Option<u64>,
}

// ── Recording Schedule types ──

/// 予約スケジュール入力
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleInput {
    pub program_id: u64,
    pub start_at: u64,
    pub end_at: u64,
    pub service_id: u32,
    #[serde(default)]
    pub options: ScheduleOptions,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 予約オプション
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScheduleOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_tuner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
}

/// 予約スケジュール（mirakc応答）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingSchedule {
    pub program_id: u64,
    pub start_at: u64,
    pub end_at: u64,
    pub service_id: u32,
    pub service_name: String,
    pub program_name: String,
    pub is_paid: bool,
    pub tags: Vec<String>,
    pub options: ScheduleOptions,
    pub state: String,
}

/// mirakc API から番組情報を取得
pub async fn fetch_program(api_base: &str, program_id: &str) -> Result<Program, reqwest::Error> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/programs/{}", api_base, program_id);
    info!("Fetching program info: {}", url);
    client.get(&url).send().await?.json().await
}

/// mirakc API からサービス情報を取得
pub async fn fetch_service(api_base: &str, service_id: u32) -> Result<Service, reqwest::Error> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/services/{}", api_base, service_id);
    info!("Fetching service info: {}", url);
    client.get(&url).send().await?.json().await
}

/// ファイル名から programId を抽出
/// 形式: <datetime>_<program-id>.m2ts または <datetime>_<program-id>.ts
pub fn extract_program_id_from_filename(filename: &str) -> Option<String> {
    let filename = filename.trim_end_matches(".m2ts").trim_end_matches(".ts");
    let parts: Vec<&str> = filename.rsplitn(2, '_').collect();
    if parts.len() >= 2 {
        Some(parts[1].to_string())
    } else {
        None
    }
}

/// 番組名とチャンネル名を取得
pub async fn fetch_program_info(
    api_base: &str,
    filename: &str,
) -> Option<ProgramInfo> {
    let program_id = extract_program_id_from_filename(filename)?;
    
    let program = fetch_program(api_base, &program_id).await.ok()?;
    let service = fetch_service(api_base, program.service_id).await.ok()?;
    
    Some(ProgramInfo {
        program_id: program.id,
        name: program.name,
        service_id: program.service_id,
        service_name: service.name,
        start_at: Some(program.start_at),
        duration: Some(program.duration),
    })
}

// ── Recording Schedule API functions ──

/// 番組一覧取得（期間指定）
pub async fn fetch_programs(
    api_base: &str,
    start_at: Option<u64>,
    end_at: Option<u64>,
) -> Result<Vec<Program>, reqwest::Error> {
    let client = reqwest::Client::new();
    let mut url = format!("{}/api/programs", api_base);
    if let (Some(s), Some(e)) = (start_at, end_at) {
        url.push_str(&format!("&start_at={}&end_at={}", s, e));
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
