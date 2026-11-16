use crate::audio::{RecordingCapture, resample_to_target, start_recording, stop_recording};
use crate::qwen_asr::{self, QwenAsrConfig};
use crate::sensevoice;
use crate::state::{InputMode, InputState, ModelProvider, NativeAppState};
use crate::task::TaskControl;
use anyhow::{Context, Result, anyhow};
use arboard::Clipboard;
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsWindow,
};
fn delivery_target_slot() -> &'static Mutex<Option<isize>> {
    static SLOT: OnceLock<Mutex<Option<isize>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}
#[derive(Clone)]
pub struct FinishInputSnapshot {
    clarification: Option<crate::ai::Clarification>,
    ai: crate::ai::AiConfig,
    input_mode: InputMode,
    auto_paste: bool,
    model_provider: ModelProvider,
    local_model_dir: String,
    local_model_ready: bool,
    local_model_status: String,
    local_model_summary: String,
    qwen_api_key: String,
    qwen_model: String,
    qwen_url: String,
    qwen_language: String,
    qwen_ready: bool,
    qwen_status: String,
}

pub struct FinishInputWork {
    capture: RecordingCapture,
    last_recording_info: String,
    snapshot: FinishInputSnapshot,
}

pub struct FinishInputResult {
    pub clarification: Option<crate::ai::Clarification>,
    pub review_required: bool,
    pub ai_note: Option<String>,
    pub input_state: InputState,
    pub status_message: String,
    pub raw_text: Option<String>,
    pub delivered_text: Option<String>,
    pub local_model_ready: bool,
    pub local_model_status: String,
    pub local_model_summary: String,
    pub qwen_ready: bool,
    pub qwen_status: String,
    pub last_recording_info: String,
    pub auto_paste: bool,
}

pub fn begin_input(state: &mut NativeAppState) {
    if matches!(
        state.input_state,
        InputState::Processing | InputState::Recording
    ) {
        return;
    }
    if !state.model_ready() {
        state.status_message = "请先配置并检查识别模型。".into();
        return;
    }
    // Each explicit hold selects the current editor, including follow-up answers.
    remember_delivery_target_window();
    match start_recording() {
        Ok(info) => {
            state.input_state = InputState::Recording;
            state.last_recording_info = info;

            state.status_message = "录音中；再次按快捷键或点击麦克风结束。单次最长 5 分钟。".into();
        }
        Err(error) => {
            state.input_state = InputState::Error;
            state.status_message = format!("启动录音失败: {error:#}");
            state.last_recording_info = "录音没有启动。".to_string();
        }
    }
}

pub fn begin_finish_input(state: &mut NativeAppState) -> Result<FinishInputWork> {
    if state.input_state != InputState::Recording {
        return Err(anyhow!("当前没有正在进行的口述。"));
    }

    state.input_state = InputState::Processing;
    state.status_message = "正在准备识别音频…".to_string();

    let capture = match stop_recording() {
        Ok(capture) => capture,
        Err(error) => {
            return Err(error);
        }
    };

    let last_recording_info = format!(
        "录音完成，原始采样率 {} Hz，通道 {}，时长约 {:.2} 秒。",
        capture.sample_rate, capture.channels, capture.duration_secs
    );
    state.last_recording_info = last_recording_info.clone();

    if capture.samples.is_empty() {
        return Err(anyhow!("录到的音频为空，请再试一次。"));
    }
    Ok(FinishInputWork {
        capture,
        last_recording_info,
        snapshot: FinishInputSnapshot {
            clarification: state.clarification.clone(),
            ai: state.ai.clone(),
            input_mode: state
                .clarification
                .as_ref()
                .map_or(state.input_mode, |c| c.mode),
            auto_paste: state.auto_paste,
            model_provider: state.model_provider,
            local_model_dir: state.local_model_dir.clone(),
            local_model_ready: state.local_model_ready,
            local_model_status: state.local_model_status.clone(),
            local_model_summary: state.local_model_summary.clone(),
            qwen_api_key: state.qwen_api_key.clone(),
            qwen_model: state.qwen_model.clone(),
            qwen_url: state.qwen_url.clone(),
            qwen_language: state.qwen_language.clone(),
            qwen_ready: state.qwen_ready,
            qwen_status: state.qwen_status.clone(),
        },
    })
}

