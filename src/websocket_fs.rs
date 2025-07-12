use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use uuid::Uuid;

#[derive(Debug)]
pub enum FileError {
    WebSocketRequest(Box<dyn std::error::Error>),
    RemoteFileNotFound(String),
    NoFileDescriptor,
    ReadFailed(String),
    CacheWriteFailed(std::io::Error),
    CacheReadFailed(std::io::Error),
    RemoteError(String),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FileError::WebSocketRequest(e) => write!(f, "WebSocket request failed: {}", e),
            FileError::RemoteFileNotFound(path) => write!(f, "Remote file not found: {}", path),
            FileError::NoFileDescriptor => {
                write!(f, "Remote server did not return a file descriptor")
            }
            FileError::ReadFailed(reason) => write!(f, "Failed to read remote file: {}", reason),
            FileError::CacheWriteFailed(e) => write!(f, "Failed to write to cache: {}", e),
            FileError::CacheReadFailed(e) => write!(f, "Failed to read from cache: {}", e),
            FileError::RemoteError(msg) => write!(f, "Remote server error: {}", msg),
        }
    }
}

impl std::error::Error for FileError {}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation")]
pub enum FSRequest {
    #[serde(rename = "read")]
    Read {
        id: String,
        path: String,
        size: usize,
        offset: usize,
    },
    #[serde(rename = "write")]
    Write {
        id: String,
        path: String,
        offset: usize,
        data: Vec<u8>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FSResponse {
    pub id: String,
    pub success: bool,
    pub fd: Option<i32>,
    pub bytes_read: Option<usize>,
    pub bytes_written: Option<usize>,
    pub position: Option<i64>,
    pub error: Option<String>,
}

pub struct FSResponseWithBinary {
    pub response: FSResponse,
    pub binary: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct CachedFile {
    pub position: usize,
    pub path: String,
}

pub struct WebSocketFileSystem {
    ws_sender: Option<mpsc::UnboundedSender<Message>>,
    open_files: HashMap<i32, CachedFile>,
    pending_requests:
        Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<FSResponseWithBinary>>>>,
}

impl WebSocketFileSystem {
    pub fn new(cache_dir: String) -> Self {
        std::fs::create_dir_all(&cache_dir).expect("Failed to create cache directory");

        Self {
            ws_sender: None,
            open_files: HashMap::new(),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn start_server(&mut self, port: u16) -> Result<(), Box<dyn std::error::Error>> {
        let addr = format!("127.0.0.1:{}", port);
        let listener = TcpListener::bind(&addr).await?;
        println!("WebSocket server listening on {}", addr);

        let (stream, _) = listener.accept().await?;
        println!("WebSocket client connected");

        let ws_stream = accept_async(stream).await?;
        let (ws_sender, mut ws_receiver) = ws_stream.split();

        let (tx, mut rx) = mpsc::unbounded_channel();
        self.ws_sender = Some(tx);

        let pending_requests = Arc::clone(&self.pending_requests);

        // Spawn WebSocket message handler
        tokio::spawn(async move {
            let mut ws_sender = ws_sender;

            // Handle outgoing messages
            let outgoing_task = tokio::spawn(async move {
                while let Some(message) = rx.recv().await {
                    if ws_sender.send(message).await.is_err() {
                        break;
                    }
                }
            });

            // Handle incoming messages
            let incoming_task = tokio::spawn(async move {
                while let Some(msg) = ws_receiver.next().await {
                    match msg {
                        Ok(Message::Binary(data)) => {
                            // Parse unified binary message: [json_len(4 bytes)][json][binary_data]
                            if data.len() < 4 {
                                eprintln!("WebSocket binary message too short: {} bytes, expected at least 4 for JSON length header", data.len());
                                continue;
                            }

                            let json_len =
                                u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;

                            if data.len() < 4 + json_len {
                                eprintln!("WebSocket binary message too short: {} bytes, expected {} bytes for JSON length {}", data.len(), 4 + json_len, json_len);
                                continue;
                            }

                            let json_bytes = &data[4..4 + json_len];
                            let binary_data = &data[4 + json_len..];

                            let json_str = match std::str::from_utf8(json_bytes) {
                                Ok(s) => s,
                                Err(e) => {
                                    eprintln!(
                                        "WebSocket message contains invalid UTF-8 JSON: {}",
                                        e
                                    );
                                    continue;
                                }
                            };

                            let response = match serde_json::from_str::<FSResponse>(json_str) {
                                Ok(r) => r,
                                Err(e) => {
                                    eprintln!(
                                        "WebSocket message contains invalid JSON: {} - JSON: {}",
                                        e, json_str
                                    );
                                    continue;
                                }
                            };

                            let response_id = response.id.clone();
                            let mut response_with_binary = FSResponseWithBinary {
                                response,
                                binary: None,
                            };
                            let mut pending = pending_requests.lock().await;

                            // Handle binary data if present
                            if !binary_data.is_empty() {
                                response_with_binary.binary = Some(binary_data.to_vec());
                            }

                            match pending.remove(&response_id) {
                                Some(sender) => {
                                    if let Err(_) = sender.send(response_with_binary) {
                                        eprintln!("Failed to send response to waiting request (receiver dropped): {}", response_id);
                                    }
                                }
                                None => {
                                    eprintln!(
                                        "Received WebSocket response for unknown request ID: {}",
                                        response_id
                                    );
                                }
                            }
                        }
                        Ok(Message::Close(_)) => break,
                        Err(e) => {
                            eprintln!("WebSocket error: {}", e);
                            break;
                        }
                        _ => {}
                    }
                }
            });

            // Wait for either task to complete
            tokio::select! {
                _ = outgoing_task => {},
                _ = incoming_task => {},
            }
        });

        Ok(())
    }

    pub async fn send_request(
        &self,
        request: FSRequest,
    ) -> Result<FSResponseWithBinary, Box<dyn std::error::Error>> {
        println!("Sending request: {:?}", request);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let request_id = request.get_id().to_string();

        // Store the response channel
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(request_id.clone(), tx);
        }

        // Send the request as unified binary message
        if let Some(sender) = &self.ws_sender {
            let json_str = serde_json::to_string(&request)?;
            let json_bytes = json_str.as_bytes();
            let json_len = json_bytes.len() as u32;

            let mut message_data = Vec::with_capacity(4 + json_bytes.len());
            message_data.extend_from_slice(&json_len.to_le_bytes());
            message_data.extend_from_slice(json_bytes);

            let message = Message::Binary(message_data);
            sender.send(message)?;
        } else {
            return Err("WebSocket not connected".into());
        }

        // Wait for response
        let response = rx.await?;
        Ok(response)
    }

    pub async fn send_request_with_binary(
        &self,
        request: FSRequest,
        data: &[u8],
    ) -> Result<FSResponseWithBinary, Box<dyn std::error::Error>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let request_id = request.get_id().to_string();

        // Store the response channel
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(request_id.clone(), tx);
        }

        // Send the request with binary data as unified binary message
        if let Some(sender) = &self.ws_sender {
            let json_str = serde_json::to_string(&request)?;
            let json_bytes = json_str.as_bytes();
            let json_len = json_bytes.len() as u32;

            let mut message_data = Vec::with_capacity(4 + json_bytes.len() + data.len());
            message_data.extend_from_slice(&json_len.to_le_bytes());
            message_data.extend_from_slice(json_bytes);
            message_data.extend_from_slice(data);

            let message = Message::Binary(message_data);
            sender.send(message)?;
        } else {
            return Err("WebSocket not connected".into());
        }

        // Wait for response
        let response = rx.await?;
        Ok(response)
    }

    pub fn register_fd(&mut self, fd: i32, path: &str) {
        self.open_files.insert(
            fd,
            CachedFile {
                path: path.to_string(),
                position: 0,
            },
        );
    }
    pub fn update_fd_position(&mut self, fd: i32, position: usize) {
        if let Some(file) = self.open_files.get_mut(&fd) {
            file.position = position;
        }
    }

    pub async fn open_file(&mut self, path: &str) -> Result<(), FileError> {
        // Check if file exists already
        println!("Checking if file exists: {}", path);
        if !Path::new(&path).exists() {
            println!("File does not exist, reading from Deno: {}", path);
            // Read entire file from Deno
            let read_id = Uuid::new_v4().to_string();
            let read_request = FSRequest::Read {
                id: read_id.clone(),
                path: path.to_string(),
                size: 1024 * 1024, // Read up to 1MB
                offset: 0,
            };

            let read_response = self
                .send_request(read_request)
                .await
                .map_err(FileError::WebSocketRequest)?;

            println!("Read response: {:?}", read_response.response);

            if !read_response.response.success {
                let error_msg = read_response
                    .response
                    .error
                    .unwrap_or_else(|| "Unknown read error".to_string());
                return Err(FileError::ReadFailed(error_msg));
            }

            if read_response.response.bytes_read.unwrap_or(0) > 0 {
                let file_data = read_response
                    .binary
                    .ok_or_else(|| FileError::ReadFailed("No binary data received".to_string()))?;

                if let Some(parent_dir) = Path::new(&path).parent() {
                    std::fs::create_dir_all(parent_dir).map_err(FileError::CacheWriteFailed)?;
                }
                std::fs::write(path, &file_data).map_err(FileError::CacheWriteFailed)?;

                println!("Cached file {} ({} bytes)", path, file_data.len());
            }
        }

        Ok(())
    }

    pub async fn write_file(&mut self, path: &str, data: &[u8]) -> Option<usize> {
        // Write-through to Deno first
        let write_request = FSRequest::Write {
            id: Uuid::new_v4().to_string(),
            path: path.to_string(),
            offset: 0,
            data: data.to_vec(),
        };

        // Send request with binary data
        match self.send_request_with_binary(write_request, data).await {
            Ok(response) if response.response.success => {
                // Update cache file on disk
                if let Ok(()) = std::fs::write(path, data) {
                    Some(data.len())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn close_file(&mut self, fd: i32) -> bool {
        self.open_files.remove(&fd).is_some()
    }
}

impl FSRequest {
    fn get_id(&self) -> &str {
        match self {
            FSRequest::Read { id, .. } => id,
            FSRequest::Write { id, .. } => id,
        }
    }
}
