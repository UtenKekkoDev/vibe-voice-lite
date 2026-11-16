use crate::task::TaskControl;
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose};
use serde_json::{Value, json};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tungstenite::Message;
use tungstenite::client::IntoClientRequest;

const MAX_APPEND_BYTES: usize = 512 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Clone)]
pub struct QwenAsrConfig {
    pub api_key: String,
    pub model: String,
    pub url: String,
    pub language: String,
}

#[derive(Debug, Clone)]
pub struct QwenAsrTranscript {
    pub text: String,
}

pub fn validate_config(config: &QwenAsrConfig) -> Result<()> {
    let api_key = normalize_dashscope_api_key(&config.api_key);
    if api_key.is_empty() {
        bail!("请先填写 DashScope API Key。");
    }
    if api_key
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        bail!("DashScope API Key 不能包含空格或换行，请重新粘贴纯 API Key。");
    }
    if config.model.trim().is_empty() {
        bail!("请先填写 Qwen-ASR 模型名。");
    }
    if config.url.trim().is_empty() {
        bail!("请先填写 Qwen-ASR WebSocket 地址。");
    }
    if !config.url.trim().starts_with("wss://") {
        bail!("Qwen-ASR WebSocket 地址需要以 wss:// 开头。");
    }
    if !config
        .model
        .trim()
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        bail!("模型名仅支持字母、数字、连字符、点和下划线。");
    }
    Ok(())
}

pub fn probe_connection(config: &QwenAsrConfig) -> Result<()> {
    validate_config(config)?;

    probe_connection_inner(config, &TaskControl::new(PROBE_TIMEOUT))
}

pub fn transcribe_audio(
    config: &QwenAsrConfig,
    samples: Vec<f32>,
    task: &TaskControl,
) -> Result<QwenAsrTranscript> {
    validate_config(config)?;
    if samples.is_empty() {
        bail!("录音音频为空。");
    }

    let pcm = samples_to_pcm16_le(&samples);
    let mut socket = connect_qwen_socket(config, task)?;
    send_session_update(&mut socket, config)?;

    for chunk in pcm.chunks(MAX_APPEND_BYTES) {
        task.check()?;
        send_json(
            &mut socket,
            json!({
                "event_id": event_id("audio"),
                "type": "input_audio_buffer.append",
                "audio": general_purpose::STANDARD.encode(chunk)
            }),
        )?;
    }

    send_json(
        &mut socket,
        json!({
            "event_id": event_id("commit"),
            "type": "input_audio_buffer.commit"
        }),
    )?;
    send_json(
        &mut socket,
        json!({
            "event_id": event_id("finish"),
            "type": "session.finish"
        }),
    )?;

    let mut final_text = String::new();
    loop {
        let message = read_message(&mut socket, task)?;
        let Message::Text(text) = message else {
            continue;
        };
        let payload: Value = serde_json::from_str(&text)
            .with_context(|| format!("解析 Qwen-ASR 响应失败: {text}"))?;
        let event_type = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match event_type {
            "error" => bail!(format_qwen_error(&payload)),
            "conversation.item.input_audio_transcription.completed" => {
                if let Some(transcript) = payload.get("transcript").and_then(Value::as_str) {
                    final_text = transcript.trim().to_string();
                }
            }
            "conversation.item.input_audio_transcription.failed" => {
                bail!(format_qwen_error(&payload));
            }
            "session.finished" => break,
            _ => {}
        }
    }

    let _ = socket.close(None);
    if final_text.is_empty() {
        bail!("Qwen-ASR 没有返回最终识别文本。");
    }

    Ok(QwenAsrTranscript { text: final_text })
}

fn probe_connection_inner(config: &QwenAsrConfig, task: &TaskControl) -> Result<()> {
    let mut socket = connect_qwen_socket(config, task)?;
    send_session_update(&mut socket, config)?;

    loop {
        let message = read_message(&mut socket, task)?;
        let Message::Text(text) = message else {
            continue;
        };
        let payload: Value = serde_json::from_str(&text)
            .with_context(|| format!("解析 Qwen-ASR 配置检查响应失败: {text}"))?;
        let event_type = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match event_type {
            "error" => bail!(format_qwen_error(&payload)),
            "session.updated" => {
                let _ = socket.close(None);
                return Ok(());
            }
            _ => {}
        }
    }
}