pub fn run_finish_input_work(work: FinishInputWork, task: &TaskControl) -> FinishInputResult {
    let FinishInputWork {
        capture,
        last_recording_info,
        snapshot,
    } = work;

    let mut local_model_ready = snapshot.local_model_ready;
    let mut local_model_status = snapshot.local_model_status.clone();
    let mut local_model_summary = snapshot.local_model_summary.clone();
    let mut qwen_ready = snapshot.qwen_ready;
    let mut qwen_status = snapshot.qwen_status.clone();

    let result = (|| -> Result<(String, String, usize, String)> {
        task.check()?;
        let normalized_samples =
            resample_to_target(capture.samples, capture.sample_rate, capture.channels);
        task.check()?;
        if snapshot.model_provider == ModelProvider::OnlineQwen {
            let config = qwen_config_from_snapshot(&snapshot);
            qwen_asr::validate_config(&config)?;
            qwen_ready = true;
            qwen_status = format!("Qwen-ASR Realtime 已配置: {}", config.model);
            let transcript = qwen_asr::transcribe_audio(&config, normalized_samples, task)?;
            let raw_text = normalize_text(&transcript.text);
            let delivered_text = raw_text.clone();
            task.check()?;
            let delivery_message = "识别结果已保留，可编辑或复制。".to_owned();
            return Ok((raw_text, delivered_text, 1, delivery_message));
        }

        if !local_model_ready {
            let probe =
                sensevoice::prepare_and_probe(std::path::Path::new(&snapshot.local_model_dir))?;
            local_model_ready = true;
            local_model_status = "SenseVoice 已可被 Rust provider 加载。".to_string();
            local_model_summary = sensevoice::format_probe_summary(&probe);
        }

        let transcript = sensevoice::transcribe_audio(
            std::path::Path::new(&snapshot.local_model_dir),
            normalized_samples,
            task,
        )?;
        let raw_text = normalize_text(&transcript.text);
        if raw_text.is_empty() {
            return Err(anyhow!("未识别到有效语音，请检查麦克风后重试。"));
        }
        let delivered_text = raw_text.clone();
        task.check()?;
        let delivery_message = "识别结果已保留，可编辑或复制。".to_owned();
        Ok((
            raw_text,
            delivered_text,
            transcript.segment_count,
            delivery_message,
        ))
    })();

    match result {
        Ok((raw_text, mut delivered_text, segments, delivery_message)) => {
            let mut clarification = None;
            let mut review_required = false;
            let mut cancelled = false;
            let ai_note = if snapshot.ai.active_for(snapshot.input_mode)
                || snapshot.clarification.is_some()
            {
                match crate::ai::polish(
                    &snapshot.ai,
                    snapshot.input_mode,
                    &raw_text,
                    task,
                    snapshot.clarification.as_ref(),
                ) {
                    Ok(polished) => {
                        cancelled = polished.cancelled;
                        delivered_text = polished.text;
                        if let Some(question) = polished.question {
                            review_required = true;
                            clarification = Some(next_clarification(
                                snapshot.clarification.as_ref(),
                                &raw_text,
                                snapshot.input_mode,
                                question,
                            ));
                            delivered_text.clear();
                        }
                        Some(
                            if cancelled {
                                "已取消整理"
                            } else if review_required {
                                "AI 需要确认"
                            } else {
                                "AI 已整理"
                            }
                            .to_string(),
                        )
                    }
                    Err(error) => {
                        // A failed/malformed AI response never approves an answer.
                        review_required = true;
                        if let Some(c) = &snapshot.clarification {
                            clarification = Some(c.continue_with(&raw_text, c.question.clone()));
                            delivered_text.clear();
                        } else if snapshot.input_mode == InputMode::Interview {
                            clarification = Some(next_clarification(
                                None,
                                &raw_text,
                                InputMode::Interview,
                                "请继续说明你希望实现的目标。".into(),
                            ));
                            delivered_text.clear();
                        } else {
                            delivered_text = raw_text.clone();
                        }
                        Some(format!("AI 整理失败，原文已保留，未自动粘贴：{error}"))
                    }
                }
            } else {
                None
            };
            let engine_label = match snapshot.model_provider {
                ModelProvider::Local => "SenseVoice",
                ModelProvider::OnlineQwen => "Qwen-ASR",
            };
            FinishInputResult {
                clarification,
                review_required,
                ai_note,
                input_state: InputState::Success,
                status_message: format!(
                    "{engine_label} 转写完成，已识别 {} 个字符，切出 {} 段。{}",
                    raw_text.chars().count(),
                    segments,
                    delivery_message
                ),
                raw_text: Some(raw_text),
                delivered_text: Some(delivered_text),
                local_model_ready,
                local_model_status,
                local_model_summary,
                qwen_ready,
                qwen_status,
                last_recording_info,
                auto_paste: snapshot.auto_paste && !cancelled,
            }
        }
        Err(error) => {
            let engine_label = match snapshot.model_provider {
                ModelProvider::Local => "SenseVoice",
                ModelProvider::OnlineQwen => "Qwen-ASR",
            };
            FinishInputResult {
                clarification: None,
                review_required: snapshot.clarification.is_some(),
                ai_note: None,
                input_state: InputState::Error,
                status_message: format!("{engine_label} 转写失败: {error:#}"),
                raw_text: None,
                delivered_text: None,
                local_model_ready,
                local_model_status,
                local_model_summary,
                qwen_ready,
                qwen_status,
                last_recording_info,
                auto_paste: snapshot.auto_paste,
            }
        }
    }
}

