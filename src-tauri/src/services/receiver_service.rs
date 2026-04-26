use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
};

use crate::{
    db::repositories::SharedMemoryRepository,
    errors::app_error::{AppError, AppResult},
    models::{MemoryInput, MemorySourceType},
    services::{
        link_enrichment_service::LinkEnrichmentService, memory_service::MemoryService,
        pairing_service::PairingService,
    },
};

const DEFAULT_RECEIVER_PORT: u16 = 47653;
const PORT_FALLBACK_COUNT: u16 = 20;
const MAX_REQUEST_BYTES: usize = 512 * 1024;

#[derive(Clone)]
pub struct DesktopReceiverService {
    pairing_service: Arc<PairingService>,
    memory_service: Arc<MemoryService>,
    memory_repository: SharedMemoryRepository,
    link_enrichment_service: Arc<LinkEnrichmentService>,
    running: Arc<AtomicBool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IncomingPushPayload {
    memory: IncomingMemoryPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IncomingMemoryPayload {
    id: String,
    title: Option<String>,
    content: Option<String>,
    url: Option<String>,
    source: Option<String>,
    note: Option<String>,
    memory_type: Option<String>,
    created_at: i64,
    image_uri: Option<String>,
    preview_text: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PushMemoryResponse {
    ok: bool,
    memory_id: Option<String>,
    duplicate: bool,
    message: String,
}

impl DesktopReceiverService {
    pub fn new(
        pairing_service: Arc<PairingService>,
        memory_service: Arc<MemoryService>,
        memory_repository: SharedMemoryRepository,
        link_enrichment_service: Arc<LinkEnrichmentService>,
    ) -> Self {
        Self {
            pairing_service,
            memory_service,
            memory_repository,
            link_enrichment_service,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn start(&self, app: AppHandle) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }

        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = service.run(app).await {
                service.running.store(false, Ordering::SeqCst);
                eprintln!("[recall][receiver] failed to start: {error}");
            }
        });
    }

    async fn run(&self, app: AppHandle) -> AppResult<()> {
        let bind_ip = detect_lan_ip()
            .await
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let (listener, port) = bind_with_fallback(bind_ip).await?;
        let endpoint = format!("http://{}:{}", bind_ip, port);
        self.pairing_service
            .set_endpoint(endpoint.clone(), port)
            .await?;
        let _ = app.emit(
            "recall://pairing-updated",
            &self.pairing_service.info(true).await?,
        );
        debug_receiver_log(format!("listening endpoint={endpoint}"));

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            if !is_local_network_peer(peer_addr.ip(), bind_ip) {
                debug_receiver_log(format!(
                    "rejected peer outside local network peer={peer_addr}"
                ));
                continue;
            }

            let service = self.clone();
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = service.handle_connection(app, stream).await {
                    debug_receiver_log(format!("connection error={error}"));
                }
            });
        }
    }

    async fn handle_connection(&self, app: AppHandle, mut stream: TcpStream) -> AppResult<()> {
        let request = read_http_request(&mut stream).await?;
        let response = self.route_request(app, request).await;
        write_http_response(&mut stream, response).await?;
        Ok(())
    }

    async fn route_request(&self, app: AppHandle, request: HttpRequest) -> HttpResponse {
        if request.method == "OPTIONS" {
            return json_response(204, json!({ "ok": true }));
        }

        if request.path != "/api/ping" && request.path != "/api/push-memory" {
            return json_response(404, json!({ "ok": false, "error": "not_found" }));
        }

        match self.is_authorized(&request).await {
            Ok(true) => {}
            Ok(false) => {
                return json_response(401, json!({ "ok": false, "error": "unauthorized" }));
            }
            Err(error) => {
                return json_response(
                    500,
                    json!({ "ok": false, "error": "pairing_unavailable", "message": error.to_string() }),
                );
            }
        }

        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/api/ping") => match self.pairing_service.info(self.is_running()).await {
                Ok(info) => json_response(
                    200,
                    json!({
                        "ok": true,
                        "deviceId": info.device_id,
                        "desktopName": info.desktop_name,
                        "endpoint": info.endpoint,
                    }),
                ),
                Err(error) => json_response(
                    500,
                    json!({ "ok": false, "error": "pairing_unavailable", "message": error.to_string() }),
                ),
            },
            ("POST", "/api/push-memory") => match self.ingest_push(app, &request.body).await {
                Ok(response) => json_response(200, response),
                Err(error) => {
                    let status = if matches!(error, AppError::Invalid(_)) {
                        400
                    } else {
                        500
                    };
                    json_response(
                        status,
                        json!({ "ok": false, "error": "push_failed", "message": error.to_string() }),
                    )
                }
            },
            _ => json_response(405, json!({ "ok": false, "error": "method_not_allowed" })),
        }
    }

    async fn is_authorized(&self, request: &HttpRequest) -> AppResult<bool> {
        let expected = self.pairing_service.current_secret().await?;
        let Some(value) = request.header("authorization") else {
            return Ok(false);
        };
        Ok(value.trim() == format!("Bearer {expected}"))
    }

    async fn ingest_push(&self, app: AppHandle, body: &[u8]) -> AppResult<PushMemoryResponse> {
        let payload = serde_json::from_slice::<IncomingPushPayload>(body)?;
        let input = validate_incoming_memory(payload.memory)?;
        if let Some(existing) = self
            .memory_repository
            .find_by_external_source("mobile", input.external_id.as_deref().unwrap_or_default())
            .await?
        {
            return Ok(PushMemoryResponse {
                ok: true,
                memory_id: Some(existing.id),
                duplicate: true,
                message: "Memory already received.".into(),
            });
        }

        let memory = self.memory_service.create(input).await?;
        app.emit("recall://memory-saved", &memory)?;
        self.link_enrichment_service
            .schedule_for_memory(app, memory.clone())
            .await;

        Ok(PushMemoryResponse {
            ok: true,
            memory_id: Some(memory.id),
            duplicate: false,
            message: "Memory received.".into(),
        })
    }
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl HttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

