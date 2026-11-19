use base64::{Engine as _, engine::general_purpose};
use futures_util::{StreamExt, stream};
use ipnet::IpNet;
use percent_encoding::percent_decode_str;
use rand::Rng;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State};
use tokio::net::TcpStream;
use url::Url;
use uuid::Uuid;

struct RunnerState {
    running: AtomicBool,
    cancelled: AtomicBool,
}

struct PanelSessionState {
    cookies: Mutex<HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TargetValidation {
    valid: Vec<String>,
    invalid: Vec<InvalidLine>,
    duplicate_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InvalidLine {
    line: usize,
    value: String,
    reason: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SpeedTestRequest {
    targets: String,
    concurrency: u16,
    attempts: u8,
    download_count: u16,
    download_seconds: u16,
    port: u16,
    #[serde(default)]
    latency_min: f64,
    latency_max: f64,
    packet_loss_max: f64,
    speed_min: f64,
    disable_download: bool,
    test_url: Option<String>,
    httping: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SpeedResult {
    address: String,
    sent: u32,
    received: u32,
    packet_loss: f64,
    average_latency: f64,
    download_speed: f64,
    colo: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SpeedTestResponse {
    run_id: String,
    tested_count: usize,
    results: Vec<SpeedResult>,
    persistence_warning: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistorySummary {
    run_id: String,
    created_at: i64,
    target: String,
    ip_mode: String,
    tested_count: usize,
    result_count: usize,
    best_latency: f64,
    best_speed: f64,
    status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryRun {
    request: SpeedTestRequest,
    results: Vec<SpeedResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InterruptedTask {
    run_id: String,
    started_at: i64,
    target: String,
    ip_mode: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Diagnostics {
    version: String,
    os: String,
    arch: String,
    generated_at: i64,
    recent_runs: Vec<HistorySummary>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticsExport {
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscriptionHistoryRequest {
    mode: String,
    input_nodes: usize,
    preferred_endpoints: usize,
    output_nodes: usize,
    formats: Vec<String>,
    endpoint_host: Option<String>,
    status: String,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubscriptionHistorySummary {
    run_id: String,
    created_at: i64,
    mode: String,
    input_nodes: usize,
    preferred_endpoints: usize,
    output_nodes: usize,
    formats: Vec<String>,
    endpoint_host: String,
    status: String,
    error: String,
}

const DEFAULT_CLOUDFLARE_SPEED_URL: &str = "https://cf.xiu2.xyz/url";
const FALLBACK_CLOUDFLARE_SPEED_URL: &str = "https://speed.cloudflare.com/__down?bytes=300000000";
const CFST_DOWNLOAD_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_12_6) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/98.0.4758.80 Safari/537.36";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalGenerateRequest {
    node_links: String,
    preferred_ips: String,
    name_prefix: String,
    keep_original_host: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalGenerateResponse {
    links: Vec<String>,
    raw_base64: String,
    input_nodes: usize,
    preferred_endpoints: usize,
    output_nodes: usize,
    warnings: Vec<String>,
    clash_yaml: String,
    surge_profile: String,
    changes: Vec<NodeChange>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeChange {
    protocol: String,
    name: String,
    original_address: String,
    preferred_address: String,
    host_sni_preserved: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NodePreview {
    protocol: String,
    name: String,
    address: String,
    port: String,
    tls: String,
    transport: String,
    host: String,
    sni: String,
    valid: bool,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishRequest {
    base_url: String,
    generate_path: String,
    api_key: Option<String>,
    node_links: String,
    preferred_ips: String,
    name_prefix: String,
    keep_original_host: bool,
}

#[derive(Debug, Serialize)]
struct PublishResponse {
    data: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishConnectionRequest {
    base_url: String,
    generate_path: String,
    api_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishConnectionResponse {
    endpoint: String,
    status: u16,
    reachable: bool,
    note: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreeXuiRequest {
    panel_url: String,
    api_token: String,
    filter: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PanelLoginRequest {
    panel_url: String,
    username: String,
    password: String,
    two_factor_code: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PanelLoginResponse {
    authenticated: bool,
    two_factor_enabled: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedPanelLogin {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedPanelLoginStatus {
    saved: bool,
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PanelClientRequest {
    panel_url: String,
    api_token: String,
    email: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PanelClientSummary {
    email: String,
    enabled: bool,
    sub_id: String,
    inbound_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PanelClientLinksResponse {
    email: String,
    links: Vec<String>,
    count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreeXuiResponse {
    links: Vec<String>,
    count: usize,
    source: String,
}

const PANEL_KEYRING_SERVICE: &str = "cc.supconit.iptools.3xui";
const PANEL_LOGIN_KEYRING_SERVICE: &str = "cc.supconit.iptools.3xui.login";

fn panel_keyring_entry(panel_url: &str) -> Result<keyring::Entry, String> {
    let normalized = panel_url.trim().trim_end_matches('/');
    if normalized.is_empty() {
        return Err("请先填写 3X-UI 面板 URL".into());
    }
    keyring::Entry::new(PANEL_KEYRING_SERVICE, normalized)
        .map_err(|e| format!("无法访问 Windows 凭据管理器：{e}"))
}

fn panel_login_keyring_entry(panel_url: &str) -> Result<keyring::Entry, String> {
    let normalized = panel_session_key(panel_url)?;
    keyring::Entry::new(PANEL_LOGIN_KEYRING_SERVICE, &normalized)
        .map_err(|e| format!("无法访问 Windows 凭据管理器：{e}"))
}

fn read_saved_panel_login(panel_url: &str) -> Result<Option<SavedPanelLogin>, String> {
    match panel_login_keyring_entry(panel_url)?.get_password() {
        Ok(value) => serde_json::from_str(&value)
            .map(Some)
            .map_err(|_| "保存的 3X-UI 登录凭据已损坏，请删除后重新保存".to_string()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("无法读取登录凭据：{error}")),
    }
}

#[tauri::command]
fn save_panel_login(panel_url: String, username: String, password: String) -> Result<(), String> {
    if username.trim().is_empty() || password.is_empty() {
        return Err("用户名或密码为空，未保存".into());
    }
    let saved = SavedPanelLogin {
        username: username.trim().to_string(),
        password,
    };
    let value = serde_json::to_string(&saved).map_err(|e| format!("无法编码登录凭据：{e}"))?;
    panel_login_keyring_entry(&panel_url)?
        .set_password(&value)
        .map_err(|e| format!("无法保存登录凭据：{e}"))
}

#[tauri::command]
fn panel_login_status(panel_url: String) -> Result<SavedPanelLoginStatus, String> {
    let saved = read_saved_panel_login(&panel_url)?;
    Ok(SavedPanelLoginStatus {
        saved: saved.is_some(),
        username: saved.map(|value| value.username),
    })
}

#[tauri::command]
fn delete_panel_login(panel_url: String) -> Result<(), String> {
    match panel_login_keyring_entry(&panel_url)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("无法删除登录凭据：{error}")),
    }
}

#[tauri::command]
fn save_panel_token(panel_url: String, token: String) -> Result<(), String> {
    if token.trim().is_empty() {
        return Err("Token 为空，未保存".into());
    }
    panel_keyring_entry(&panel_url)?
        .set_password(token.trim())
        .map_err(|e| format!("无法保存 Token：{e}"))
}

#[tauri::command]
fn panel_token_status(panel_url: String) -> Result<bool, String> {
    match panel_keyring_entry(&panel_url)?.get_password() {
        Ok(value) => Ok(!value.is_empty()),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(error) => Err(format!("无法读取凭据状态：{error}")),
    }
}

#[tauri::command]
fn delete_panel_token(panel_url: String) -> Result<(), String> {
    match panel_keyring_entry(&panel_url)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("无法删除 Token：{error}")),
    }
}

#[tauri::command]
fn validate_targets(input: String) -> TargetValidation {
    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut duplicate_count = 0;
    for (index, raw) in input.lines().enumerate() {
        let value = raw.trim();
        if value.is_empty() || value.starts_with('#') {
            continue;
        }
        let parsed = value.parse::<IpNet>().is_ok() || value.parse::<IpAddr>().is_ok();
        if !parsed {
            invalid.push(InvalidLine {
                line: index + 1,
                value: value.into(),
                reason: "不是有效的 IPv4、IPv6 或 CIDR".into(),
            });
        } else if seen.insert(value.to_lowercase()) {
            valid.push(value.into());
        } else {
            duplicate_count += 1;
        }
    }
    TargetValidation {
        valid,
        invalid,
        duplicate_count,
    }
}

#[tauri::command]
async fn run_native_speed_test(
    app: tauri::AppHandle,
    state: State<'_, RunnerState>,
    request: SpeedTestRequest,
) -> Result<SpeedTestResponse, String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("已有测速任务正在运行".into());
    }
    state.cancelled.store(false, Ordering::SeqCst);
    let request_for_history = request.clone();
    let run_id = Uuid::new_v4().to_string();
    let tracking_warning = record_task_start(&app, &run_id, &request_for_history).err();
    let mut result = run_native_speed_test_inner(&app, &state, request, &run_id).await;
    if let Ok(response) = &mut result
        && let Err(error) = persist_run(&app, &request_for_history, response)
    {
        response.persistence_warning = Some(error);
    }
    if let Ok(response) = &mut result
        && let Some(warning) = tracking_warning
    {
        response.persistence_warning = Some(match response.persistence_warning.take() {
            Some(existing) => format!("{existing}；任务状态保存失败：{warning}"),
            None => format!("任务状态保存失败：{warning}"),
        });
    }
    let final_status = match &result {
        Ok(_) => "completed",
        Err(error) if error == "测速已取消" => "cancelled",
        Err(_) => "failed",
    };
    let _ = record_task_end(&app, &run_id, final_status);
    state.running.store(false, Ordering::SeqCst);
    result
}

async fn run_native_speed_test_inner(
    app: &tauri::AppHandle,
    state: &RunnerState,
    request: SpeedTestRequest,
    run_id: &str,
) -> Result<SpeedTestResponse, String> {
    let validation = validate_targets(request.targets.clone());
    if !validation.invalid.is_empty() {
        return Err(format!(
            "候选网段包含 {} 个无效行",
            validation.invalid.len()
        ));
    }
    let candidates = generate_candidates(&validation.valid)?;
    if candidates.is_empty() {
        return Err("候选网段没有生成可测试 IP".into());
    }
    let _ = app.emit(
        "speed-log",
        format!("任务 {run_id}：已生成 {} 个候选 IP", candidates.len()),
    );

    let test_url = effective_test_url(
        request.test_url.as_deref(),
        request.disable_download,
        request.httping,
    );
    let parsed_url = test_url
        .as_deref()
        .map(Url::parse)
        .transpose()
        .map_err(|_| "VPS 测速 URL 格式无效".to_string())?;
    if let Some(url) = &parsed_url
        && !matches!(url.scheme(), "http" | "https")
    {
        return Err("测速 URL 必须使用 HTTP 或 HTTPS".into());
    }
    let _ = app.emit(
        "speed-log",
        format!(
            "延迟模式：{}，端口 {}，每个 IP {} 次；下载测速：{}",
            if request.httping { "HTTPing" } else { "TCPing" },
            request.port,
            request.attempts.clamp(1, 20),
            if request.disable_download {
                "关闭"
            } else if request
                .test_url
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            {
                "自定义地址"
            } else {
                "CFST 默认地址"
            }
        ),
    );

    let total = candidates.len();
    let completed = Arc::new(AtomicUsize::new(0));
    let _ = app.emit("speed-stage", format!("延迟测试 0/{total}"));
    let concurrency = request.concurrency.clamp(1, 1000) as usize;
    let attempts = request.attempts.clamp(1, 20);
    let port = request.port;
    let httping = request.httping;
    let url_for_probe = test_url.clone();
    let cancelled = &state.cancelled;
    let app_for_probe = app.clone();
    let completed_for_probe = completed.clone();

    let mut latency_results: Vec<SpeedResult> = stream::iter(candidates.into_iter().map(|ip| {
        let url = url_for_probe.clone();
        let app = app_for_probe.clone();
        let completed = completed_for_probe.clone();
        async move {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            let result = probe_ip(ip, port, attempts, httping, url.as_deref()).await;
            let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
            if done.is_multiple_of(20) || done == total {
                let _ = app.emit("speed-stage", format!("延迟测试 {done}/{total}"));
            }
            result
        }
    }))
    .buffer_unordered(concurrency)
    .filter_map(|value| async move { value })
    .collect()
    .await;

    if state.cancelled.load(Ordering::SeqCst) {
        return Err("测速已取消".into());
    }
    let reachable_count = latency_results.len();
    latency_results.retain(|item| {
        item.average_latency >= request.latency_min
            && item.average_latency <= request.latency_max
            && item.packet_loss <= request.packet_loss_max
    });
    latency_results.sort_by(|a, b| {
        a.packet_loss
            .total_cmp(&b.packet_loss)
            .then(a.average_latency.total_cmp(&b.average_latency))
    });
    let _ = app.emit(
        "speed-log",
        format!(
            "延迟测试完成：{} 个可达，{} 个通过延迟/丢包筛选",
            reachable_count,
            latency_results.len()
        ),
    );
    if latency_results.is_empty() {
        return Err("没有候选入口通过延迟和丢包筛选".into());
    }

    let tested_count = total;
    if !request.disable_download {
        let count = request.download_count.max(1) as usize;
        latency_results.truncate(count);
        let download_total = latency_results.len();
        let url = test_url.ok_or_else(|| "缺少下载测速 URL".to_string())?;
        let seconds = request.download_seconds.clamp(1, 60);
        let _ = app.emit("speed-stage", format!("下载测速 0/{download_total}"));
        let _ = app.emit(
            "speed-log",
            format!("开始下载测速：队列 {download_total} 个候选"),
        );
        let mut measured = Vec::new();
        for (index, mut item) in latency_results.into_iter().enumerate() {
            if state.cancelled.load(Ordering::Relaxed) {
                return Err("测速已取消".into());
            }
            let measured_result = measure_download(
                item.address
                    .parse()
                    .map_err(|_| "结果 IP 无效".to_string())?,
                &url,
                port,
                seconds,
                &state.cancelled,
            )
            .await;
            match measured_result {
                Ok((speed, colo)) if speed > 0.0 => {
                    item.download_speed = speed;
                    item.colo = colo;
                }
                Ok(_) | Err(_) if url == DEFAULT_CLOUDFLARE_SPEED_URL => {
                    let _ = app.emit(
                        "speed-log",
                        format!("{} 未返回有效数据，切换官方下载接口", item.address),
                    );
                    match measure_download(
                        item.address
                            .parse()
                            .map_err(|_| "结果 IP 无效".to_string())?,
                        FALLBACK_CLOUDFLARE_SPEED_URL,
                        port,
                        seconds,
                        &state.cancelled,
                    )
                    .await
                    {
                        Ok((speed, colo)) => {
                            item.download_speed = speed;
                            item.colo = colo;
                        }
                        Err(error) if error == "测速已取消" => return Err(error),
                        Err(error) => {
                            let _ = app.emit(
                                "speed-log",
                                format!("下载测速 {} 失败：{}", item.address, error),
                            );
                        }
                    }
                }
                Ok(_) => {}
                Err(error) if error == "测速已取消" => return Err(error),
                Err(error) => {
                    let _ = app.emit(
                        "speed-log",
                        format!("下载测速 {} 失败：{}", item.address, error),
                    );
                }
            }
            let _ = app.emit(
                "speed-stage",
                format!("下载测速 {}/{download_total}", index + 1),
            );
            if item.download_speed >= request.speed_min {
                measured.push(item);
            }
        }
        latency_results = measured;
        latency_results.sort_by(|a, b| {
            b.download_speed
                .total_cmp(&a.download_speed)
                .then(a.average_latency.total_cmp(&b.average_latency))
        });
        let _ = app.emit(
            "speed-log",
            format!("下载测速完成：{} 个结果满足速度条件", latency_results.len()),
        );
    }
    let _ = app.emit("speed-stage", "已完成");
    let _ = app.emit(
        "speed-log",
        format!("任务完成：输出 {} 个优选 IP", latency_results.len()),
    );
    Ok(SpeedTestResponse {
        run_id: run_id.to_string(),
        tested_count,
        results: latency_results,
        persistence_warning: None,
    })
}

fn effective_test_url(
    configured: Option<&str>,
    disable_download: bool,
    httping: bool,
) -> Option<String> {
    configured
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            (!disable_download || httping).then(|| DEFAULT_CLOUDFLARE_SPEED_URL.to_string())
        })
}

fn open_history_db(app: &tauri::AppHandle) -> Result<Connection, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录：{e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建应用数据目录：{e}"))?;
    let connection =
        Connection::open(dir.join("iptools.db")).map_err(|e| format!("无法打开历史数据库：{e}"))?;
    init_history_schema(&connection)?;
    Ok(connection)
}

fn init_history_schema(connection: &Connection) -> Result<(), String> {
    connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS speed_runs(
            run_id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, target TEXT NOT NULL,
            ip_mode TEXT NOT NULL, tested_count INTEGER NOT NULL, result_count INTEGER NOT NULL,
            best_latency REAL NOT NULL, best_speed REAL NOT NULL, request_json TEXT NOT NULL, results_json TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS task_runs(
            run_id TEXT PRIMARY KEY, started_at INTEGER NOT NULL, status TEXT NOT NULL,
            target TEXT NOT NULL, ip_mode TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_task_runs_status ON task_runs(status);
        CREATE TABLE IF NOT EXISTS subscription_runs(
            run_id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, mode TEXT NOT NULL,
            input_nodes INTEGER NOT NULL, preferred_endpoints INTEGER NOT NULL,
            output_nodes INTEGER NOT NULL, formats_json TEXT NOT NULL,
            endpoint_host TEXT NOT NULL, status TEXT NOT NULL, error TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_subscription_runs_created_at ON subscription_runs(created_at DESC);").map_err(|e| format!("无法初始化历史数据库：{e}"))
}

fn unix_timestamp() -> Result<i64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .map_err(|e| e.to_string())
}

fn request_target_and_mode(request: &SpeedTestRequest) -> (String, String) {
    let target = request
        .test_url
        .as_deref()
        .and_then(|value| Url::parse(value).ok())
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "TCP only".into());
    let ip_mode = if request.targets.contains(':')
        && !request.targets.lines().any(|line| line.contains('.'))
    {
        "IPv6"
    } else {
        "IPv4"
    };
    (target, ip_mode.into())
}

fn insert_task_start(
    connection: &Connection,
    run_id: &str,
    request: &SpeedTestRequest,
) -> Result<(), String> {
    let (target, ip_mode) = request_target_and_mode(request);
    connection
        .execute(
            "INSERT INTO task_runs(run_id,started_at,status,target,ip_mode) VALUES(?1,?2,'running',?3,?4)",
            params![run_id, unix_timestamp()?, target, ip_mode],
        )
        .map(|_| ())
        .map_err(|e| format!("无法记录测速任务：{e}"))
}

fn record_task_start(
    app: &tauri::AppHandle,
    run_id: &str,
    request: &SpeedTestRequest,
) -> Result<(), String> {
    insert_task_start(&open_history_db(app)?, run_id, request)
}

fn record_task_end(app: &tauri::AppHandle, run_id: &str, status: &str) -> Result<(), String> {
    open_history_db(app)?
        .execute(
            "UPDATE task_runs SET status=?1 WHERE run_id=?2",
            params![status, run_id],
        )
        .map(|_| ())
        .map_err(|e| format!("无法更新测速任务状态：{e}"))
}

fn recover_tasks(connection: &Connection) -> Result<Vec<InterruptedTask>, String> {
    let tasks = {
        let mut statement = connection
            .prepare("SELECT run_id,started_at,target,ip_mode FROM task_runs WHERE status='running' ORDER BY started_at DESC")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok(InterruptedTask {
                    run_id: row.get(0)?,
                    started_at: row.get(1)?,
                    target: row.get(2)?,
                    ip_mode: row.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    connection
        .execute(
            "UPDATE task_runs SET status='interrupted' WHERE status='running'",
            [],
        )
        .map_err(|e| format!("无法标记中断任务：{e}"))?;
    Ok(tasks)
}

#[tauri::command]
fn recover_unfinished_tasks(app: tauri::AppHandle) -> Result<Vec<InterruptedTask>, String> {
    recover_tasks(&open_history_db(&app)?)
}

fn persist_run(
    app: &tauri::AppHandle,
    request: &SpeedTestRequest,
    response: &SpeedTestResponse,
) -> Result<(), String> {
    let connection = open_history_db(app)?;
    let created_at = unix_timestamp()?;
    let (target, ip_mode) = request_target_and_mode(request);
    let best_latency = response
        .results
        .iter()
        .map(|v| v.average_latency)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let best_speed = response
        .results
        .iter()
        .map(|v| v.download_speed)
        .reduce(f64::max)
        .unwrap_or(0.0);
    let request_json = serde_json::to_string(request).map_err(|e| e.to_string())?;
    let results_json = serde_json::to_string(&response.results).map_err(|e| e.to_string())?;
    connection.execute("INSERT INTO speed_runs(run_id,created_at,target,ip_mode,tested_count,result_count,best_latency,best_speed,request_json,results_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![response.run_id,created_at,target,ip_mode,response.tested_count as i64,response.results.len() as i64,best_latency,best_speed,request_json,results_json]).map_err(|e| format!("无法保存测速历史：{e}"))?;
    Ok(())
}

#[tauri::command]
fn list_history(app: tauri::AppHandle) -> Result<Vec<HistorySummary>, String> {
    let connection = open_history_db(&app)?;
    query_history(&connection)
}

#[tauri::command]
fn clear_history(app: tauri::AppHandle) -> Result<(), String> {
    let mut connection = open_history_db(&app)?;
    clear_history_tables(&mut connection)
}

fn clear_history_tables(connection: &mut Connection) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|e| format!("无法开始清理历史：{e}"))?;
    transaction
        .execute_batch(
            "DELETE FROM speed_runs; DELETE FROM task_runs; DELETE FROM subscription_runs;",
        )
        .map_err(|e| format!("无法清理历史：{e}"))?;
    transaction
        .commit()
        .map_err(|e| format!("无法提交历史清理：{e}"))
}

fn query_history(connection: &Connection) -> Result<Vec<HistorySummary>, String> {
    let mut statement = connection
        .prepare(
            "SELECT t.run_id,t.started_at AS created_at,t.target,t.ip_mode,
                COALESCE(s.tested_count,0),COALESCE(s.result_count,0),
                COALESCE(s.best_latency,0),COALESCE(s.best_speed,0),t.status
         FROM task_runs t LEFT JOIN speed_runs s ON s.run_id=t.run_id
         UNION ALL
         SELECT s.run_id,s.created_at,s.target,s.ip_mode,s.tested_count,s.result_count,
                s.best_latency,s.best_speed,'completed'
         FROM speed_runs s WHERE NOT EXISTS(SELECT 1 FROM task_runs t WHERE t.run_id=s.run_id)
         ORDER BY created_at DESC LIMIT 100",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(HistorySummary {
                run_id: row.get(0)?,
                created_at: row.get(1)?,
                target: row.get(2)?,
                ip_mode: row.get(3)?,
                tested_count: row.get::<_, i64>(4)? as usize,
                result_count: row.get::<_, i64>(5)? as usize,
                best_latency: row.get(6)?,
                best_speed: row.get(7)?,
                status: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn sanitize_subscription_history(
    request: SubscriptionHistoryRequest,
) -> Result<SubscriptionHistorySummary, String> {
    let mode = match request.mode.trim() {
        "local" => "local",
        "online" => "online",
        _ => return Err("订阅历史模式无效".into()),
    };
    let status = match request.status.trim() {
        "completed" => "completed",
        "failed" => "failed",
        _ => return Err("订阅历史状态无效".into()),
    };
    let formats = request
        .formats
        .into_iter()
        .map(|value| value.trim().chars().take(24).collect::<String>())
        .filter(|value| !value.is_empty())
        .take(8)
        .collect::<Vec<_>>();
    let endpoint_host = request
        .endpoint_host
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| {
            Url::parse(value)
                .ok()
                .and_then(|url| url.host_str().map(str::to_string))
                .or_else(|| {
                    (!value.contains('/') && !value.contains(char::is_whitespace))
                        .then(|| value.to_string())
                })
        })
        .unwrap_or_default()
        .chars()
        .take(255)
        .collect::<String>();
    let error = request
        .error
        .unwrap_or_default()
        .chars()
        .filter(|value| *value != '\r' && *value != '\n')
        .take(500)
        .collect::<String>();
    Ok(SubscriptionHistorySummary {
        run_id: Uuid::new_v4().to_string(),
        created_at: unix_timestamp()?,
        mode: mode.into(),
        input_nodes: request.input_nodes.min(500),
        preferred_endpoints: request.preferred_endpoints.min(500),
        output_nodes: request.output_nodes.min(500),
        formats,
        endpoint_host,
        status: status.into(),
        error,
    })
}

fn insert_subscription_history(
    connection: &Connection,
    record: &SubscriptionHistorySummary,
) -> Result<(), String> {
    let formats_json = serde_json::to_string(&record.formats).map_err(|e| e.to_string())?;
    connection
        .execute(
            "INSERT INTO subscription_runs(run_id,created_at,mode,input_nodes,preferred_endpoints,output_nodes,formats_json,endpoint_host,status,error) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                record.run_id,
                record.created_at,
                record.mode,
                record.input_nodes as i64,
                record.preferred_endpoints as i64,
                record.output_nodes as i64,
                formats_json,
                record.endpoint_host,
                record.status,
                record.error
            ],
        )
        .map(|_| ())
        .map_err(|e| format!("无法保存订阅生成历史：{e}"))
}

fn query_subscription_history(
    connection: &Connection,
) -> Result<Vec<SubscriptionHistorySummary>, String> {
    let mut statement = connection
        .prepare(
            "SELECT run_id,created_at,mode,input_nodes,preferred_endpoints,output_nodes,formats_json,endpoint_host,status,error
             FROM subscription_runs ORDER BY created_at DESC LIMIT 50",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            let formats_json: String = row.get(6)?;
            let formats = serde_json::from_str::<Vec<String>>(&formats_json).unwrap_or_default();
            Ok(SubscriptionHistorySummary {
                run_id: row.get(0)?,
                created_at: row.get(1)?,
                mode: row.get(2)?,
                input_nodes: row.get::<_, i64>(3)? as usize,
                preferred_endpoints: row.get::<_, i64>(4)? as usize,
                output_nodes: row.get::<_, i64>(5)? as usize,
                formats,
                endpoint_host: row.get(7)?,
                status: row.get(8)?,
                error: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn record_subscription_history(
    app: tauri::AppHandle,
    request: SubscriptionHistoryRequest,
) -> Result<(), String> {
    let record = sanitize_subscription_history(request)?;
    insert_subscription_history(&open_history_db(&app)?, &record)
}

#[tauri::command]
fn list_subscription_history(
    app: tauri::AppHandle,
) -> Result<Vec<SubscriptionHistorySummary>, String> {
    query_subscription_history(&open_history_db(&app)?)
}

#[tauri::command]
fn get_history_run(app: tauri::AppHandle, run_id: String) -> Result<HistoryRun, String> {
    let connection = open_history_db(&app)?;
    let (request_json, results_json): (String, String) = connection
        .query_row(
            "SELECT request_json,results_json FROM speed_runs WHERE run_id=?1",
            [run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| "找不到该测速历史".to_string())?;
    Ok(HistoryRun {
        request: serde_json::from_str(&request_json).map_err(|e| format!("历史参数损坏：{e}"))?,
        results: serde_json::from_str(&results_json).map_err(|e| format!("历史结果损坏：{e}"))?,
    })
}

#[tauri::command]
fn get_diagnostics(app: tauri::AppHandle) -> Result<Diagnostics, String> {
    let generated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    let recent_runs = query_history(&open_history_db(&app)?)?;
    Ok(Diagnostics {
        version: env!("CARGO_PKG_VERSION").into(), os: std::env::consts::OS.into(), arch: std::env::consts::ARCH.into(), generated_at,
        recent_runs,
        notes: vec!["此诊断包不包含节点链接、UUID、密码、API Key、3X-UI Token、完整测速 URL 或完整测速结果。".into()],
    })
}

#[tauri::command]
fn export_diagnostics_file(app: tauri::AppHandle) -> Result<DiagnosticsExport, String> {
    let diagnostics = get_diagnostics(app.clone())?;
    let downloads = app
        .path()
        .download_dir()
        .map_err(|e| format!("无法定位系统下载文件夹：{e}"))?;
    std::fs::create_dir_all(&downloads).map_err(|e| format!("无法创建下载文件夹：{e}"))?;
    let path = downloads.join(format!(
        "linkforge-diagnostics-{}.json",
        diagnostics.generated_at
    ));
    let content =
        serde_json::to_vec_pretty(&diagnostics).map_err(|e| format!("无法生成脱敏诊断包：{e}"))?;
    std::fs::write(&path, content).map_err(|e| format!("无法写入脱敏诊断包：{e}"))?;
    Ok(DiagnosticsExport {
        path: path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
fn cancel_speed_test(state: State<'_, RunnerState>) -> bool {
    if !state.running.load(Ordering::SeqCst) {
        return false;
    }
    state.cancelled.store(true, Ordering::SeqCst);
    true
}

fn generate_candidates(values: &[String]) -> Result<Vec<IpAddr>, String> {
    let mut rng = rand::rng();
    let mut output = Vec::new();
    for value in values {
        if let Ok(ip) = value.parse::<IpAddr>() {
            output.push(ip);
            continue;
        }
        let net = value
            .parse::<IpNet>()
            .map_err(|_| format!("无效网段：{value}"))?;
        match net {
            IpNet::V4(net) if net.prefix_len() <= 24 => {
                let blocks = 1usize
                    .checked_shl((24 - net.prefix_len()) as u32)
                    .unwrap_or(usize::MAX)
                    .min(16_384);
                let base = u32::from(net.network());
                for block in 0..blocks {
                    output.push(IpAddr::V4(Ipv4Addr::from(
                        base + ((block as u32) << 8) + rng.random_range(1..=254),
                    )));
                }
            }
            IpNet::V4(net) => {
                let hosts: Vec<_> = net.hosts().collect();
                if !hosts.is_empty() {
                    output.push(IpAddr::V4(hosts[rng.random_range(0..hosts.len())]));
                }
            }
            IpNet::V6(net) => {
                let base = u128::from(net.network());
                let host_bits = 128 - net.prefix_len();
                let mask = if host_bits == 128 {
                    u128::MAX
                } else {
                    (1u128 << host_bits) - 1
                };
                output.push(IpAddr::V6(Ipv6Addr::from(
                    base | (rng.random::<u128>() & mask),
                )));
            }
        }
    }
    output.sort();
    output.dedup();
    Ok(output)
}

async fn probe_ip(
    ip: IpAddr,
    port: u16,
    attempts: u8,
    httping: bool,
    test_url: Option<&str>,
) -> Option<SpeedResult> {
    let mut received = 0u32;
    let mut elapsed_ms = 0.0;
    for _ in 0..attempts {
        let started = Instant::now();
        let ok = if httping {
            match test_url {
                Some(url) => http_probe(ip, url, port).await,
                None => false,
            }
        } else {
            tokio::time::timeout(
                Duration::from_secs(1),
                TcpStream::connect(SocketAddr::new(ip, port)),
            )
            .await
            .map(|r| r.is_ok())
            .unwrap_or(false)
        };
        if ok {
            received += 1;
            elapsed_ms += started.elapsed().as_secs_f64() * 1000.0;
        }
    }
    if received == 0 {
        return None;
    }
    Some(SpeedResult {
        address: ip.to_string(),
        sent: attempts as u32,
        received,
        packet_loss: 1.0 - received as f64 / attempts as f64,
        average_latency: elapsed_ms / received as f64,
        download_speed: 0.0,
        colo: "N/A".into(),
    })
}

fn client_for_ip(
    ip: IpAddr,
    url: &Url,
    fallback_port: u16,
    timeout: Duration,
) -> Result<reqwest::Client, String> {
    let host = url
        .host_str()
        .ok_or_else(|| "测速 URL 缺少域名".to_string())?;
    let port = url.port_or_known_default().unwrap_or(fallback_port);
    reqwest::Client::builder()
        .no_proxy()
        .resolve(host, SocketAddr::new(ip, port))
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
}

async fn http_probe(ip: IpAddr, raw_url: &str, fallback_port: u16) -> bool {
    let Ok(url) = Url::parse(raw_url) else {
        return false;
    };
    let Ok(client) = client_for_ip(ip, &url, fallback_port, Duration::from_secs(4)) else {
        return false;
    };
    client
        .head(url)
        .send()
        .await
        .map(|r| r.status().is_success() || r.status().is_redirection())
        .unwrap_or(false)
}

async fn measure_download(
    ip: IpAddr,
    raw_url: &str,
    fallback_port: u16,
    seconds: u16,
    cancelled: &AtomicBool,
) -> Result<(f64, String), String> {
    let mut url = Url::parse(raw_url).map_err(|_| "下载测速 URL 无效".to_string())?;
    let duration = Duration::from_secs(seconds as u64);
    let request_started = Instant::now();
    let request_timeout = duration + Duration::from_secs(5);
    let mut redirect_count = 0usize;
    let response = 'redirects: loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err("测速已取消".into());
        }
        let remaining = request_timeout
            .checked_sub(request_started.elapsed())
            .ok_or_else(|| "下载测速请求超时".to_string())?;
        let client = client_for_ip(ip, &url, fallback_port, remaining)?;
        let request = client
            .get(url.clone())
            .header(reqwest::header::USER_AGENT, CFST_DOWNLOAD_USER_AGENT)
            .send();
        tokio::pin!(request);
        let response = loop {
            tokio::select! {
                result = &mut request => break result.map_err(|e| e.to_string())?,
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if cancelled.load(Ordering::Relaxed) {
                        return Err("测速已取消".into());
                    }
                }
            }
        };
        if response.status().is_redirection() {
            redirect_count += 1;
            if redirect_count > 10 {
                return Err("下载测速重定向次数过多".into());
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| format!("HTTP {} 重定向缺少 Location", response.status()))?;
            url = url
                .join(location)
                .map_err(|_| "下载测速重定向地址无效".to_string())?;
            continue 'redirects;
        }
        break 'redirects response;
    };
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let colo = response
        .headers()
        .get("cf-ray")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit_once('-').map(|(_, c)| c))
        .filter(|v| !v.is_empty())
        .unwrap_or("N/A")
        .to_string();
    let started = Instant::now();
    let mut bytes = 0usize;
    let mut body = response.bytes_stream();
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err("测速已取消".into());
        }
        if started.elapsed() >= duration {
            break;
        }
        match tokio::time::timeout(Duration::from_millis(100), body.next()).await {
            Ok(Some(chunk)) => bytes += chunk.map_err(|e| e.to_string())?.len(),
            Ok(None) => break,
            Err(_) => continue,
        }
    }
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    Ok((bytes as f64 / 1_048_576.0 / elapsed, colo))
}

#[derive(Clone)]
struct PreferredEndpoint {
    host: String,
    port: Option<u16>,
    label: String,
}

fn parse_endpoints(input: &str) -> (Vec<PreferredEndpoint>, Vec<String>) {
    let mut endpoints = Vec::new();
    let mut warnings = Vec::new();
    for (index, raw) in input.lines().enumerate() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let (address, label) = raw
            .split_once('#')
            .map(|(a, l)| (a.trim(), l.trim().to_string()))
            .unwrap_or((raw, String::new()));
        let parsed = if address.starts_with('[') {
            Url::parse(&format!("tcp://{address}"))
        } else if address.matches(':').count() > 1 {
            Url::parse(&format!("tcp://[{address}]"))
        } else {
            Url::parse(&format!("tcp://{address}"))
        };
        match parsed {
            Ok(url) if url.host_str().is_some() => endpoints.push(PreferredEndpoint {
                host: url.host_str().unwrap_or_default().into(),
                port: url.port(),
                label,
            }),
            _ => warnings.push(format!("优选地址第 {} 行无效", index + 1)),
        }
    }
    (endpoints, warnings)
}

fn expand_node_input(input: &str) -> Vec<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed.lines().all(|line| line.trim().contains("://")) {
        return trimmed
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
    }
    let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    let decoded = general_purpose::STANDARD
        .decode(&compact)
        .or_else(|_| general_purpose::STANDARD_NO_PAD.decode(&compact));
    match decoded.ok().and_then(|bytes| String::from_utf8(bytes).ok()) {
        Some(value) if value.contains("://") => value
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
        _ => trimmed
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    }
}

#[tauri::command]
fn preview_nodes(input: String) -> Vec<NodePreview> {
    expand_node_input(&input)
        .into_iter()
        .map(|node| preview_node(&node))
        .collect()
}

fn preview_node(node: &str) -> NodePreview {
    let invalid = |protocol: &str, message: &str| NodePreview {
        protocol: protocol.into(),
        name: "—".into(),
        address: "—".into(),
        port: "—".into(),
        tls: "—".into(),
        transport: "—".into(),
        host: "—".into(),
        sni: "—".into(),
        valid: false,
        message: message.into(),
    };
    if let Some(encoded) = node.strip_prefix("vmess://") {
        let decoded = general_purpose::STANDARD
            .decode(encoded)
            .or_else(|_| general_purpose::STANDARD_NO_PAD.decode(encoded));
        let Ok(bytes) = decoded else {
            return invalid("VMess", "Base64 无效");
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            return invalid("VMess", "JSON 无效");
        };
        return NodePreview {
            protocol: "VMess".into(),
            name: value
                .get("ps")
                .and_then(Value::as_str)
                .unwrap_or("未命名")
                .into(),
            address: value
                .get("add")
                .and_then(Value::as_str)
                .unwrap_or("—")
                .into(),
            port: value
                .get("port")
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.to_string())
                })
                .unwrap_or_else(|| "—".into()),
            tls: value
                .get("tls")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .unwrap_or("无")
                .into(),
            transport: value
                .get("net")
                .and_then(Value::as_str)
                .unwrap_or("tcp")
                .into(),
            host: value
                .get("host")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .unwrap_or("—")
                .into(),
            sni: value
                .get("sni")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .unwrap_or("—")
                .into(),
            valid: true,
            message: "凭据已隐藏".into(),
        };
    }
    let protocol = if node.starts_with("vless://") {
        "VLESS"
    } else if node.starts_with("trojan://") {
        "Trojan"
    } else {
        return invalid("未知", "不支持的节点协议");
    };
    let Ok(url) = Url::parse(node) else {
        return invalid(protocol, "URI 无效");
    };
    let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
    NodePreview {
        protocol: protocol.into(),
        name: url.fragment().unwrap_or("未命名").into(),
        address: url.host_str().unwrap_or("—").into(),
        port: url
            .port()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "—".into()),
        tls: query
            .get("security")
            .map(|v| v.as_ref())
            .unwrap_or("无")
            .into(),
        transport: query
            .get("type")
            .map(|v| v.as_ref())
            .unwrap_or("tcp")
            .into(),
        host: query
            .get("host")
            .map(|value| value.as_ref())
            .filter(|value| !value.is_empty())
            .unwrap_or("—")
            .into(),
        sni: query
            .get("sni")
            .map(|value| value.as_ref())
            .filter(|value| !value.is_empty())
            .unwrap_or("—")
            .into(),
        valid: url.host_str().is_some(),
        message: "凭据已隐藏".into(),
    }
}

#[tauri::command]
fn generate_local(request: LocalGenerateRequest) -> Result<LocalGenerateResponse, String> {
    let expanded = expand_node_input(&request.node_links);
    let nodes: Vec<&str> = expanded.iter().map(String::as_str).collect();
    if nodes.is_empty() {
        return Err("请粘贴至少一个节点链接".into());
    }
    let (endpoints, mut warnings) = parse_endpoints(&request.preferred_ips);
    if endpoints.is_empty() {
        return Err("请至少输入一个有效优选地址".into());
    }
    if nodes.len().saturating_mul(endpoints.len()) > 500 {
        return Err("生成节点超过 500 个，请减少输入".into());
    }
    let mut links = Vec::new();
    let mut changes = Vec::new();
    for node in &nodes {
        let before = preview_node(node);
        for (index, endpoint) in endpoints.iter().enumerate() {
            let label = if endpoint.label.is_empty() {
                format!("{:02}", index + 1)
            } else {
                endpoint.label.clone()
            };
            let name = [request.name_prefix.trim(), &label]
                .into_iter()
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
                .join("-");
            match transform_node(node, endpoint, &name, request.keep_original_host) {
                Ok(link) => {
                    let after = preview_node(&link);
                    changes.push(NodeChange {
                        protocol: before.protocol.clone(),
                        name: after.name,
                        original_address: before.address.clone(),
                        preferred_address: after.address,
                        host_sni_preserved: request.keep_original_host,
                    });
                    links.push(link);
                }
                Err(e) => warnings.push(e),
            }
        }
    }
    if links.is_empty() {
        return Err("没有成功生成节点，请检查格式".into());
    }
    let raw = links.join("\n");
    let (clash_yaml, clash_warnings) = generate_mihomo_yaml(&links);
    let (surge_profile, surge_warnings) = generate_surge_profile(&links);
    warnings.extend(clash_warnings);
    warnings.extend(surge_warnings);
    let mut names = std::collections::HashSet::new();
    let duplicate_names = changes
        .iter()
        .filter(|change| !names.insert(change.name.to_lowercase()))
        .count();
    if duplicate_names > 0 {
        warnings.push(format!(
            "检测到 {duplicate_names} 个重复节点名称，部分客户端可能覆盖同名节点"
        ));
    }
    let missing_sni = links
        .iter()
        .filter(|link| node_uses_tls(link) && !node_has_server_name(link))
        .count();
    if missing_sni > 0 {
        warnings.push(format!(
            "{missing_sni} 个 TLS/REALITY 节点缺少 SNI/Host，请确认源节点配置"
        ));
    }
    Ok(LocalGenerateResponse {
        raw_base64: general_purpose::STANDARD.encode(raw.as_bytes()),
        output_nodes: links.len(),
        input_nodes: nodes.len(),
        preferred_endpoints: endpoints.len(),
        links,
        warnings,
        clash_yaml,
        surge_profile,
        changes,
    })
}

fn node_uses_tls(link: &str) -> bool {
    if let Some(encoded) = link.strip_prefix("vmess://") {
        return general_purpose::STANDARD
            .decode(encoded)
            .or_else(|_| general_purpose::STANDARD_NO_PAD.decode(encoded))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|value| value_text(&value, "tls", "") == "tls");
    }
    Url::parse(link).ok().is_some_and(|url| {
        let query = url_query(&url);
        link.starts_with("trojan://")
            || matches!(
                query.get("security").map(String::as_str),
                Some("tls" | "reality")
            )
    })
}

fn node_has_server_name(link: &str) -> bool {
    if let Some(encoded) = link.strip_prefix("vmess://") {
        return general_purpose::STANDARD
            .decode(encoded)
            .or_else(|_| general_purpose::STANDARD_NO_PAD.decode(encoded))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|value| {
                !value_text(&value, "sni", "").is_empty()
                    || !value_text(&value, "host", "").is_empty()
            });
    }
    Url::parse(link).ok().is_some_and(|url| {
        let query = url_query(&url);
        ["sni", "host", "peer"]
            .iter()
            .any(|key| query.get(*key).is_some_and(|value| !value.is_empty()))
    })
}

fn yaml_quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}
fn value_text(value: &Value, key: &str, fallback: &str) -> String {
    value
        .get(key)
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .unwrap_or_else(|| v.to_string())
        })
        .filter(|v| v != "null")
        .unwrap_or_else(|| fallback.into())
}
fn url_query(url: &Url) -> std::collections::HashMap<String, String> {
    url.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}
fn decoded_user(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().into_owned()
}

fn generate_mihomo_yaml(links: &[String]) -> (String, Vec<String>) {
    let mut blocks = Vec::new();
    let mut warnings = Vec::new();
    for link in links {
        match link_to_mihomo(link) {
            Ok(block) => blocks.push(block),
            Err(error) => warnings.push(format!("Mihomo：{error}")),
        }
    }
    (format!("proxies:\n{}", blocks.join("\n")), warnings)
}

fn link_to_mihomo(link: &str) -> Result<String, String> {
    if let Some(encoded) = link.strip_prefix("vmess://") {
        let bytes = general_purpose::STANDARD
            .decode(encoded)
            .or_else(|_| general_purpose::STANDARD_NO_PAD.decode(encoded))
            .map_err(|_| "VMess Base64 无效".to_string())?;
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "VMess JSON 无效".to_string())?;
        let name = value_text(&value, "ps", "VMess");
        let server = value_text(&value, "add", "");
        let port = value_text(&value, "port", "443");
        let uuid = value_text(&value, "id", "");
        let network = value_text(&value, "net", "tcp");
        if server.is_empty() || uuid.is_empty() {
            return Err("VMess 缺少服务器或 UUID".into());
        }
        let mut out = format!(
            "  - name: {}\n    type: vmess\n    server: {}\n    port: {}\n    uuid: {}\n    alterId: {}\n    cipher: {}\n    udp: true",
            yaml_quote(&name),
            yaml_quote(&server),
            port,
            yaml_quote(&uuid),
            value_text(&value, "aid", "0"),
            yaml_quote(&value_text(&value, "scy", "auto"))
        );
        if value_text(&value, "tls", "") == "tls" {
            out.push_str("\n    tls: true");
            let sni = value_text(&value, "sni", "");
            if !sni.is_empty() {
                out.push_str(&format!("\n    servername: {}", yaml_quote(&sni)));
            }
        }
        if network != "tcp" {
            out.push_str(&format!("\n    network: {}", yaml_quote(&network)));
        }
        if network == "ws" {
            let path = value_text(&value, "path", "/");
            let host = value_text(&value, "host", "");
            out.push_str(&format!(
                "\n    ws-opts:\n      path: {}",
                yaml_quote(&path)
            ));
            if !host.is_empty() {
                out.push_str(&format!(
                    "\n      headers:\n        Host: {}",
                    yaml_quote(&host)
                ));
            }
        }
        return Ok(out);
    }
    let url = Url::parse(link).map_err(|_| "分享链接 URI 无效".to_string())?;
    let protocol = url.scheme();
    if !matches!(protocol, "vless" | "trojan") {
        return Err(format!("不支持 {protocol} 协议"));
    }
    let query = url_query(&url);
    let name = decoded_user(url.fragment().unwrap_or(protocol));
    let server = url.host_str().ok_or_else(|| "缺少服务器".to_string())?;
    let port = url.port().unwrap_or(443);
    let credential = decoded_user(url.username());
    let network = query.get("type").map(String::as_str).unwrap_or("tcp");
    let security = query.get("security").map(String::as_str).unwrap_or("");
    let mut out = format!(
        "  - name: {}\n    type: {}\n    server: {}\n    port: {}\n    {}: {}\n    udp: true",
        yaml_quote(&name),
        protocol,
        yaml_quote(server),
        port,
        if protocol == "vless" {
            "uuid"
        } else {
            "password"
        },
        yaml_quote(&credential)
    );
    if protocol == "trojan" || matches!(security, "tls" | "reality") {
        out.push_str("\n    tls: true");
    }
    if let Some(sni) = query.get("sni") {
        out.push_str(&format!("\n    servername: {}", yaml_quote(sni)));
    }
    if let Some(flow) = query.get("flow").filter(|v| !v.is_empty()) {
        out.push_str(&format!("\n    flow: {}", yaml_quote(flow)));
    }
    if let Some(fp) = query.get("fp").filter(|v| !v.is_empty()) {
        out.push_str(&format!("\n    client-fingerprint: {}", yaml_quote(fp)));
    }
    if security == "reality" {
        let pk = query
            .get("pbk")
            .ok_or_else(|| "REALITY 缺少公钥 pbk".to_string())?;
        out.push_str(&format!(
            "\n    reality-opts:\n      public-key: {}",
            yaml_quote(pk)
        ));
        if let Some(sid) = query.get("sid") {
            out.push_str(&format!("\n      short-id: {}", yaml_quote(sid)));
        }
    }
    if network != "tcp" {
        out.push_str(&format!("\n    network: {}", yaml_quote(network)));
    }
    if network == "ws" {
        out.push_str(&format!(
            "\n    ws-opts:\n      path: {}",
            yaml_quote(query.get("path").map(String::as_str).unwrap_or("/"))
        ));
        if let Some(host) = query.get("host").filter(|v| !v.is_empty()) {
            out.push_str(&format!(
                "\n      headers:\n        Host: {}",
                yaml_quote(host)
            ));
        }
    }
    if network == "grpc" {
        out.push_str(&format!(
            "\n    grpc-opts:\n      grpc-service-name: {}",
            yaml_quote(query.get("serviceName").map(String::as_str).unwrap_or(""))
        ));
    }
    if matches!(network, "xhttp" | "splithttp") {
        out.push_str(&format!(
            "\n    xhttp-opts:\n      path: {}",
            yaml_quote(query.get("path").map(String::as_str).unwrap_or("/"))
        ));
        if let Some(host) = query.get("host").filter(|v| !v.is_empty()) {
            out.push_str(&format!("\n      host: {}", yaml_quote(host)));
        }
    }
    Ok(out)
}

fn surge_value(value: &str) -> String {
    if value.contains([',', ' ', '|']) {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.into()
    }
}
fn generate_surge_profile(links: &[String]) -> (String, Vec<String>) {
    let mut lines = vec!["[Proxy]".to_string()];
    let mut warnings = Vec::new();
    for link in links {
        match link_to_surge(link) {
            Ok(line) => lines.push(line),
            Err(error) => warnings.push(format!("Surge：{error}")),
        }
    }
    (lines.join("\n"), warnings)
}
fn link_to_surge(link: &str) -> Result<String, String> {
    let (
        protocol,
        name,
        server,
        port,
        credential,
        network,
        security,
        sni,
        path,
        host,
        flow,
        pbk,
        sid,
    ) = if let Some(encoded) = link.strip_prefix("vmess://") {
        let bytes = general_purpose::STANDARD
            .decode(encoded)
            .or_else(|_| general_purpose::STANDARD_NO_PAD.decode(encoded))
            .map_err(|_| "VMess Base64 无效".to_string())?;
        let v: Value = serde_json::from_slice(&bytes).map_err(|_| "VMess JSON 无效".to_string())?;
        (
            "vmess".to_string(),
            value_text(&v, "ps", "VMess"),
            value_text(&v, "add", ""),
            value_text(&v, "port", "443"),
            value_text(&v, "id", ""),
            value_text(&v, "net", "tcp"),
            value_text(&v, "tls", ""),
            value_text(&v, "sni", ""),
            value_text(&v, "path", "/"),
            value_text(&v, "host", ""),
            String::new(),
            String::new(),
            String::new(),
        )
    } else {
        let u = Url::parse(link).map_err(|_| "分享链接 URI 无效".to_string())?;
        let q = url_query(&u);
        let p = u.scheme().to_string();
        if !matches!(p.as_str(), "vless" | "trojan") {
            return Err(format!("不支持 {p} 协议"));
        }
        (
            p.clone(),
            decoded_user(u.fragment().unwrap_or(&p)),
            u.host_str().unwrap_or("").into(),
            u.port().unwrap_or(443).to_string(),
            decoded_user(u.username()),
            q.get("type").cloned().unwrap_or_else(|| "tcp".into()),
            q.get("security").cloned().unwrap_or_default(),
            q.get("sni").cloned().unwrap_or_default(),
            q.get("path").cloned().unwrap_or_else(|| "/".into()),
            q.get("host").cloned().unwrap_or_default(),
            q.get("flow").cloned().unwrap_or_default(),
            q.get("pbk").cloned().unwrap_or_default(),
            q.get("sid").cloned().unwrap_or_default(),
        )
    };
    if server.is_empty() || credential.is_empty() {
        return Err(format!("{name} 缺少服务器或凭据"));
    }
    if protocol != "vless" && security == "reality" {
        return Err(format!("{name} 的 REALITY 不能由 Surge 无损表达"));
    }
    if !matches!(
        network.as_str(),
        "tcp" | "raw" | "ws" | "httpupgrade" | "grpc" | "xhttp" | "splithttp"
    ) {
        return Err(format!("{name} 使用 Surge 不支持的传输 {network}"));
    }
    if network == "grpc" && protocol != "vless" {
        return Err(format!("{name} 的 gRPC 仅能用于 Surge VLESS"));
    }
    let mut parts = vec![
        protocol.clone(),
        server,
        port,
        format!(
            "{}={}",
            if protocol == "trojan" {
                "password"
            } else {
                "username"
            },
            surge_value(&credential)
        ),
    ];
    if protocol == "trojan" || security == "tls" {
        parts.push("tls=true".into());
    }
    if !sni.is_empty() {
        parts.push(format!("sni={}", surge_value(&sni)));
    }
    if network == "ws" || network == "httpupgrade" {
        parts.push("ws=true".into());
        if network == "httpupgrade" {
            parts.push("ws-http-upgrade=true".into());
        }
        parts.push(format!("ws-path={}", surge_value(&path)));
        if !host.is_empty() {
            parts.push(format!(
                "ws-headers={}",
                surge_value(&format!("Host:{host}"))
            ));
        }
    }
    if network == "grpc" {
        parts.push("grpc=true".into());
        parts.push(format!(
            "grpc-service-name={}",
            surge_value(path.trim_start_matches('/'))
        ));
    }
    if matches!(network.as_str(), "xhttp" | "splithttp") {
        parts.push("xhttp=true".into());
        parts.push(format!("xhttp-path={}", surge_value(&path)));
        if !host.is_empty() {
            parts.push(format!("xhttp-host={}", surge_value(&host)));
        }
    }
    if !flow.is_empty() {
        parts.push(format!("flow={}", surge_value(&flow)));
    }
    if security == "reality" {
        if pbk.is_empty() {
            return Err(format!("{name} 的 REALITY 缺少公钥"));
        }
        parts.push(format!("reality-public-key={}", surge_value(&pbk)));
        if !sid.is_empty() {
            parts.push(format!("reality-short-id={}", surge_value(&sid)));
        }
    }
    Ok(format!("{} = {}", surge_value(&name), parts.join(", ")))
}

fn transform_node(
    node: &str,
    endpoint: &PreferredEndpoint,
    name: &str,
    keep_original_host: bool,
) -> Result<String, String> {
    if let Some(encoded) = node.strip_prefix("vmess://") {
        let bytes = general_purpose::STANDARD
            .decode(encoded)
            .or_else(|_| general_purpose::STANDARD_NO_PAD.decode(encoded))
            .map_err(|_| "无法解析 VMess Base64".to_string())?;
        let mut value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "VMess 内容不是有效 JSON".to_string())?;
        value["add"] = Value::String(endpoint.host.clone());
        if !keep_original_host {
            value["host"] = Value::String(endpoint.host.clone());
            value["sni"] = Value::String(endpoint.host.clone());
        }
        if let Some(port) = endpoint.port {
            value["port"] = Value::Number(port.into());
        }
        if !name.is_empty() {
            value["ps"] = Value::String(name.into());
        }
        return Ok(format!(
            "vmess://{}",
            general_purpose::STANDARD
                .encode(serde_json::to_vec(&value).map_err(|e| e.to_string())?)
        ));
    }
    if node.starts_with("vless://") || node.starts_with("trojan://") {
        let mut url = Url::parse(node).map_err(|_| "无法解析 VLESS/Trojan URI".to_string())?;
        url.set_host(Some(&endpoint.host))
            .map_err(|_| "无法设置优选地址".to_string())?;
        if !keep_original_host {
            let pairs: Vec<(String, String)> = url
                .query_pairs()
                .map(|(k, v)| {
                    let replace = matches!(k.as_ref(), "host" | "sni");
                    (
                        k.into_owned(),
                        if replace {
                            endpoint.host.clone()
                        } else {
                            v.into_owned()
                        },
                    )
                })
                .collect();
            url.query_pairs_mut().clear().extend_pairs(pairs);
        }
        if let Some(port) = endpoint.port {
            url.set_port(Some(port))
                .map_err(|_| "无法设置端口".to_string())?;
        }
        if !name.is_empty() {
            url.set_fragment(Some(name));
        }
        return Ok(url.to_string());
    }
    Err(format!("暂不支持的节点：{}", redact_node(node)))
}
fn redact_node(node: &str) -> String {
    node.split_once("://")
        .map(|(s, _)| format!("{s}://***"))
        .unwrap_or_else(|| "***".into())
}

fn bounded_preview_text(value: Option<&Value>, limit: usize) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .take(limit)
        .collect()
}

fn sanitize_publish_preview(value: &Value) -> Option<Value> {
    if let Some(link) = value.as_str() {
        return serde_json::to_value(preview_node(link)).ok();
    }
    let object = value.as_object()?;
    let first = |names: &[&str]| names.iter().find_map(|name| object.get(*name));
    let protocol = bounded_preview_text(first(&["protocol", "type"]), 24).to_uppercase();
    let name = bounded_preview_text(first(&["name", "remark", "ps"]), 160);
    let address = bounded_preview_text(first(&["address", "server", "host"]), 255);
    let port = object
        .get("port")
        .map(|port| {
            port.as_str()
                .map(ToString::to_string)
                .or_else(|| port.as_u64().map(|value| value.to_string()))
                .unwrap_or_default()
        })
        .unwrap_or_default()
        .chars()
        .take(6)
        .collect::<String>();
    if protocol.is_empty() && address.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "protocol": protocol,
        "name": name,
        "address": address,
        "port": port,
        "tls": bounded_preview_text(first(&["tls", "security"]), 32),
        "transport": bounded_preview_text(first(&["transport", "network", "net"]), 32),
        "host": bounded_preview_text(first(&["host"]), 255),
        "sni": bounded_preview_text(first(&["sni", "serverName", "servername"]), 255),
        "valid": true,
        "message": ""
    }))
}

fn normalize_publish_response(data: &Value) -> Result<Value, String> {
    let nested = data.get("data").unwrap_or(data);
    let explicit = nested.get("urls").and_then(Value::as_object);
    let common_keys = [
        "raw",
        "auto",
        "clash",
        "mihomo",
        "surge",
        "v2rayn",
        "v2rayng",
        "shadowrocket",
    ];
    let mut urls = serde_json::Map::new();
    if let Some(values) = explicit {
        if values.len() > 16 {
            return Err("发布服务返回的订阅格式超过 16 个".into());
        }
        for (name, value) in values {
            if let Some(value) = value.as_str() {
                urls.insert(name.clone(), Value::String(value.to_string()));
            }
        }
    } else if let Some(values) = nested.as_object() {
        for name in common_keys {
            if let Some(value) = values.get(name).and_then(Value::as_str) {
                urls.insert(name.into(), Value::String(value.to_string()));
            }
        }
    }
    if urls.is_empty() {
        return Err("发布服务响应缺少 urls 或已知订阅链接字段".into());
    }
    let allowed_schemes = [
        "https",
        "clash",
        "surge",
        "shadowrocket",
        "v2rayn",
        "v2rayng",
    ];
    for (name, value) in &urls {
        let value = value.as_str().unwrap_or_default();
        if value.len() > 4096 {
            return Err(format!("{name} 订阅链接超过 4096 字符"));
        }
        let url = Url::parse(value).map_err(|_| format!("{name} 不是有效订阅链接"))?;
        if !allowed_schemes.contains(&url.scheme()) {
            return Err(format!("{name} 使用不允许的链接协议：{}", url.scheme()));
        }
        if url.scheme() == "https" && url.host_str().is_none() {
            return Err(format!("{name} 的 HTTPS 链接缺少主机名"));
        }
    }
    let counts = nested
        .get("counts")
        .and_then(Value::as_object)
        .map(|values| {
            ["inputNodes", "preferredEndpoints", "outputNodes"]
                .into_iter()
                .filter_map(|name| {
                    values
                        .get(name)
                        .and_then(Value::as_u64)
                        .filter(|value| *value <= 100_000)
                        .map(|value| (name.to_string(), Value::Number(value.into())))
                })
                .collect::<serde_json::Map<String, Value>>()
        })
        .unwrap_or_default();
    let warnings = nested
        .get("warnings")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .take(50)
                .map(|value| Value::String(value.chars().take(500).collect()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let preview = nested
        .get("preview")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(sanitize_publish_preview)
                .take(20)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(
        serde_json::json!({ "urls": urls, "counts": counts, "warnings": warnings, "preview": preview }),
    )
}

#[tauri::command]
async fn publish_goodip(request: PublishRequest) -> Result<PublishResponse, String> {
    let base = request.base_url.trim_end_matches('/');
    let path = if request.generate_path.starts_with('/') {
        request.generate_path
    } else {
        format!("/{}", request.generate_path)
    };
    let endpoint =
        Url::parse(&format!("{base}{path}")).map_err(|_| "发布服务地址无效".to_string())?;
    if endpoint.scheme() != "https" {
        return Err("发布服务必须使用 HTTPS".into());
    }
    let resolved_publish_address = resolve_public_publish_address(&endpoint).await?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .resolve(
            endpoint
                .host_str()
                .ok_or_else(|| "发布服务地址缺少主机名".to_string())?,
            resolved_publish_address,
        )
        .build()
        .map_err(|e| format!("无法创建 HTTP 客户端：{e}"))?;
    let body = serde_json::json!({ "nodeLinks": request.node_links, "preferredIps": request.preferred_ips, "namePrefix": request.name_prefix, "keepOriginalHost": request.keep_original_host });
    let mut builder = client.post(endpoint).json(&body);
    if let Some(key) = request.api_key.filter(|v| !v.trim().is_empty()) {
        builder = builder.bearer_auth(key);
    }
    let response = builder
        .send()
        .await
        .map_err(|e| format!("发布服务连接失败：{e}"))?;
    let status = response.status();
    if status.is_redirection() {
        return Err("发布服务不允许重定向到其他地址".into());
    }
    if response
        .content_length()
        .is_some_and(|size| size > 1_048_576)
    {
        return Err("发布服务响应超过 1 MiB 限制".into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取发布响应失败：{e}"))?;
        if bytes.len().saturating_add(chunk.len()) > 1_048_576 {
            return Err("发布服务响应超过 1 MiB 限制".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let data: Value =
        serde_json::from_slice(&bytes).map_err(|_| "发布服务没有返回有效 JSON".to_string())?;
    if !status.is_success() || data.get("ok").and_then(Value::as_bool) == Some(false) {
        let message = data
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("发布服务拒绝了请求");
        return Err(format!("{message}（HTTP {status}）"));
    }
    Ok(PublishResponse {
        data: normalize_publish_response(&data)?,
    })
}

fn is_non_public_publish_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(value) => {
            value.is_private()
                || value.is_loopback()
                || value.is_link_local()
                || value.is_unspecified()
                || value.is_multicast()
                || value.octets()[0] == 0
                || (value.octets()[0] == 169 && value.octets()[1] == 254)
        }
        IpAddr::V6(value) => {
            value.is_loopback()
                || value.is_unspecified()
                || value.is_multicast()
                || value.is_unique_local()
                || value.is_unicast_link_local()
        }
    }
}

async fn resolve_public_publish_address(endpoint: &Url) -> Result<SocketAddr, String> {
    let host = endpoint
        .host_str()
        .ok_or_else(|| "发布服务地址缺少主机名".to_string())?;
    let port = endpoint
        .port_or_known_default()
        .ok_or_else(|| "发布服务地址缺少端口".to_string())?;
    let candidates = match endpoint.host() {
        Some(url::Host::Ipv4(value)) => vec![IpAddr::V4(value)],
        Some(url::Host::Ipv6(value)) => vec![IpAddr::V6(value)],
        Some(url::Host::Domain(_)) => tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| format!("发布服务域名解析失败：{e}"))?
            .map(|address| address.ip())
            .collect(),
        None => return Err("发布服务地址缺少主机名".into()),
    };
    let ip = candidates
        .into_iter()
        .find(|value| !is_non_public_publish_ip(*value))
        .ok_or_else(|| "发布服务域名解析到了本机或内网地址，已拒绝连接".to_string())?;
    Ok(SocketAddr::new(ip, port))
}

#[tauri::command]
async fn test_publish_connection(
    request: PublishConnectionRequest,
) -> Result<PublishConnectionResponse, String> {
    let base = request.base_url.trim_end_matches('/');
    let path = if request.generate_path.starts_with('/') {
        request.generate_path
    } else {
        format!("/{}", request.generate_path)
    };
    let endpoint =
        Url::parse(&format!("{base}{path}")).map_err(|_| "发布服务地址无效".to_string())?;
    if endpoint.scheme() != "https" {
        return Err("发布服务必须使用 HTTPS".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut builder = client.request(reqwest::Method::OPTIONS, endpoint.clone());
    if let Some(key) = request.api_key.filter(|v| !v.trim().is_empty()) {
        builder = builder.bearer_auth(key);
    }
    let response = builder
        .send()
        .await
        .map_err(|e| format!("无法连接发布服务：{e}"))?;
    let status = response.status();
    let note = if status.is_success() {
        "HTTPS 可达；服务接受 OPTIONS。真实响应合约仍会在生成时校验。"
    } else if matches!(status.as_u16(), 401 | 403) {
        "HTTPS 可达，但服务要求有效认证。"
    } else if status.as_u16() == 405 {
        "HTTPS 可达；服务仅允许生成请求。真实响应合约仍会在生成时校验。"
    } else {
        "HTTPS 可达，但接口返回非成功状态。"
    };
    Ok(PublishConnectionResponse {
        endpoint: endpoint.to_string(),
        status: status.as_u16(),
        reachable: true,
        note: note.into(),
    })
}

fn panel_root_url(panel_url: &str, operation: &str) -> Result<Url, String> {
    let mut url = Url::parse(panel_url.trim()).map_err(|_| "3X-UI 面板地址无效".to_string())?;
    if url.scheme() != "https" {
        return Err("3X-UI 面板必须使用 HTTPS".into());
    }
    let path = url.path().trim_end_matches('/');
    let root = path.strip_suffix("/panel").unwrap_or(path);
    url.set_path(&format!("{root}/{}", operation.trim_start_matches('/')));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn panel_session_key(panel_url: &str) -> Result<String, String> {
    let url = panel_root_url(panel_url, "panel/")?;
    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn login_cookie(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .find(|value| value.trim_start().starts_with("3x-ui="))
        .map(|value| value.trim().to_string())
}

fn panel_network_error(action: &str, error: &reqwest::Error) -> String {
    let kind = if error.is_timeout() {
        "连接超时"
    } else if error.is_connect() {
        "无法建立连接（请检查 Windows 系统代理、VPN、防火墙和 TLS 证书）"
    } else if error.is_request() {
        "请求无法发送"
    } else {
        "网络请求失败"
    };
    let mut details = Vec::new();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        let text = cause.to_string();
        if !text.is_empty() && !details.contains(&text) {
            details.push(text);
        }
        source = cause.source();
    }
    if details.is_empty() {
        format!("{action}：{kind}")
    } else {
        format!("{action}：{kind}（{}）", details.join("；"))
    }
}

#[tauri::command]
fn panel_session_status(
    state: State<'_, PanelSessionState>,
    panel_url: String,
) -> Result<bool, String> {
    let key = panel_session_key(&panel_url)?;
    Ok(state
        .cookies
        .lock()
        .map_err(|_| "3X-UI 会话状态不可用".to_string())?
        .contains_key(&key))
}

#[tauri::command]
fn forget_panel_session(
    state: State<'_, PanelSessionState>,
    panel_url: String,
) -> Result<(), String> {
    let key = panel_session_key(&panel_url)?;
    state
        .cookies
        .lock()
        .map_err(|_| "3X-UI 会话状态不可用".to_string())?
        .remove(&key);
    Ok(())
}

async fn perform_panel_login(
    state: &PanelSessionState,
    request: PanelLoginRequest,
) -> Result<PanelLoginResponse, String> {
    if request.username.trim().is_empty() || request.password.is_empty() {
        return Err("请填写 3X-UI 用户名和密码".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let root_response = client
        .get(panel_root_url(&request.panel_url, "")?)
        .header("X-Requested-With", "XMLHttpRequest")
        .send()
        .await
        .map_err(|e| panel_network_error("无法建立 3X-UI 登录会话", &e))?;
    let anonymous_cookie = login_cookie(root_response.headers())
        .ok_or_else(|| "3X-UI 没有返回登录会话 Cookie".to_string())?;
    let csrf_response = client
        .get(panel_root_url(&request.panel_url, "csrf-token")?)
        .header(reqwest::header::COOKIE, &anonymous_cookie)
        .header("X-Requested-With", "XMLHttpRequest")
        .send()
        .await
        .map_err(|e| panel_network_error("无法获取 3X-UI CSRF Token", &e))?;
    let csrf_status = csrf_response.status();
    let session_cookie =
        login_cookie(csrf_response.headers()).unwrap_or_else(|| anonymous_cookie.clone());
    let csrf_data = read_limited_json(csrf_response, 65_536).await?;
    let csrf_token = csrf_data
        .get("obj")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("3X-UI CSRF Token 获取失败（HTTP {csrf_status}）"))?;
    let two_factor_enabled = match client
        .post(panel_root_url(&request.panel_url, "getTwoFactorEnable")?)
        .header(reqwest::header::COOKIE, &session_cookie)
        .header("X-Requested-With", "XMLHttpRequest")
        .header("X-CSRF-Token", csrf_token)
        .send()
        .await
    {
        Ok(response) => response
            .json::<Value>()
            .await
            .ok()
            .and_then(|value| value.get("obj").and_then(Value::as_bool))
            .unwrap_or(false),
        Err(_) => false,
    };
    if two_factor_enabled && request.two_factor_code.trim().is_empty() {
        return Err("此面板已启用两步验证，请填写验证码".into());
    }
    let endpoint = panel_root_url(&request.panel_url, "login")?;
    let response = client
        .post(endpoint)
        .header(reqwest::header::COOKIE, &session_cookie)
        .header("X-Requested-With", "XMLHttpRequest")
        .header("X-CSRF-Token", csrf_token)
        .form(&[
            ("username", request.username.as_str()),
            ("password", request.password.as_str()),
            ("twoFactorCode", request.two_factor_code.trim()),
        ])
        .send()
        .await
        .map_err(|e| panel_network_error("3X-UI 登录连接失败", &e))?;
    let status = response.status();
    let cookie = login_cookie(response.headers());
    let data = read_limited_json(response, 65_536).await?;
    if !status.is_success() || data.get("success").and_then(Value::as_bool) != Some(true) {
        let message = data
            .get("msg")
            .and_then(Value::as_str)
            .unwrap_or("用户名、密码或验证码错误");
        return Err(format!("3X-UI 登录失败：{message}"));
    }
    let cookie = cookie.ok_or_else(|| "3X-UI 登录成功但没有返回会话 Cookie".to_string())?;
    let key = panel_session_key(&request.panel_url)?;
    state
        .cookies
        .lock()
        .map_err(|_| "3X-UI 会话状态不可用".to_string())?
        .insert(key, cookie);
    Ok(PanelLoginResponse {
        authenticated: true,
        two_factor_enabled,
    })
}

#[tauri::command]
async fn login_three_xui(
    state: State<'_, PanelSessionState>,
    request: PanelLoginRequest,
) -> Result<PanelLoginResponse, String> {
    perform_panel_login(&state, request).await
}

#[tauri::command]
async fn login_saved_three_xui(
    state: State<'_, PanelSessionState>,
    panel_url: String,
    two_factor_code: String,
) -> Result<PanelLoginResponse, String> {
    let saved = read_saved_panel_login(&panel_url)?
        .ok_or_else(|| "此面板没有保存的登录凭据".to_string())?;
    perform_panel_login(
        &state,
        PanelLoginRequest {
            panel_url,
            username: saved.username,
            password: saved.password,
            two_factor_code,
        },
    )
    .await
}

fn three_xui_api_url(panel_url: &str, operation: &str) -> Result<Url, String> {
    three_xui_resource_url(panel_url, "inbounds", operation)
}

fn three_xui_resource_url(panel_url: &str, resource: &str, operation: &str) -> Result<Url, String> {
    panel_root_url(
        panel_url,
        &format!("panel/api/{resource}/{}", operation.trim_start_matches('/')),
    )
}

fn three_xui_client_links_url(panel_url: &str, email: &str) -> Result<Url, String> {
    let mut url = three_xui_resource_url(panel_url, "clients", "links")?;
    url.path_segments_mut()
        .map_err(|_| "3X-UI 面板地址无法追加客户端路径".to_string())?
        .push(email);
    Ok(url)
}

fn extract_three_xui_links(data: &Value, filter: &str) -> Result<Vec<String>, String> {
    if data.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(data
            .get("msg")
            .and_then(Value::as_str)
            .unwrap_or("3X-UI 拒绝请求")
            .to_string());
    }
    let obj = data.get("obj").unwrap_or(data);
    let values = obj
        .as_array()
        .or_else(|| obj.get("links").and_then(Value::as_array))
        .ok_or_else(|| "3X-UI 响应不包含分享链接数组".to_string())?;
    let needle = filter.trim().to_lowercase();
    let links = values
        .iter()
        .filter_map(|value| {
            value
                .as_str()
                .or_else(|| value.get("link").and_then(Value::as_str))
                .or_else(|| value.get("url").and_then(Value::as_str))
        })
        .filter(|value| {
            matches!(
                value.split_once("://").map(|(protocol, _)| protocol),
                Some("vmess" | "vless" | "trojan")
            )
        })
        .filter(|value| {
            if needle.is_empty() {
                return true;
            }
            let preview = preview_node(value);
            [preview.name, preview.protocol, preview.address]
                .iter()
                .any(|field| field.to_lowercase().contains(&needle))
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    Ok(links)
}

fn extract_panel_clients(data: &Value) -> Result<Vec<PanelClientSummary>, String> {
    if data.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(data
            .get("msg")
            .and_then(Value::as_str)
            .unwrap_or("3X-UI 拒绝请求")
            .to_string());
    }
    let obj = data.get("obj").unwrap_or(data);
    let values = obj
        .as_array()
        .or_else(|| obj.get("items").and_then(Value::as_array))
        .or_else(|| obj.get("clients").and_then(Value::as_array))
        .ok_or_else(|| "3X-UI 响应不包含客户端列表".to_string())?;
    let mut clients = values
        .iter()
        .filter_map(|record| {
            let client = record.get("client").unwrap_or(record);
            let email = client.get("email")?.as_str()?.trim();
            if email.is_empty() {
                return None;
            }
            let inbound_ids = record
                .get("inboundIds")
                .and_then(Value::as_array)
                .map(|ids| ids.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default();
            Some(PanelClientSummary {
                email: email.to_string(),
                enabled: client
                    .get("enable")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                sub_id: client
                    .get("subId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                inbound_ids,
            })
        })
        .collect::<Vec<_>>();
    clients.sort_by_key(|a| a.email.to_lowercase());
    Ok(clients)
}

fn panel_auth(
    state: &PanelSessionState,
    panel_url: &str,
    api_token: &str,
) -> Result<(Option<String>, Option<String>), String> {
    let session_key = panel_session_key(panel_url)?;
    let session_cookie = state
        .cookies
        .lock()
        .map_err(|_| "3X-UI 会话状态不可用".to_string())?
        .get(&session_key)
        .cloned();
    let token = if !api_token.trim().is_empty() {
        Some(api_token.trim().to_string())
    } else if session_cookie.is_some() {
        None
    } else {
        match panel_keyring_entry(panel_url)?.get_password() {
            Ok(value) if !value.is_empty() => Some(value),
            _ => return Err("请先使用账号登录 3X-UI，或填写/保存 API Token".to_string()),
        }
    };
    Ok((token, session_cookie))
}

fn authenticate_panel_request(
    builder: reqwest::RequestBuilder,
    token: &Option<String>,
    session_cookie: &Option<String>,
) -> reqwest::RequestBuilder {
    let builder = builder.header("X-Requested-With", "XMLHttpRequest");
    if let Some(token) = token {
        builder.bearer_auth(token)
    } else if let Some(cookie) = session_cookie {
        builder.header(reqwest::header::COOKIE, cookie)
    } else {
        builder
    }
}

async fn read_limited_json(response: reqwest::Response, limit: usize) -> Result<Value, String> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(format!("3X-UI 响应超过 {} MiB 限制", limit / 1_048_576));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取 3X-UI 响应失败：{e}"))?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(format!("3X-UI 响应超过 {} MiB 限制", limit / 1_048_576));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "3X-UI 没有返回有效 JSON".to_string())
}

#[tauri::command]
async fn fetch_three_xui_links(
    state: State<'_, PanelSessionState>,
    request: ThreeXuiRequest,
) -> Result<ThreeXuiResponse, String> {
    let (token, session_cookie) = panel_auth(&state, &request.panel_url, &request.api_token)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let mut source = "allLinks".to_string();
    let mut endpoint = three_xui_api_url(&request.panel_url, "allLinks")?;
    let mut response =
        authenticate_panel_request(client.get(endpoint.clone()), &token, &session_cookie)
            .send()
            .await
            .map_err(|e| format!("3X-UI 连接失败：{e}"))?;
    if matches!(response.status().as_u16(), 404 | 405) {
        source = "list/slim（兼容模式）".to_string();
        endpoint = three_xui_api_url(&request.panel_url, "list/slim")?;
        response =
            authenticate_panel_request(client.get(endpoint.clone()), &token, &session_cookie)
                .send()
                .await
                .map_err(|e| format!("3X-UI 兼容接口连接失败：{e}"))?;
    }
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "3X-UI 返回 HTTP {status}（{endpoint}）；请检查面板路径、API Token 和版本"
        ));
    }
    let data = read_limited_json(response, 2_097_152).await?;
    let links = extract_three_xui_links(&data, &request.filter)?;
    let count = links.len();
    Ok(ThreeXuiResponse {
        links,
        count,
        source,
    })
}

#[tauri::command]
async fn list_three_xui_clients(
    state: State<'_, PanelSessionState>,
    request: PanelClientRequest,
) -> Result<Vec<PanelClientSummary>, String> {
    let (token, session_cookie) = panel_auth(&state, &request.panel_url, &request.api_token)?;
    let endpoint = three_xui_resource_url(&request.panel_url, "clients", "list")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let response =
        authenticate_panel_request(client.get(endpoint.clone()), &token, &session_cookie)
            .send()
            .await
            .map_err(|e| format!("3X-UI 客户端列表连接失败：{e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("3X-UI 客户端列表返回 HTTP {status}（{endpoint}）"));
    }
    extract_panel_clients(&read_limited_json(response, 2_097_152).await?)
}

#[tauri::command]
async fn fetch_three_xui_client_links(
    state: State<'_, PanelSessionState>,
    request: PanelClientRequest,
) -> Result<PanelClientLinksResponse, String> {
    let email = request
        .email
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "请先选择 3X-UI 客户端".to_string())?;
    let (token, session_cookie) = panel_auth(&state, &request.panel_url, &request.api_token)?;
    let endpoint = three_xui_client_links_url(&request.panel_url, email)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let response =
        authenticate_panel_request(client.get(endpoint.clone()), &token, &session_cookie)
            .send()
            .await
            .map_err(|e| format!("3X-UI 客户端节点连接失败：{e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("3X-UI 客户端节点返回 HTTP {status}（{endpoint}）"));
    }
    let links = extract_three_xui_links(&read_limited_json(response, 2_097_152).await?, "")?;
    Ok(PanelClientLinksResponse {
        email: email.to_string(),
        count: links.len(),
        links,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(RunnerState {
            running: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
        })
        .manage(PanelSessionState {
            cookies: Mutex::new(HashMap::new()),
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            validate_targets,
            run_native_speed_test,
            cancel_speed_test,
            list_history,
            clear_history,
            get_history_run,
            record_subscription_history,
            list_subscription_history,
            recover_unfinished_tasks,
            get_diagnostics,
            export_diagnostics_file,
            preview_nodes,
            generate_local,
            save_panel_token,
            save_panel_login,
            panel_login_status,
            delete_panel_login,
            panel_token_status,
            delete_panel_token,
            panel_session_status,
            login_three_xui,
            login_saved_three_xui,
            forget_panel_session,
            fetch_three_xui_links,
            list_three_xui_clients,
            fetch_three_xui_client_links,
            test_publish_connection,
            publish_goodip
        ])
        .run(tauri::generate_context!())
        .expect("error while running LinkForge");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[test]
    fn validates_ip_and_cidr() {
        let r = validate_targets("1.1.1.1\n2606:4700::/32\nbad\n1.1.1.1".into());
        assert_eq!(r.valid.len(), 2);
        assert_eq!(r.invalid.len(), 1);
        assert_eq!(r.duplicate_count, 1);
    }

    #[test]
    fn uses_cfst_default_url_only_when_http_download_is_needed() {
        assert_eq!(
            effective_test_url(None, false, false).as_deref(),
            Some(DEFAULT_CLOUDFLARE_SPEED_URL)
        );
        assert_eq!(
            effective_test_url(Some("  https://example.test/file  "), false, false).as_deref(),
            Some("https://example.test/file")
        );
        assert_eq!(
            effective_test_url(None, true, true).as_deref(),
            Some(DEFAULT_CLOUDFLARE_SPEED_URL)
        );
        assert_eq!(effective_test_url(None, true, false), None);
    }
    #[test]
    fn transforms_vless_and_preserves_sni() {
        let ep = PreferredEndpoint {
            host: "104.16.1.2".into(),
            port: None,
            label: "HK".into(),
        };
        let r = transform_node(
            "vless://test-id@example.com:443?security=tls&sni=origin.example.com#old",
            &ep,
            "CF-HK",
            true,
        )
        .unwrap();
        assert!(r.contains("104.16.1.2:443"));
        assert!(r.contains("sni=origin.example.com"));
        assert!(r.ends_with("#CF-HK"));
    }
    #[test]
    fn transforms_vmess_json() {
        let source = serde_json::json!({"v":"2","ps":"old","add":"origin.example.com","port":"443","id":"11111111-1111-1111-1111-111111111111","net":"ws","host":"origin.example.com","tls":"tls","sni":"origin.example.com"});
        let node = format!(
            "vmess://{}",
            general_purpose::STANDARD.encode(serde_json::to_vec(&source).unwrap())
        );
        let ep = PreferredEndpoint {
            host: "104.16.1.2".into(),
            port: Some(2053),
            label: "HK".into(),
        };
        let output = transform_node(&node, &ep, "CF-HK", true).unwrap();
        let decoded = general_purpose::STANDARD
            .decode(output.trim_start_matches("vmess://"))
            .unwrap();
        let value: Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(value["add"], "104.16.1.2");
        assert_eq!(value["port"], 2053);
        assert_eq!(value["host"], "origin.example.com");
        assert_eq!(value["sni"], "origin.example.com");
    }
    #[test]
    fn generates_one_ipv4_candidate_per_24() {
        let results = generate_candidates(&["104.16.0.0/23".into()]).unwrap();
        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .all(|ip| ip.to_string().starts_with("104.16."))
        );
    }
    #[test]
    fn expands_base64_subscription_and_redacts_preview() {
        let source = "vless://secret-id@example.com:443?security=tls&type=ws#Demo";
        let encoded = general_purpose::STANDARD.encode(source);
        let expanded = expand_node_input(&encoded);
        assert_eq!(expanded, vec![source]);
        let preview = preview_nodes(encoded);
        assert_eq!(preview[0].protocol, "VLESS");
        assert_eq!(preview[0].address, "example.com");
        assert!(
            !serde_json::to_string(&preview)
                .unwrap()
                .contains("secret-id")
        );
    }
    #[test]
    fn initializes_and_reads_sqlite_history() {
        let connection = Connection::open_in_memory().unwrap();
        init_history_schema(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO speed_runs VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    "run-1",
                    1_i64,
                    "example.com",
                    "IPv4",
                    10_i64,
                    2_i64,
                    25.0,
                    8.5,
                    "{}",
                    "[]"
                ],
            )
            .unwrap();
        let rows = query_history(&connection).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target, "example.com");
        assert_eq!(rows[0].result_count, 2);
        assert_eq!(rows[0].status, "completed");
    }
    #[test]
    fn recognizes_and_marks_interrupted_speed_tasks() {
        let connection = Connection::open_in_memory().unwrap();
        init_history_schema(&connection).unwrap();
        let request = SpeedTestRequest {
            targets: "104.16.0.0/24".into(),
            concurrency: 10,
            attempts: 2,
            download_count: 1,
            download_seconds: 1,
            port: 443,
            latency_min: 0.0,
            latency_max: 250.0,
            packet_loss_max: 0.2,
            speed_min: 0.0,
            disable_download: false,
            test_url: Some("https://speed.example.com/file.bin".into()),
            httping: false,
        };
        insert_task_start(&connection, "task-1", &request).unwrap();
        let recovered = recover_tasks(&connection).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].target, "speed.example.com");
        let status: String = connection
            .query_row(
                "SELECT status FROM task_runs WHERE run_id='task-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "interrupted");
        let rows = query_history(&connection).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "interrupted");
        assert_eq!(rows[0].result_count, 0);
        assert!(recover_tasks(&connection).unwrap().is_empty());
    }
    #[test]
    fn builds_three_xui_url_with_custom_root_path() {
        let url = three_xui_api_url("https://example.com/secret-root/panel/", "allLinks").unwrap();
        assert_eq!(
            url.as_str(),
            "https://example.com/secret-root/panel/api/inbounds/allLinks"
        );
        assert!(three_xui_api_url("http://example.com/panel", "allLinks").is_err());
    }
    #[test]
    fn builds_three_xui_login_url_and_extracts_only_session_cookie() {
        let url = panel_root_url("https://example.com:8443/secret-root/panel/", "login").unwrap();
        assert_eq!(url.as_str(), "https://example.com:8443/secret-root/login");
        let mut headers = reqwest::header::HeaderMap::new();
        headers.append(
            reqwest::header::SET_COOKIE,
            "theme=dark; Path=/".parse().unwrap(),
        );
        headers.append(
            reqwest::header::SET_COOKIE,
            "3x-ui=session-value; Path=/secret-root/; HttpOnly; Secure"
                .parse()
                .unwrap(),
        );
        assert_eq!(
            login_cookie(&headers).as_deref(),
            Some("3x-ui=session-value")
        );
    }
    #[test]
    fn extracts_and_filters_three_xui_links() {
        let data = serde_json::json!({
            "success": true,
            "obj": [
                "vless://id@example.com:443?security=tls#Alice-HK",
                "trojan://password@example.net:443?security=tls#Bob-US",
                "ss://ignored"
            ]
        });
        let links = extract_three_xui_links(&data, "alice").unwrap();
        assert_eq!(links.len(), 1);
        assert!(links[0].contains("Alice-HK"));
    }
    #[test]
    fn accepts_three_xui_object_link_shape() {
        let data = serde_json::json!({
            "success": true,
            "obj": {"links": [{"link": "vmess://invalid-but-recognized"}]}
        });
        let links = extract_three_xui_links(&data, "").unwrap();
        assert_eq!(links, vec!["vmess://invalid-but-recognized"]);
    }

    #[test]
    fn extracts_current_and_legacy_client_list_shapes() {
        let current = serde_json::json!({
            "success": true,
            "obj": [{
                "client": {"email":"mt", "enable":true, "subId":"sub-mt"},
                "inboundIds": [7, 9]
            }]
        });
        let clients = extract_panel_clients(&current).unwrap();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].email, "mt");
        assert_eq!(clients[0].inbound_ids, vec![7, 9]);

        let legacy = serde_json::json!({
            "success": true,
            "obj": {"clients":[{"email":"vless", "enable":false}]}
        });
        let clients = extract_panel_clients(&legacy).unwrap();
        assert_eq!(clients[0].email, "vless");
        assert!(!clients[0].enabled);
    }

    #[test]
    fn saved_panel_login_round_trips_without_exposing_fields() {
        let saved = SavedPanelLogin {
            username: "admin".into(),
            password: "not-written-to-app-settings".into(),
        };
        let encoded = serde_json::to_string(&saved).unwrap();
        let decoded: SavedPanelLogin = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.username, "admin");
        assert_eq!(decoded.password, "not-written-to-app-settings");
    }
    #[test]
    fn exports_vless_ws_tls_to_mihomo_without_losing_host_or_sni() {
        let link = "vless://test-id@104.16.1.2:443?security=tls&type=ws&host=origin.example.com&sni=origin.example.com&path=%2Fedge#CF-HK";
        let yaml = link_to_mihomo(link).unwrap();
        assert!(yaml.contains("type: vless"));
        assert!(yaml.contains("server: \"104.16.1.2\""));
        assert!(yaml.contains("servername: \"origin.example.com\""));
        assert!(yaml.contains("Host: \"origin.example.com\""));
        assert!(yaml.contains("path: \"/edge\""));
    }
    #[test]
    fn exports_vless_reality_to_surge() {
        let link = "vless://test-id@104.16.1.2:443?security=reality&type=tcp&sni=origin.example.com&flow=xtls-rprx-vision&pbk=public-key&sid=abcd#CF-HK";
        let profile = link_to_surge(link).unwrap();
        assert!(profile.starts_with("CF-HK = vless, 104.16.1.2, 443"));
        assert!(profile.contains("sni=origin.example.com"));
        assert!(profile.contains("flow=xtls-rprx-vision"));
        assert!(profile.contains("reality-public-key=public-key"));
        assert!(profile.contains("reality-short-id=abcd"));
    }
    #[test]
    fn warns_when_surge_cannot_preserve_transport() {
        let link = "trojan://secret@104.16.1.2:443?security=tls&type=grpc&serviceName=edge#CF-HK";
        let (_, warnings) = generate_surge_profile(&[link.to_string()]);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("gRPC"));
    }
    #[test]
    fn local_generation_reports_safe_address_changes_and_missing_sni() {
        let result = generate_local(LocalGenerateRequest {
            node_links: "vless://test-id@origin.example.com:443?security=tls&type=ws#Demo".into(),
            preferred_ips: "104.16.1.2#HK".into(),
            name_prefix: "CF".into(),
            keep_original_host: true,
        })
        .unwrap();
        assert_eq!(result.changes.len(), 1);
        assert_eq!(result.changes[0].original_address, "origin.example.com");
        assert_eq!(result.changes[0].preferred_address, "104.16.1.2");
        assert!(result.changes[0].host_sni_preserved);
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("缺少 SNI"))
        );
    }
    #[test]
    fn normalizes_supported_publish_response_shapes() {
        let direct = serde_json::json!({
            "ok": true,
            "urls": {
                "raw": "https://subscriptions.example.com/raw/abc",
                "clash": "clash://install-config?url=https%3A%2F%2Fsubscriptions.example.com%2Fclash%2Fabc"
            },
            "counts": {"inputNodes": 1, "preferredEndpoints": 2, "outputNodes": 2},
            "warnings": ["demo warning"],
            "preview": [
                "vless://secret-uuid@104.16.1.2:443?security=tls&type=ws&sni=origin.example.com#CF-1",
                {"type":"vmess","name":"CF-2","server":"104.16.1.3","port":443,"uuid":"must-not-leak"}
            ]
        });
        let normalized = normalize_publish_response(&direct).unwrap();
        assert_eq!(
            normalized["urls"]["raw"],
            "https://subscriptions.example.com/raw/abc"
        );
        assert_eq!(normalized["counts"]["outputNodes"], 2);
        assert_eq!(normalized["warnings"][0], "demo warning");
        assert_eq!(normalized["preview"].as_array().unwrap().len(), 2);
        let serialized = serde_json::to_string(&normalized["preview"]).unwrap();
        assert!(!serialized.contains("secret-uuid"));
        assert!(!serialized.contains("must-not-leak"));
        assert!(serialized.contains("104.16.1.2"));
        let wrapped = serde_json::json!({
            "data": {"surge": "surge:///install-config?url=https%3A%2F%2Fsubscriptions.example.com%2Fsurge%2Fabc"}
        });
        assert!(normalize_publish_response(&wrapped).unwrap()["urls"]["surge"].is_string());
    }
    #[test]
    fn rejects_unsafe_or_incomplete_publish_response() {
        assert!(normalize_publish_response(&serde_json::json!({"ok": true})).is_err());
        assert!(
            normalize_publish_response(&serde_json::json!({
                "urls": {"raw": "javascript:alert(1)"}
            }))
            .is_err()
        );
    }
    #[test]
    fn subscription_history_keeps_only_safe_summary_fields() {
        let connection = Connection::open_in_memory().unwrap();
        init_history_schema(&connection).unwrap();
        let record = sanitize_subscription_history(SubscriptionHistoryRequest {
            mode: "online".into(),
            input_nodes: 2,
            preferred_endpoints: 10,
            output_nodes: 20,
            formats: vec![" Clash ".into(), "Surge".into(), "".into()],
            endpoint_host: Some("https://goodip.example/api/generate?token=secret".into()),
            status: "completed".into(),
            error: Some("line\nbreak".into()),
        })
        .unwrap();
        assert_eq!(record.endpoint_host, "goodip.example");
        assert_eq!(record.formats, vec!["Clash", "Surge"]);
        assert_eq!(record.error, "linebreak");
        insert_subscription_history(&connection, &record).unwrap();
        let rows = query_subscription_history(&connection).unwrap();
        assert_eq!(rows.len(), 1);
        let serialized = serde_json::to_string(&rows[0]).unwrap();
        assert!(!serialized.contains("/api/generate"));
        assert!(!serialized.contains('\n'));
    }
    #[test]
    fn clearing_history_removes_speed_task_and_subscription_rows_atomically() {
        let mut connection = Connection::open_in_memory().unwrap();
        init_history_schema(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO speed_runs(run_id,created_at,target,ip_mode,tested_count,result_count,best_latency,best_speed,request_json,results_json) VALUES('speed',1,'TCP only','IPv4',1,1,20,1,'{}','[]')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO task_runs(run_id,started_at,status,target,ip_mode) VALUES('task',1,'failed','TCP only','IPv4')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO subscription_runs(run_id,created_at,mode,input_nodes,preferred_endpoints,output_nodes,formats_json,endpoint_host,status,error) VALUES('sub',1,'local',1,1,1,'[]','','completed','')",
                [],
            )
            .unwrap();
        clear_history_tables(&mut connection).unwrap();
        assert_eq!(query_history(&connection).unwrap().len(), 0);
        assert_eq!(query_subscription_history(&connection).unwrap().len(), 0);
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM task_runs", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn classifies_non_public_publish_addresses() {
        assert!(is_non_public_publish_ip("127.0.0.1".parse().unwrap()));
        assert!(is_non_public_publish_ip("10.0.0.8".parse().unwrap()));
        assert!(is_non_public_publish_ip("fc00::1".parse().unwrap()));
        assert!(!is_non_public_publish_ip("1.1.1.1".parse().unwrap()));
    }
    #[tokio::test]
    async fn rejects_private_publish_endpoint_before_connecting() {
        let endpoint = Url::parse("https://127.0.0.1/api/generate").unwrap();
        assert!(resolve_public_publish_address(&endpoint).await.is_err());
    }
    #[tokio::test]
    async fn measures_local_http_download_and_extracts_colo() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await.unwrap();
            let body = vec![b'x'; 65_536];
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nCF-Ray: test-HKG\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(header.as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
        });
        let url = format!("http://localhost:{port}/speed.bin");
        let cancelled = AtomicBool::new(false);
        let (speed, colo) =
            measure_download("127.0.0.1".parse().unwrap(), &url, port, 1, &cancelled)
                .await
                .unwrap();
        server.await.unwrap();
        assert!(speed > 0.0);
        assert_eq!(colo, "HKG");
    }
    #[tokio::test]
    async fn follows_redirects_while_pinning_each_host_to_candidate_ip() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            for index in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 1024];
                let size = socket.read(&mut request).await.unwrap();
                let request = String::from_utf8_lossy(&request[..size]);
                if index == 0 {
                    assert!(request.starts_with("GET /start"));
                    let location = format!("http://redirect.test:{port}/final");
                    let header = format!(
                        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    socket.write_all(header.as_bytes()).await.unwrap();
                } else {
                    assert!(request.starts_with("GET /final"));
                    let body = vec![b'y'; 32_768];
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    socket.write_all(header.as_bytes()).await.unwrap();
                    socket.write_all(&body).await.unwrap();
                }
            }
        });
        let cancelled = AtomicBool::new(false);
        let (speed, _) = measure_download(
            "127.0.0.1".parse().unwrap(),
            &format!("http://localhost:{port}/start"),
            port,
            1,
            &cancelled,
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert!(speed > 0.0);
    }
    #[tokio::test]
    async fn cancels_a_stalled_download_promptly() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 999999\r\n\r\n")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = cancelled.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            cancellation.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        let result = measure_download(
            "127.0.0.1".parse().unwrap(),
            &format!("http://localhost:{port}/stall.bin"),
            port,
            30,
            &cancelled,
        )
        .await;
        assert_eq!(result.unwrap_err(), "测速已取消");
        assert!(started.elapsed() < Duration::from_secs(1));
        server.abort();
    }
}