fn next_clarification(
    previous: Option<&crate::ai::Clarification>,
    answer: &str,
    mode: InputMode,
    question: String,
) -> crate::ai::Clarification {
    match previous {
        Some(c) => c.continue_with(answer, question),
        None => crate::ai::Clarification {
            original: answer.to_owned(),
            question,
            mode,
            answers: Vec::new(),
        },
    }
}

pub fn check_local_model(state: &mut NativeAppState) {
    state.local_model_ready = false;
    state.status_message = "正在检查 SenseVoice 本地模型...".to_string();

    match sensevoice::prepare_and_probe(std::path::Path::new(&state.local_model_dir)) {
        Ok(probe) => {
            state.local_model_ready = true;
            state.local_model_status =
                "SenseVoice 已可被 Rust provider 加载。可以开始口述。".to_string();
            state.local_model_summary = sensevoice::format_probe_summary(&probe);
            state.status_message =
                "SenseVoice 本地模型检查通过，已生成兼容词表并完成加载探测。".to_string();
        }
        Err(error) => {
            state.local_model_status = format!("SenseVoice 检查失败: {error:#}");
            state.local_model_summary = "官方模型目录需要 model.int8.onnx（或 model.onnx）和 tokens.txt；旧 FunASR 导出需转换为兼容格式。".to_string();
            state.input_state = InputState::Error;
            state.status_message =
                "SenseVoice 本地模型暂时不可用，请先看下方错误信息。".to_string();
        }
    }
}

pub fn check_online_model(state: &mut NativeAppState) {
    let config = QwenAsrConfig {
        api_key: state.qwen_api_key.clone(),
        model: state.qwen_model.clone(),
        url: state.qwen_url.clone(),
        language: state.qwen_language.clone(),
    };

    match qwen_asr::probe_connection(&config) {
        Ok(()) => {
            state.qwen_ready = true;
            state.qwen_status = format!("Qwen-ASR Realtime 连接检查通过: {}", config.model);
            state.status_message =
                "在线模型连接检查通过，录音结束后将调用 Qwen-ASR Realtime。".to_string();
        }
        Err(error) => {
            state.qwen_ready = false;
            state.qwen_status = format!("Qwen-ASR 连接检查失败: {error:#}");
            state.input_state = InputState::Error;
            state.status_message =
                "在线模型暂时不可用，请检查 API Key、模型名和 WebSocket 地址。".to_string();
        }
    }
}

fn qwen_config_from_snapshot(snapshot: &FinishInputSnapshot) -> QwenAsrConfig {
    QwenAsrConfig {
        api_key: snapshot.qwen_api_key.clone(),
        model: snapshot.qwen_model.clone(),
        url: snapshot.qwen_url.clone(),
        language: snapshot.qwen_language.clone(),
    }
}

pub fn auto_paste_result(result: &FinishInputResult) -> Result<()> {
    if !result.review_required && result.clarification.is_none() && result.auto_paste {
        let text = result
            .delivered_text
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .context("没有可粘贴的最终结果")?;
        paste_without_refocusing(
            text,
            ensure_delivery_target_active,
            write_clipboard,
            simulate_paste_shortcut,
        )?;
    }
    Ok(())
}