async fn read_http_request(stream: &mut TcpStream) -> AppResult<HttpRequest> {
    let mut buffer = Vec::new();
    let mut temp = [0_u8; 4096];
    let mut header_end = None;

    while buffer.len() < MAX_REQUEST_BYTES {
        let read = stream.read(&mut temp).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&temp[..read]);
        if let Some(index) = find_header_end(&buffer) {
            header_end = Some(index);
            break;
        }
    }

    let header_end =
        header_end.ok_or_else(|| AppError::Invalid("Malformed HTTP request.".into()))?;
    let header_text = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| AppError::Invalid("Missing request line.".into()))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| AppError::Invalid("Missing HTTP method.".into()))?
        .to_ascii_uppercase();
    let path = request_parts
        .next()
        .ok_or_else(|| AppError::Invalid("Missing HTTP path.".into()))?
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();

    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > MAX_REQUEST_BYTES {
        return Err(AppError::Invalid("Request body is too large.".into()));
    }

    let body_start = header_end + 4;
    let mut body = buffer.get(body_start..).unwrap_or_default().to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut temp).await?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&temp[..read]);
        if body.len() > MAX_REQUEST_BYTES {
            return Err(AppError::Invalid("Request body is too large.".into()));
        }
    }
    body.truncate(content_length);

    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

async fn write_http_response(stream: &mut TcpStream, response: HttpResponse) -> AppResult<()> {
    let reason = match response.status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        _ => "Internal Server Error",
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Authorization, Content-Type\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nConnection: close\r\n\r\n",
        response.status,
        reason,
        response.body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&response.body).await?;
    stream.shutdown().await?;
    Ok(())
}

fn json_response<T: Serialize>(status: u16, value: T) -> HttpResponse {
    HttpResponse {
        status,
        body: serde_json::to_vec(&value).unwrap_or_else(|_| b"{\"ok\":false}".to_vec()),
    }
}

async fn bind_with_fallback(bind_ip: IpAddr) -> AppResult<(TcpListener, u16)> {
    for port in DEFAULT_RECEIVER_PORT..=(DEFAULT_RECEIVER_PORT + PORT_FALLBACK_COUNT) {
        let addr = SocketAddr::new(bind_ip, port);
        match TcpListener::bind(addr).await {
            Ok(listener) => return Ok((listener, port)),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(error) => return Err(error.into()),
        }
    }

    Err(AppError::Invalid(
        "No available Recall receiver port.".into(),
    ))
}

async fn detect_lan_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").await.ok()?;
    socket.connect("8.8.8.8:80").await.ok()?;
    let ip = socket.local_addr().ok()?.ip();
    if is_private_or_link_local(ip) {
        Some(ip)
    } else {
        None
    }
}

fn is_private_or_link_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_private() || ip.is_link_local() || ip == Ipv4Addr::LOCALHOST,
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
    }
}

fn is_local_network_peer(peer_ip: IpAddr, bind_ip: IpAddr) -> bool {
    if peer_ip.is_loopback() {
        return true;
    }

    match (peer_ip, bind_ip) {
        (IpAddr::V4(peer), IpAddr::V4(bind)) => {
            if !is_private_or_link_local(IpAddr::V4(peer)) {
                return false;
            }
            let peer = peer.octets();
            let bind = bind.octets();
            peer[0] == bind[0] && peer[1] == bind[1]
        }
        (IpAddr::V6(peer), IpAddr::V6(_)) => peer.is_unique_local() || peer.is_unicast_link_local(),
        _ => false,
    }
}

