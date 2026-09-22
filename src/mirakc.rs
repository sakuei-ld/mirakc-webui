use serde::{Deserialize, Serialize};
use tracing::info;

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