fn paste_without_refocusing(
    text: &str,
    mut check: impl FnMut() -> Result<()>,
    copy: impl FnOnce(&str) -> Result<()>,
    send: impl FnOnce() -> Result<()>,
) -> Result<()> {
    // Preserve a manual-paste fallback even when the user changed windows.
    copy(text)?;
    check()?;
    send()
}

// Commit recognition before attempting delivery, so a clipboard/focus error cannot lose speech.
pub fn apply_finish_result(
    state: &mut NativeAppState,
    result: FinishInputResult,
    deliver: impl FnOnce(&FinishInputResult) -> Result<()>,
) -> Option<String> {
    if result.input_state == InputState::Success {
        state.clarification = result.clarification.clone();
    }
    state.ai_status = result.ai_note.clone().unwrap_or_default();
    if let Some(text) = &result.raw_text {
        state.raw_text = text.clone();
    }
    if let Some(text) = &result.delivered_text {
        state.delivered_text = text.clone();
    }
    let paste_error = if result.input_state == InputState::Success
        && !result.review_required
        && result.clarification.is_none()
    {
        deliver(&result).err()
    } else {
        None
    };
    state.input_state = result.input_state;
    state.status_message = result.status_message;
    state.local_model_ready = result.local_model_ready;
    state.local_model_status = result.local_model_status;
    state.local_model_summary = result.local_model_summary;
    state.qwen_ready = result.qwen_ready;
    state.qwen_status = result.qwen_status;
    state.last_recording_info = result.last_recording_info;
    if let Some(error) = paste_error {
        state.status_message = format!("识别成功，粘贴未完成：{error}。结果已保留，可复制或重试。");
        Some(state.status_message.clone())
    } else {
        None
    }
}

/// Automatic TTS is only for an actual AI clarification question.
pub fn spoken_question(result: &FinishInputResult) -> Option<String> {
    if result.input_state != InputState::Success
        || !result.review_required
        || result.ai_note.as_deref() != Some("AI 需要确认")
    {
        return None;
    }
    result
        .clarification
        .as_ref()
        .map(|c| c.question.trim())
        .filter(|q| !q.is_empty())
        .map(str::to_owned)
}

fn write_clipboard(text: &str) -> Result<()> {
    let mut clipboard = Clipboard::new().context("打开系统剪贴板失败。")?;
    clipboard
        .set_text(text.to_string())
        .context("写入系统剪贴板失败。")?;
    Ok(())
}

fn simulate_paste_shortcut() -> Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, INPUT, SendInput, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    // Do not release physical keys belonging to the user or send Ctrl+Shift+V.
    if [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
        .iter()
        .any(|&key| unsafe { GetAsyncKeyState(key as i32) < 0 })
    {
        anyhow::bail!("修饰键仍按下；结果已复制，请松开后按 Ctrl+V");
    }
    let keys = paste_inputs();
    let sent = unsafe {
        SendInput(
            keys.len() as u32,
            keys.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent != keys.len() as u32 {
        // Release only keys this batch could have pressed.
        if sent > 0 {
            unsafe {
                SendInput(2, keys[2..].as_ptr(), std::mem::size_of::<INPUT>() as i32);
            }
        }
        anyhow::bail!(
            "粘贴快捷键未完整送达（{sent}/4），请检查目标软件权限；结果已复制，可按 Ctrl+V"
        );
    }
    crate::config::lifecycle("paste shortcut sent");
    Ok(())
}

fn paste_inputs() -> [windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT; 4] {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_LCONTROL, VK_V,
    };
    // Virtual key V avoids character-to-scan-code translation through an IME.
    [
        (VK_LCONTROL, 0),
        (VK_V, 0),
        (VK_V, KEYEVENTF_KEYUP),
        (VK_LCONTROL, KEYEVENTF_KEYUP),
    ]
    .map(|(key, flags)| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    })
}