fn connect_qwen_socket(
    config: &QwenAsrConfig,
    task: &TaskControl,
) -> Result<tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>> {
    let base_url = config.url.trim().trim_end_matches('/');
    let separator = if base_url.contains('?') { "&" } else { "?" };
    let endpoint = format!("{base_url}{separator}model={}", config.model.trim());
    let api_key = normalize_dashscope_api_key(&config.api_key);
    let mut request = endpoint
        .as_str()
        .into_client_request()
        .with_context(|| format!("创建 Qwen-ASR WebSocket 请求失败: {endpoint}"))?;
    request.headers_mut().insert(
        "Authorization",
        format!("bearer {api_key}")
            .parse()
            .context("生成 DashScope Authorization header 失败")?,
    );

    task.check()?;
    let host = request
        .uri()
        .host()
        .context("WebSocket 地址缺少主机名")?
        .to_owned();
    let port = request.uri().port_u16().unwrap_or(443);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(
            (host.as_str(), port)
                .to_socket_addrs()
                .map(|a| a.collect::<Vec<_>>()),
        );
    });
    let addresses = loop {
        task.check()?;
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => break result.context("解析服务器地址失败")?,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => bail!("地址解析任务已退出"),
        }
    };
    let mut connected = None;
    for address in addresses {
        task.check()?;
        if let Ok(stream) =
            TcpStream::connect_timeout(&address, task.remaining()?.min(Duration::from_secs(3)))
        {
            connected = Some(stream);
            break;
        }
    }
    let stream = connected.context("连接服务器失败，请检查网络")?;
    let handshake_timeout = task.remaining()?.min(Duration::from_secs(5));
    stream.set_read_timeout(Some(handshake_timeout))?;
    stream.set_write_timeout(Some(handshake_timeout))?;
    let (socket, _) = tungstenite::client_tls(request, stream).context("WebSocket/TLS 握手失败")?;
    match socket.get_ref() {
        tungstenite::stream::MaybeTlsStream::Plain(s) => {
            s.set_read_timeout(Some(Duration::from_millis(250)))?
        }
        tungstenite::stream::MaybeTlsStream::NativeTls(s) => s
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(250)))?,
        _ => {}
    }
    task.check()?;
    Ok(socket)
}

fn send_session_update(
    socket: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    config: &QwenAsrConfig,
) -> Result<()> {
    send_json(
        socket,
        json!({
            "event_id": event_id("session"),
            "type": "session.update",
            "session": {
                "input_audio_format": "pcm",
                "sample_rate": 16000,
                "input_audio_transcription": {
                    "language": config.language.trim()
                },
                "turn_detection": null
            }
        }),
    )
}

fn normalize_dashscope_api_key(input: &str) -> String {
    let trimmed = input.trim();
    trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

fn send_json(
    socket: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    payload: Value,
) -> Result<()> {
    socket
        .send(Message::Text(payload.to_string().into()))
        .context("发送 Qwen-ASR WebSocket 事件失败")
}

fn samples_to_pcm16_le(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn format_qwen_error(payload: &Value) -> String {
    let error = payload.get("error").unwrap_or(payload);
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("未知错误");
    let param = error.get("param").and_then(Value::as_str).unwrap_or("");
    if param.is_empty() {
        format!("Qwen-ASR 返回错误: {code}: {message}")
    } else {
        format!("Qwen-ASR 返回错误: {code}: {message} ({param})")
    }
}

fn event_id(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{prefix}_{millis}")
}

fn read_message(
    socket: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
    task: &TaskControl,
) -> Result<Message> {
    loop {
        task.check()?;
        match socket.read() {
            Ok(Message::Close(_)) => bail!("服务端提前关闭了连接，请重试。"),
            Ok(message) => return Ok(message),
            Err(tungstenite::Error::Io(ref e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error).context("读取在线识别响应失败"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn silent_server() -> (
        tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let _ = ws.read(); // Client drops the connection after timeout/cancellation.
        });
        let stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (socket, _) = tungstenite::client(
            format!("ws://{address}"),
            tungstenite::stream::MaybeTlsStream::Plain(stream),
        )
        .unwrap();
        if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream
                .set_read_timeout(Some(Duration::from_millis(15)))
                .unwrap();
        }
        (socket, worker)
    }

    #[test]
    fn silent_server_respects_overall_deadline() {
        let (mut socket, worker) = silent_server();
        let task = TaskControl::new(Duration::from_millis(60));
        assert!(
            read_message(&mut socket, &task)
                .unwrap_err()
                .to_string()
                .contains("超时")
        );
        drop(socket);
        worker.join().unwrap();
    }

    #[test]
    fn cancellation_interrupts_wait_for_server() {
        let (mut socket, worker) = silent_server();
        let task = TaskControl::new(Duration::from_secs(30));
        let token = task.clone();
        let cancel = thread::spawn(move || {
            thread::sleep(Duration::from_millis(40));
            token.cancel();
        });
        assert!(
            read_message(&mut socket, &task)
                .unwrap_err()
                .to_string()
                .contains("取消")
        );
        drop(socket);
        cancel.join().unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn pcm_conversion_clips_without_overflow() {
        let bytes = samples_to_pcm16_le(&[-2.0, 0.0, 2.0]);
        let samples: Vec<i16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(samples, [-32767, 0, 32767]);
    }
}