fn validate_incoming_memory(memory: IncomingMemoryPayload) -> AppResult<MemoryInput> {
    let external_id = normalize_required(memory.id, "memory.id", 128)?;
    let title = normalize_optional(memory.title, 180);
    let content = normalize_optional_body(memory.content, 64_000);
    let url = normalize_optional(memory.url, 2048);
    let note = normalize_optional_body(memory.note, 16_000);
    let preview_text = normalize_optional_body(memory.preview_text, 16_000);
    let source = normalize_optional(memory.source, 120);
    let memory_type = normalize_optional(memory.memory_type, 120);
    let image_uri = normalize_optional(memory.image_uri, 2048);

    let body = content
        .or_else(|| url.clone())
        .or_else(|| preview_text.clone())
        .ok_or_else(|| {
            AppError::Invalid("Incoming memory must include content, url, or previewText.".into())
        })?;

    let note = match (note, preview_text, image_uri, memory_type) {
        (note, preview, image, kind) => {
            let mut parts = Vec::new();
            if let Some(note) = note {
                parts.push(note);
            }
            if let Some(preview) = preview.filter(|preview| preview != &body) {
                parts.push(format!("Preview: {preview}"));
            }
            if let Some(kind) = kind {
                parts.push(format!("Mobile type: {kind}"));
            }
            if let Some(image) = image {
                parts.push(format!("Image: {image}"));
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n\n"))
            }
        }
    };

    Ok(MemoryInput {
        source_type: Some(MemorySourceType::Manual),
        title,
        content: body,
        note,
        project_id: None,
        url,
        external_id: Some(external_id),
        folder_path: None,
        source_app: Some("mobile".into()),
        source_window: source,
        created_at: Some(timestamp_to_rfc3339(memory.created_at)?),
        updated_at: None,
    })
}

fn timestamp_to_rfc3339(value: i64) -> AppResult<String> {
    if value <= 0 {
        return Err(AppError::Invalid(
            "memory.createdAt must be positive.".into(),
        ));
    }

    let datetime = if value > 1_000_000_000_000 {
        Utc.timestamp_millis_opt(value).single()
    } else {
        Utc.timestamp_opt(value, 0).single()
    }
    .ok_or_else(|| AppError::Invalid("memory.createdAt is not a valid timestamp.".into()))?;

    Ok(datetime.to_rfc3339())
}

fn normalize_required(value: String, field: &str, max_chars: usize) -> AppResult<String> {
    normalize_optional(Some(value), max_chars)
        .ok_or_else(|| AppError::Invalid(format!("{field} is required.")))
}

fn normalize_optional(value: Option<String>, max_chars: usize) -> Option<String> {
    value.and_then(|value| {
        let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            None
        } else {
            Some(truncate_chars(&normalized, max_chars))
        }
    })
}

fn normalize_optional_body(value: Option<String>, max_chars: usize) -> Option<String> {
    value.and_then(|value| {
        let normalized = value
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        if normalized.is_empty() {
            None
        } else {
            Some(truncate_chars(&normalized, max_chars))
        }
    })
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut output = value.chars().take(max_chars).collect::<String>();
    output.push_str("...");
    output
}

fn debug_receiver_log(message: String) {
    if cfg!(debug_assertions) {
        eprintln!("[recall][receiver] {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::{timestamp_to_rfc3339, validate_incoming_memory, IncomingMemoryPayload};

    #[test]
    fn incoming_memory_requires_content_or_url() {
        let result = validate_incoming_memory(IncomingMemoryPayload {
            id: "mobile-1".into(),
            title: None,
            content: None,
            url: None,
            source: None,
            note: None,
            memory_type: None,
            created_at: 1_776_000_000_000,
            image_uri: None,
            preview_text: None,
        });

        assert!(result.is_err());
    }

    #[test]
    fn incoming_memory_maps_to_capture_input() {
        let input = validate_incoming_memory(IncomingMemoryPayload {
            id: " mobile-1 ".into(),
            title: Some(" Saved from phone ".into()),
            content: None,
            url: Some(" https://example.com/article ".into()),
            source: Some(" iPhone ".into()),
            note: Some(" Read later ".into()),
            memory_type: Some("article".into()),
            created_at: 1_776_000_000_000,
            image_uri: None,
            preview_text: Some("Useful page summary".into()),
        })
        .expect("valid memory");

        assert_eq!(input.external_id.as_deref(), Some("mobile-1"));
        assert_eq!(input.source_app.as_deref(), Some("mobile"));
        assert_eq!(input.source_window.as_deref(), Some("iPhone"));
        assert_eq!(input.content, "https://example.com/article");
        assert_eq!(input.url.as_deref(), Some("https://example.com/article"));
        assert!(input.note.unwrap_or_default().contains("Read later"));
    }

    #[test]
    fn timestamp_accepts_millis_and_seconds() {
        assert!(timestamp_to_rfc3339(1_776_000_000_000)
            .unwrap()
            .starts_with("2026"));
        assert!(timestamp_to_rfc3339(1_776_000_000)
            .unwrap()
            .starts_with("2026"));
    }
}