fn normalize_text(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn remember_delivery_target_window() {
    #[cfg(target_os = "windows")]
    {
        let hwnd = unsafe { GetForegroundWindow() };
        let mut pid = 0;
        if !hwnd.is_null() {
            unsafe {
                GetWindowThreadProcessId(hwnd, &mut pid);
            }
        }
        if let Ok(mut guard) = delivery_target_slot().lock() {
            *guard = new_delivery_target(hwnd as isize, pid, std::process::id());
        }
    }
}

fn new_delivery_target(hwnd: isize, pid: u32, own_pid: u32) -> Option<isize> {
    (hwnd != 0 && pid != 0 && pid != own_pid).then_some(hwnd)
}

fn ensure_delivery_target_active() -> Result<()> {
    let target = delivery_target_slot().lock().ok().and_then(|guard| *guard);
    let foreground = unsafe { GetForegroundWindow() } as isize;
    if !target.is_some_and(|hwnd| hwnd == foreground && unsafe { IsWindow(hwnd as _) } != 0) {
        crate::config::lifecycle("paste skipped: target missing or changed");
        return Err(anyhow!(
            "目标未选择或已切换窗口；结果已复制，请在输入框按 Ctrl+V"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires local ASR model and loopback mock AI"]
    fn real_pipeline_keeps_asking_and_recovers_from_ai_failure() {
        use serde_json::{Value, json};
        use std::io::{BufRead, BufReader, Read, Write};
        let model = std::env::var("VIBE_TEST_MODEL").unwrap();
        let mut reader =
            hound::WavReader::open(std::path::Path::new(&model).join("test_wavs/zh.wav")).unwrap();
        let spec = reader.spec();
        let samples: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / 32768.0)
            .collect();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let server = std::thread::spawn(move || {
            for step in 0..4 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(15)))
                    .unwrap();
                let mut input = BufReader::new(&mut socket);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    assert_ne!(input.read_line(&mut line).unwrap(), 0);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                }
                let mut bytes = vec![0; length];
                input.read_exact(&mut bytes).unwrap();
                let body: Value = serde_json::from_slice(&bytes).unwrap();
                let data: Value =
                    serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
                if step == 0 {
                    assert!(data["确认上下文"].is_null());
                } else {
                    let c = &data["确认上下文"];
                    assert!(!c["原始口述"].as_str().unwrap().is_empty());
                    assert_eq!(c["历轮问答"].as_array().unwrap().len(), step - 1);
                    assert!(c.get("本轮是唯一一次补充").is_none());
                    if step >= 2 {
                        assert_eq!(c["本轮待回答问题"], "还是不明确，请说明令牌类型？");
                    }
                }
                let content = match step {
                    0 => json!({"text":"不应暴露的草稿", "question":"是哪一种令牌？"}),
                    1 => json!({"text":"", "question":"还是不明确，请说明令牌类型？"}),
                    2 => json!({"text":"缺少 question，不可当成确认"}),
                    _ => json!({"text":"最终确认：访问令牌过期时先刷新。", "question":null}),
                };
                let response = json!({"choices":[{"finish_reason":"stop","message":{"content":content.to_string()}}]}).to_string();
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
            }
        });
        let mut state = NativeAppState::default();
        for step in 0..4 {
            let snapshot = FinishInputSnapshot {
                clarification: state.clarification.clone(),
                ai: crate::ai::AiConfig {
                    endpoint: endpoint.clone(),
                    model: "mock".into(),
                    enabled: true,
                    ..Default::default()
                },
                input_mode: InputMode::CodeEdit,
                auto_paste: true,
                model_provider: ModelProvider::Local,
                local_model_dir: model.clone(),
                local_model_ready: false,
                local_model_status: String::new(),
                local_model_summary: String::new(),
                qwen_api_key: String::new(),
                qwen_model: String::new(),
                qwen_url: String::new(),
                qwen_language: String::new(),
                qwen_ready: false,
                qwen_status: String::new(),
            };
            let work = FinishInputWork {
                capture: RecordingCapture {
                    samples: samples.clone(),
                    sample_rate: spec.sample_rate,
                    channels: spec.channels,
                    duration_secs: 5.0,
                },
                last_recording_info: String::new(),
                snapshot,
            };
            let result =
                run_finish_input_work(work, &TaskControl::new(std::time::Duration::from_secs(30)));
            assert_eq!(result.input_state, InputState::Success);
            let delivered = std::cell::Cell::new(false);
            if step < 3 {
                assert!(result.review_required);
                assert!(result.clarification.is_some());
                assert_eq!(result.delivered_text.as_deref(), Some(""));
            } else {
                assert!(!result.review_required);
                assert!(result.clarification.is_none());
                assert_eq!(
                    result.delivered_text.as_deref(),
                    Some("最终确认：访问令牌过期时先刷新。")
                );
            }
            apply_finish_result(&mut state, result, |_| {
                delivered.set(true);
                Ok(())
            });
            assert_eq!(delivered.get(), step == 3);
        }
        server.join().unwrap();
    }
    #[test]
    #[ignore = "requires VIBE_TEST_MODEL with the bundled Chinese test audio"]
    fn code_mode_uses_ai_without_general_toggle_and_keeps_text_on_failure() {
        let model = std::env::var("VIBE_TEST_MODEL").unwrap();
        let mut reader =
            hound::WavReader::open(std::path::Path::new(&model).join("test_wavs/zh.wav")).unwrap();
        let spec = reader.spec();
        let samples: Vec<f32> = reader
            .samples::<i16>()
            .map(|sample| sample.unwrap() as f32 / 32768.0)
            .collect();
        let work = |input_mode| FinishInputWork {
            capture: RecordingCapture {
                samples: samples.clone(),
                sample_rate: spec.sample_rate,
                channels: spec.channels,
                duration_secs: 5.0,
            },
            last_recording_info: String::new(),
            snapshot: FinishInputSnapshot {
                clarification: None,
                ai: crate::ai::AiConfig {
                    enabled: false,
                    ..Default::default()
                },
                input_mode,
                auto_paste: false,
                model_provider: ModelProvider::Local,
                local_model_dir: model.clone(),
                local_model_ready: false,
                local_model_status: String::new(),
                local_model_summary: String::new(),
                qwen_api_key: String::new(),
                qwen_model: String::new(),
                qwen_url: String::new(),
                qwen_language: String::new(),
                qwen_ready: false,
                qwen_status: String::new(),
            },
        };
        let task = TaskControl::new(std::time::Duration::from_secs(30));
        let disabled = run_finish_input_work(work(InputMode::DirectPrompt), &task);
        assert_eq!(disabled.input_state, InputState::Success);
        assert!(disabled.ai_note.is_none());
        assert_eq!(disabled.raw_text, disabled.delivered_text);
        let failed = run_finish_input_work(work(InputMode::CodeEdit), &task);
        assert_eq!(failed.input_state, InputState::Success);
        assert!(failed.ai_note.unwrap().contains("AI 整理失败"));
        assert!(!failed.raw_text.as_ref().unwrap().is_empty());
        assert_eq!(failed.raw_text, failed.delivered_text);
    }
    #[test]
    fn switched_target_never_receives_automatic_paste() {
        let copied = std::cell::Cell::new(false);
        assert!(
            paste_without_refocusing(
                "text",
                || Err(anyhow!("changed")),
                |_| {
                    copied.set(true);
                    Ok(())
                },
                || panic!("paste sent")
            )
            .is_err()
        );
        assert!(copied.get());
        let sent = std::cell::Cell::new(false);
        assert!(
            paste_without_refocusing(
                "text",
                || Ok(()),
                |_| Ok(()),
                || {
                    sent.set(true);
                    Ok(())
                }
            )
            .is_ok()
        );
        assert!(sent.get());
        assert_eq!(new_delivery_target(42, 7, 7), None);
        assert_eq!(new_delivery_target(42, 8, 7), Some(42));
        assert_eq!(new_delivery_target(0, 0, 7), None);
    }
    fn result(state: InputState) -> FinishInputResult {
        FinishInputResult {
            clarification: None,
            review_required: false,
            ai_note: None,
            input_state: state,
            status_message: "完成".into(),
            raw_text: (state == InputState::Success).then(|| "添加 refresh token".into()),
            delivered_text: (state == InputState::Success).then(|| "添加 refresh token".into()),
            local_model_ready: true,
            local_model_status: String::new(),
            local_model_summary: String::new(),
            qwen_ready: false,
            qwen_status: String::new(),
            last_recording_info: String::new(),
            auto_paste: true,
        }
    }
    #[test]
    fn delivery_failure_preserves_successful_transcription() {
        let mut state = NativeAppState::default();
        let error = apply_finish_result(&mut state, result(InputState::Success), |_| {
            Err(anyhow!("目标已关闭"))
        });
        assert!(error.unwrap().contains("结果已保留"));
        assert_eq!(state.raw_text, "添加 refresh token");
        assert_eq!(state.input_state, InputState::Success);
    }
    #[test]
    fn clarification_retains_context_skips_paste_and_clears_after_answer() {
        let mut state = NativeAppState::default();
        let mut first = result(InputState::Success);
        first.review_required = true;
        first.ai_note = Some("AI 需要确认".into());
        first.clarification = Some(crate::ai::Clarification {
            original: "原始要求".into(),
            question: "你指的是访问令牌吗？".into(),
            mode: InputMode::CodeEdit,
            answers: Vec::new(),
        });
        assert_eq!(
            spoken_question(&first).as_deref(),
            Some("你指的是访问令牌吗？")
        );
        apply_finish_result(&mut state, first, |_| {
            panic!("must not paste before clarification")
        });
        assert_eq!(state.clarification.as_ref().unwrap().original, "原始要求");
        apply_finish_result(&mut state, result(InputState::Error), |_| {
            panic!("failed ASR must not paste")
        });
        assert!(state.clarification.is_some());
        for i in 1..=3 {
            let mut answer = result(InputState::Success);
            answer.review_required = true;
            answer.ai_note = Some("AI 需要确认".into());
            answer.delivered_text = Some(String::new());
            answer.clarification = Some(next_clarification(
                state.clarification.as_ref(),
                "还没说明白",
                InputMode::CodeEdit,
                format!("第 {i} 轮问题？"),
            ));
            assert_eq!(spoken_question(&answer), Some(format!("第 {i} 轮问题？")));
            apply_finish_result(&mut state, answer, |_| {
                panic!("unresolved result must not paste")
            });
            let c = state.clarification.as_ref().unwrap();
            assert_eq!(c.answers.len(), i);
            assert_eq!(c.original, "原始要求");
            assert!(state.delivered_text.is_empty());
        }
        let delivered = std::cell::Cell::new(false);
        apply_finish_result(&mut state, result(InputState::Success), |_| {
            delivered.set(true);
            Ok(())
        });
        assert!(state.clarification.is_none());
        assert!(delivered.get());
    }
    #[test]
    fn empty_final_result_never_reports_paste_success() {
        let mut empty = result(InputState::Success);
        empty.delivered_text = Some("  ".into());
        assert!(auto_paste_result(&empty).is_err());
    }
    #[test]
    fn paste_chord_is_one_complete_virtual_key_batch() {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
        let inputs = paste_inputs();
        let keys: Vec<_> = inputs
            .iter()
            .map(|i| unsafe { (i.Anonymous.ki.wVk, i.Anonymous.ki.dwFlags) })
            .collect();
        assert_eq!(
            keys,
            vec![
                (VK_LCONTROL, 0),
                (VK_V, 0),
                (VK_V, KEYEVENTF_KEYUP),
                (VK_LCONTROL, KEYEVENTF_KEYUP)
            ]
        );
    }
    #[test]
    fn only_pending_ai_questions_are_spoken() {
        let mut done = result(InputState::Success);
        for note in [
            None,
            Some("AI 已整理"),
            Some("已取消整理"),
            Some("AI 整理失败：test"),
        ] {
            done.ai_note = note.map(str::to_owned);
            assert!(spoken_question(&done).is_none());
        }
        done.review_required = true;
        done.clarification = Some(next_clarification(
            None,
            "原话",
            InputMode::CodeEdit,
            "缓存还是线程？".into(),
        ));
        assert!(spoken_question(&done).is_none()); // Stale question after AI failure.
        done.ai_note = Some("AI 需要确认".into());
        assert_eq!(spoken_question(&done).as_deref(), Some("缓存还是线程？"));
        done.input_state = InputState::Error;
        assert!(spoken_question(&done).is_none());
    }
    #[test]
    fn failed_recognition_keeps_previous_text_without_delivery() {
        let mut state = NativeAppState {
            raw_text: "上一条".into(),
            ..Default::default()
        };
        apply_finish_result(&mut state, result(InputState::Error), |_| {
            panic!("must not paste a failed result")
        });
        assert_eq!(state.raw_text, "上一条");
    }
}
