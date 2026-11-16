//! The whole UI: one non-activating bar that expands while you hold the trigger
//! key, becomes a short card while you dictate, and collapses to a dot.

use crate::audio::{cancel_recording, recording_level, take_recording_error};
use crate::config;
use crate::holdkey;
use crate::services::{self, FinishInputResult};
use crate::settings::{self, SettingsAction, SettingsView};
use crate::state::{InputMode, InputState, ModelProvider, NativeAppState};
use crate::task::TaskControl;
use crate::tray::{TrayAction, TrayController};
use eframe::egui::{self, Align2, Color32, Context, FontId, Pos2, Rect, Stroke, StrokeKind, vec2};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const BAR_SIZE: [f32; 2] = [384.0, 64.0];
pub const WINDOW_TITLE: &str = "Vibe Voice Lite";

/// Resting size: a small transparent square holding a single dot.
const DOT_SIZE: f32 = 28.0;
/// Result card: wider than the bar, and as tall as the text needs.
const CARD_W: f32 = 440.0;
const CARD_MIN_H: f32 = 104.0;
const CARD_MAX_H: f32 = 380.0;
const CARD_PAD: f32 = 18.0;
const RESULT_FONT: f32 = 14.0;
/// Longest transcript kept on screen before it is clipped with an ellipsis.
const RESULT_CHARS: usize = 800;

const SURFACE: Color32 = Color32::from_rgb(26, 28, 34);
const BORDER: Color32 = Color32::from_rgb(58, 62, 72);
const TEXT: Color32 = Color32::from_rgb(232, 234, 239);
const MUTED: Color32 = Color32::from_rgb(146, 151, 162);
const RECORDING: Color32 = Color32::from_rgb(255, 107, 94);
const DONE: Color32 = Color32::from_rgb(109, 211, 160);
const ERROR: Color32 = Color32::from_rgb(240, 118, 108);

const MIN_HOLD: Duration = Duration::from_millis(280);
const MAX_HOLD: Duration = Duration::from_secs(300);
const RESULT_LINGER: Duration = Duration::from_millis(1500);
const ERROR_LINGER: Duration = Duration::from_secs(4);

enum Phase {
    Idle,
    Awaiting,
    Recording,
    Processing,
    Done { until: Instant, ok: bool },
}

pub struct VoiceLiteApp {
    speech: crate::speech::SpeechOutput,
    ctx: Context,
    state: NativeAppState,
    phase: Phase,
    recording_started_at: Option<Instant>,
    pending: Option<Receiver<FinishInputResult>>,
    active_task: Option<TaskControl>,
    health_pending: Option<Receiver<(bool, String)>>,
    model_ready: bool,
    tray: Option<TrayController>,
    settings_open: bool,
    settings_view: Arc<Mutex<SettingsView>>,
    settings_tx: Sender<SettingsAction>,
    settings_rx: Receiver<SettingsAction>,
    result_text: String,
    result_shown: String,
    paste_note: Option<String>,
    saved_config: serde_json::Value,
    last_down: bool,
    started_at: Instant,
    placed: bool,
    geometry: [f32; 2],
}

impl Drop for VoiceLiteApp {
    fn drop(&mut self) {
        config::lifecycle("cleanup begin");
        self.speech.shutdown();
        config::lifecycle("tts stopped");
        if let Some(task) = &self.active_task {
            task.cancel();
        }
        cancel_recording();
        config::lifecycle("audio stopped");
        let _ = config::save(&config::snapshot(&self.state));
        config::lifecycle("settings saved");
        self.tray.take();
        config::lifecycle("tray removed");
    }
}

impl eframe::App for VoiceLiteApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // wgpu composites per-pixel alpha, so the desktop shows through.
        [0.0; 4]
    }

    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if ctx.input(|input| input.viewport().close_requested()) {
            config::lifecycle("root close received");
            return;
        }
        self.poll_tray();
        self.demo_tick();
        self.poll_health();
        self.poll_finish();
        self.poll_speech();
        self.watch_recording();

        let settings_focused = if self.settings_open {
            let (keep, focused) = settings::show(ctx, &self.settings_view, &self.settings_tx);
            if !keep {
                self.settings_open = false;
            }
            self.drain_settings();
            self.settings_open && focused
        } else {
            false
        };
        self.poll_hold_key(settings_focused);
        self.expire_phase();

        // Resize to fit the current state, then paint into that size.
        if !self.placed && self.started_at.elapsed() > Duration::from_millis(300) {
            self.placed = true;
        }
        if self.placed {
            self.sync_geometry(ctx);
        }
        self.paint(ctx);
        self.persist();

        let cadence = if matches!(self.phase, Phase::Recording) {
            16
        } else {
            40
        };
        ctx.request_repaint_after(Duration::from_millis(cadence));
    }
}

impl VoiceLiteApp {
    pub fn new(ctx: &Context) -> Self {
        let mut state = NativeAppState::default();
        let load_error = config::load(&mut state).err();
        let (settings_tx, settings_rx) = mpsc::channel();
        let view = Arc::new(Mutex::new(SettingsView::from_state(&state)));
        let tooltip = format!(
            "Vibe Voice Lite · Qwen Serena · 按住{}说话",
            holdkey::label(&state.hold_key)
        );
        let mut app = Self {
            speech: crate::speech::SpeechOutput::default(),
            ctx: ctx.clone(),
            state,
            phase: Phase::Idle,
            recording_started_at: None,
            pending: None,
            active_task: None,
            health_pending: None,
            model_ready: false,
            tray: None,
            settings_open: false,
            settings_view: view,
            settings_tx,
            settings_rx,
            result_text: String::new(),
            result_shown: String::new(),
            paste_note: None,
            saved_config: serde_json::Value::Null,
            last_down: false,
            started_at: Instant::now(),
            placed: false,
            // Zero forces the first sync_geometry to size and place the window.
            geometry: [0.0, 0.0],
        };
        app.saved_config = config::snapshot(&app.state);
        app.tray = TrayController::install(
            app.state.input_mode,
            app.state.auto_paste,
            &tooltip,
            ctx,
            Self::request_close,
        )
        .ok();
        app.start_health_check(false);
        if app.state.tts_enabled
            && let Err(error) = app.speech.start()
        {
            app.speech.status = format!("语音不可用：{error}");
        }
        if let Some(error) = load_error {
            app.set_error(format!("设置读取失败：{error}"));
        }
        app
    }

    /// Env-gated visual check: pins one bar state without touching the mic.
    /// `VIBE_LITE_DEMO=rec|proc|done|err|long`
    fn demo_tick(&mut self) {
        let Some(mode) = std::env::var_os("VIBE_LITE_DEMO") else {
            return;
        };
        let mode = mode.to_string_lossy().to_string();
        let next = match mode.as_str() {
            "rec" => Phase::Recording,
            "proc" => Phase::Processing,
            "done" | "long" => Phase::Done {
                until: Instant::now() + Duration::from_secs(3600),
                ok: true,
            },
            "err" => Phase::Done {
                until: Instant::now() + Duration::from_secs(3600),
                ok: false,
            },
            _ => Phase::Idle,
        };
        let changed = std::mem::discriminant(&self.phase) != std::mem::discriminant(&next);
        self.phase = next;
        if changed {
            self.result_text = if mode == "err" {
                "模型未就绪 · 托盘 → 设置".to_string()
            } else if mode == "long" {
                "帮我把登录页的报错文案全部改成中文，顺便把错误码映射抽成一个单独的函数，\
                 另外检查一下 refresh token 过期以后有没有重新登录的兜底逻辑，最后再补一版单元测试。"
                    .to_string()
            } else {
                "把登录页的错误提示改成中文".to_string()
            };
            self.recording_started_at = Some(Instant::now() - Duration::from_secs(3));
            self.raise_bar();
        }
    }

    fn poll_tray(&mut self) {
        let Some(action) = self.tray.as_ref().and_then(|tray| tray.poll()) else {
            return;
        };
        match action {
            TrayAction::Settings => self.open_settings(),
            TrayAction::Mode(mode) => self.set_mode(mode),
            TrayAction::ToggleAutoPaste => self.state.auto_paste = !self.state.auto_paste,
        }
        if let Ok(mut view) = self.settings_view.lock() {
            view.input_mode = self.state.input_mode;
            view.auto_paste = self.state.auto_paste;
        }
    }

    fn request_close(ctx: &Context) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

        // Deliver a native close event even when the floating bar is inactive.
        let hwnd = own_window();
        let posted = hwnd != 0 && unsafe { PostMessageW(hwnd as _, WM_CLOSE, 0, 0) } != 0;
        config::lifecycle(&format!("close requested hwnd={hwnd} posted={posted}"));
        if !posted {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        }
    }

    fn start_health_check(&mut self, full: bool) {
        if let Ok(mut view) = self.settings_view.lock() {
            view.checking = true;
            view.status = "正在检查模型…".into();
        }
        let state = self.state.clone();
        let (tx, rx) = mpsc::channel();
        self.health_pending = Some(rx);
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let (ok, message) = settings::probe(&state, full);
            let _ = tx.send((ok, message));
            ctx.request_repaint();
        });
    }

    fn poll_health(&mut self) {
        let Some(rx) = self.health_pending.as_ref() else {
            return;
        };
        let (ok, message) = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => (false, "模型检查意外结束，请重新检查。".into()),
        };
        self.health_pending = None;
        self.model_ready = ok;
        self.debug_log(&format!("health ready={ok} status={message}"));
        if let Ok(mut view) = self.settings_view.lock() {
            view.checking = false;
            view.status = message;
        }
        if !ok && !self.settings_open {
            self.open_settings();
        }
    }

    fn poll_finish(&mut self) {
        let received = match self.pending.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(result)) => Some(Ok(result)),
            Some(Err(TryRecvError::Disconnected)) => Some(Err(())),
            _ => None,
        };
        let Some(received) = received else {
            return;
        };
        self.pending = None;
        self.active_task = None;
        match received {
            Ok(result) => self.accept_finish_result(result),
            Err(()) => self.set_error("识别进程意外退出，请重试"),
        }
    }

    fn say(&mut self, text: &str) {
        if self.state.tts_enabled
            && let Err(error) = self.speech.speak(text)
        {
            self.speech.status = format!("语音不可用：{error}；文字结果仍保留");
        }
    }

    fn poll_speech(&mut self) {
        if let Err(error) = self.speech.poll() {
            self.speech.status = format!("语音不可用：{error}");
            config::lifecycle("tts error; text retained");
        }
        if let Ok(mut view) = self.settings_view.lock() {
            view.tts_status = self.speech.status.clone();
            view.tts_enabled = self.state.tts_enabled;
            view.clarifying = self.state.clarification.is_some();
        }
    }

    fn cancel_clarification(&mut self) {
        self.speech.stop();
        if let Some(task) = self.active_task.take() {
            task.cancel();
        }
        self.pending = None;
        cancel_recording();
        self.recording_started_at = None;
        self.state.clarification = None;
        self.phase = Phase::Idle;
        self.state.input_state = InputState::Idle;
    }

    fn set_mode(&mut self, mode: InputMode) {
        if self.state.input_mode != mode {
            self.cancel_clarification();
            self.state.input_mode = mode;
        }
        if let Some(tray) = &self.tray {
            tray.set_mode(mode);
        }
    }

    fn accept_finish_result(&mut self, result: FinishInputResult) {
        let auto_paste = result.auto_paste;
        let ai_note = result.ai_note.clone();
        let review = result.review_required;
        let question = result.clarification.as_ref().map(|c| c.question.clone());
        let spoken = services::spoken_question(&result);
        let display = if ai_note.is_some() {
            result.delivered_text.clone()
        } else {
            result.raw_text.clone()
        };
        let paste_error =
            services::apply_finish_result(&mut self.state, result, services::auto_paste_result);
        let ok = self.state.input_state == InputState::Success;
        if let Ok(mut view) = self.settings_view.lock() {
            view.raw_text = self.state.raw_text.clone();
            view.delivered_text = self.state.delivered_text.clone();
            view.ai_status = self.state.ai_status.clone();
        }
        let ai_failed = ai_note
            .as_deref()
            .is_some_and(|n| n.starts_with("AI 整理失败"));
        let cancelled = ai_note.as_deref() == Some("已取消整理");
        self.result_text = if cancelled {
            "已取消本次整理".into()
        } else if ai_failed {
            "AI 整理失败，回答已保留。可稍后再试，或按 Esc 取消。".into()
        } else if ok {
            question.clone().or(display).unwrap_or_default()
        } else {
            self.state.status_message.clone()
        };
        let delivery = if review {
            "未自动粘贴"
        } else if paste_error.is_some() {
            "自动粘贴未完成"
        } else if !auto_paste {
            "自动粘贴已关闭"
        } else {
            "已复制并发送粘贴快捷键"
        };
        self.paste_note = Some(if cancelled {
            "未自动粘贴 · 可以开始新的口述".into()
        } else if ai_failed {
            "AI 服务异常 · 未自动粘贴".into()
        } else if question.is_some() && self.state.input_mode == InputMode::Interview {
            format!(
                "需求访谈 · 按住{}回答 · 说“先整理到这里”结束",
                holdkey::label(&self.state.hold_key)
            )
        } else if question.is_some() {
            format!(
                "AI 等待补充 · 按住{}回答 · Esc 取消",
                holdkey::label(&self.state.hold_key)
            )
        } else if review
            && ai_note
                .as_deref()
                .is_some_and(|n| n.starts_with("AI 整理失败"))
        {
            "AI 整理失败 · 原文已保留 · 未自动粘贴".into()
        } else if review {
            "AI 仍有待确认项 · 请查看文字结果".into()
        } else if let Some(note) = ai_note.as_deref() {
            format!(
                "{} · {delivery}",
                if note == "AI 已整理" {
                    "AI 已整理"
                } else {
                    "AI 整理失败，已用原文"
                }
            )
        } else {
            delivery.into()
        });
        self.phase = if self.state.clarification.is_some() {
            Phase::Awaiting
        } else {
            Phase::Done {
                until: Instant::now()
                    + if ok && !review {
                        RESULT_LINGER
                    } else {
                        ERROR_LINGER
                    },
                ok,
            }
        };
        if let Some(text) = spoken {
            self.say(&text);
        }
        self.raise_bar();
    }

    fn watch_recording(&mut self) {
        if std::env::var_os("VIBE_LITE_DEMO").is_some() {
            return;
        }
        if !matches!(self.phase, Phase::Recording) {
            return;
        }
        if let Some(error) = take_recording_error() {
            cancel_recording();
            self.set_error(error);
        } else if self
            .recording_started_at
            .is_some_and(|started| started.elapsed() >= MAX_HOLD)
        {
            self.stop_recording();
        }
    }

    fn poll_hold_key(&mut self, settings_focused: bool) {
        if std::env::var_os("VIBE_LITE_DEMO").is_some() {
            return;
        }
        let down = holdkey::is_down(&self.state.hold_key);
        if matches!(self.phase, Phase::Awaiting)
            && !settings_focused
            && unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x1B) }
                < 0
        {
            self.cancel_clarification();
        }
        self.handle_hold_key(down, settings_focused);
    }

    fn handle_hold_key(&mut self, down: bool, settings_focused: bool) {
        let pressed = down && !self.last_down;
        if down != self.last_down {
            self.last_down = down;
            self.debug_log(&format!("hold={down}"));
        }
        if pressed
            && !settings_focused
            && matches!(
                self.phase,
                Phase::Idle | Phase::Awaiting | Phase::Done { .. }
            )
        {
            self.start_recording();
        } else if !down && matches!(self.phase, Phase::Recording) {
            self.stop_recording();
        }
    }

    fn start_recording(&mut self) {
        self.speech.stop();
        if !self.model_ready {
            self.set_error("模型未就绪 · 托盘 → 设置");
            self.open_settings();
            return;
        }
        self.state.local_model_ready =
            self.state.model_provider == ModelProvider::Local && self.model_ready;
        self.state.qwen_ready =
            self.state.model_provider == ModelProvider::OnlineQwen && self.model_ready;
        services::begin_input(&mut self.state);
        self.debug_log(&format!(
            "begin state={:?} message={}",
            self.state.input_state, self.state.status_message
        ));
        if self.state.input_state == InputState::Recording {
            self.phase = Phase::Recording;
            self.recording_started_at = Some(Instant::now());
            self.paste_note = None;
            self.raise_bar();
        } else {
            let message = self.state.status_message.clone();
            self.set_error(message);
        }
    }

    fn stop_recording(&mut self) {
        let elapsed = self
            .recording_started_at
            .take()
            .map(|started| started.elapsed())
            .unwrap_or_default();
        self.debug_log(&format!("stop elapsed={elapsed:?}"));
        if elapsed < MIN_HOLD {
            cancel_recording();
            self.state.input_state = InputState::Idle;
            self.set_error("说话太短，按住再说一次");
            return;
        }
        match services::begin_finish_input(&mut self.state) {
            Ok(work) => {
                let task = TaskControl::new(Duration::from_secs(120));
                self.active_task = Some(task.clone());
                let (tx, rx) = mpsc::channel();
                self.pending = Some(rx);
                self.phase = Phase::Processing;
                let ctx = self.ctx.clone();
                thread::spawn(move || {
                    let result = services::run_finish_input_work(work, &task);
                    let _ = tx.send(result);
                    ctx.request_repaint();
                });
            }
            Err(error) => {
                cancel_recording();
                self.state.input_state = InputState::Error;
                self.set_error(format!("无法结束录音：{error:#}"));
            }
        }
    }

    fn expire_phase(&mut self) {
        let expired = match &self.phase {
            Phase::Done { until, .. } => Instant::now() >= *until,
            _ => false,
        };
        if expired {
            self.phase = Phase::Idle;
        }
    }

    fn set_error(&mut self, message: impl Into<String>) {
        if let Some(task) = self.active_task.take() {
            task.cancel();
        }
        self.pending = None;
        cancel_recording();
        self.recording_started_at = None;
        self.state.input_state = InputState::Error;
        self.paste_note = None;
        let message = message.into();
        self.debug_log(&format!("error: {message}"));
        self.result_text = message;
        self.phase = Phase::Done {
            until: Instant::now() + ERROR_LINGER,
            ok: false,
        };
        self.raise_bar();
    }

    /// Diagnostics stay off unless VIBE_LITE_DEBUG is set.
    fn debug_log(&self, message: &str) {
        use std::io::Write;
        if std::env::var_os("VIBE_LITE_DEBUG").is_none() {
            return;
        }
        let path = std::env::temp_dir().join("vibe-lite-debug.log");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{message}");
        }
    }

    /// Re-assert topmost z-order so the bar is not hidden by another overlay.
    fn raise_bar(&self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
        };
        let hwnd = own_window();
        if hwnd != 0 {
            unsafe {
                SetWindowPos(
                    hwnd as _,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }

    fn open_settings(&mut self) {
        if self.settings_open {
            return;
        }
        if let Ok(mut view) = self.settings_view.lock() {
            *view = SettingsView::from_state(&self.state);
        }
        self.settings_open = true;
    }

    fn drain_settings(&mut self) {
        while let Ok(action) = self.settings_rx.try_recv() {
            if matches!(
                &action,
                SettingsAction::Online(_)
                    | SettingsAction::LocalDir(_)
                    | SettingsAction::QwenKey(_)
                    | SettingsAction::QwenModel(_)
                    | SettingsAction::QwenUrl(_)
            ) {
                // Discard checks made for the previous model configuration.
                self.health_pending = None;
                if let Ok(mut view) = self.settings_view.lock() {
                    view.checking = false;
                    view.status = "配置已修改，请检查模型或关闭设置后自动检查。".into();
                }
            }
            match action {
                SettingsAction::Tts(on) => {
                    self.state.tts_enabled = on;
                    if on {
                        if let Err(e) = self.speech.start() {
                            self.speech.status = format!("语音不可用：{e}");
                        }
                    } else {
                        self.speech.shutdown();
                    }
                }
                SettingsAction::TestSpeech => {
                    self.say("需要确认一下：你说的是访问令牌，还是刷新令牌？")
                }
                SettingsAction::RestartSpeech => {
                    self.speech.shutdown();
                    if self.state.tts_enabled
                        && let Err(e) = self.speech.start()
                    {
                        self.speech.status = format!("语音不可用：{e}");
                    }
                }
                SettingsAction::CancelClarification => self.cancel_clarification(),
                SettingsAction::Ai(mut config) => {
                    if config.api_key != self.state.ai.api_key
                        || config.endpoint != self.state.ai.endpoint
                    {
                        match crate::credentials::protect(&config.api_key, &config.endpoint) {
                            Ok(encrypted) => config.protected_key = encrypted,
                            Err(error) => {
                                if let Ok(mut view) = self.settings_view.lock() {
                                    view.ai = self.state.ai.clone();
                                    view.ai_status = format!("保存密钥失败：{error}");
                                }
                                continue;
                            }
                        }
                    }
                    self.state.ai = config;
                }
                SettingsAction::HoldKey(key) => self.state.hold_key = key,
                SettingsAction::AutoPaste(on) => self.state.auto_paste = on,
                SettingsAction::Mode(mode) => self.set_mode(mode),
                SettingsAction::Online(online) => {
                    self.state.model_provider = if online {
                        ModelProvider::OnlineQwen
                    } else {
                        ModelProvider::Local
                    };
                    self.model_ready = false;
                }
                SettingsAction::LocalDir(dir) => {
                    self.state.local_model_dir = dir;
                    self.model_ready = false;
                }
                SettingsAction::QwenKey(key) => {
                    self.state.qwen_api_key = key;
                    self.model_ready = false;
                }
                SettingsAction::QwenModel(model) => {
                    self.state.qwen_model = model;
                    self.model_ready = false;
                }
                SettingsAction::QwenUrl(url) => {
                    self.state.qwen_url = url;
                    self.model_ready = false;
                }
                SettingsAction::Check => self.start_health_check(true),
                SettingsAction::Close => {
                    self.settings_open = false;
                    if !self.model_ready && self.health_pending.is_none() {
                        self.start_health_check(false);
                    }
                }
            }
        }
    }

    fn persist(&mut self) {
        let snapshot = config::snapshot(&self.state);
        if snapshot == self.saved_config {
            return;
        }
        if let Err(error) = config::save(&snapshot) {
            self.set_error(format!("设置保存失败：{error}"));
            return;
        }
        self.saved_config = snapshot;
        if let Some(tray) = &self.tray {
            tray.set_mode(self.state.input_mode);
            tray.set_auto_paste(self.state.auto_paste);
        }
    }

    fn elapsed_label(&self) -> String {
        let secs = self
            .recording_started_at
            .map(|started| started.elapsed().as_secs())
            .unwrap_or(0);
        format!("{}:{:02}", secs / 60, secs % 60)
    }

    /// Pick a window size for the current state and resize once when it changes.
    fn sync_geometry(&mut self, ctx: &Context) {
        let (w, h) = match &self.phase {
            Phase::Idle => (DOT_SIZE, DOT_SIZE),
            Phase::Done { .. } | Phase::Awaiting => {
                let wrap = CARD_W - CARD_PAD * 2.0;
                let shown = fit_result(ctx, &self.result_text, wrap, CARD_MAX_H);
                let height = card_height(ctx, &shown, wrap);
                self.result_shown = shown;
                (CARD_W, height)
            }
            _ => (BAR_SIZE[0], BAR_SIZE[1]),
        };
        if (self.geometry[0] - w).abs() < 0.5 && (self.geometry[1] - h).abs() < 0.5 {
            return;
        }
        self.geometry = [w, h];
        resize_bar(ctx.pixels_per_point(), w, h);
        ctx.request_repaint();
    }

    fn paint(&self, ctx: &Context) {
        let [width, height] = self.geometry;
        let key = holdkey::label(&self.state.hold_key).to_string();
        let phase = &self.phase;
        let elapsed = self.elapsed_label();
        let shown = self.result_shown.clone();
        let note = self.paste_note.clone();

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let painter = ui.painter();
                let rect = Rect::from_min_size(Pos2::ZERO, vec2(width, height));
                match phase {
                    Phase::Idle => paint_dot(painter, rect),
                    Phase::Done { ok, .. } => {
                        paint_card(painter, rect, *ok, false, &shown, note.as_deref())
                    }
                    Phase::Awaiting => {
                        paint_card(painter, rect, true, true, &shown, note.as_deref())
                    }
                    Phase::Recording => paint_pill(
                        painter,
                        rect,
                        PillState::Recording {
                            level: recording_level(),
                            elapsed: &elapsed,
                            hint: &format!("松开 {key} 结束并识别"),
                        },
                    ),
                    Phase::Processing => paint_pill(
                        painter,
                        rect,
                        PillState::Processing {
                            time: ctx.input(|i| i.time) as f32,
                        },
                    ),
                }
            });
    }
}

enum PillState<'a> {
    Recording {
        level: f32,
        elapsed: &'a str,
        hint: &'a str,
    },
    Processing {
        time: f32,
    },
}

fn paint_dot(painter: &egui::Painter, rect: Rect) {
    painter.circle_filled(
        rect.center(),
        5.0,
        Color32::from_rgba_unmultiplied(150, 155, 166, 120),
    );
}

fn paint_pill(painter: &egui::Painter, rect: Rect, state: PillState<'_>) {
    let radius = rect.height() / 2.0;
    // Soft drop shadow, then the capsule itself.
    painter.rect_filled(
        rect.translate(vec2(0.0, 3.0)),
        radius,
        Color32::from_black_alpha(70),
    );
    painter.rect_filled(rect.shrink(1.0), radius - 1.0, SURFACE);
    painter.rect_stroke(
        rect.shrink(1.0),
        radius - 1.0,
        Stroke::new(1.0_f32, BORDER),
        StrokeKind::Inside,
    );

    let at = |x: f32, y: f32| Pos2::new(rect.left() + x, rect.top() + y);

    match state {
        PillState::Recording {
            level,
            elapsed,
            hint,
        } => {
            let level = level.clamp(0.0, 1.0);
            for (index, weight) in [0.35_f32, 0.65, 1.0, 0.8, 0.5].iter().enumerate() {
                let bar = 4.0 + 24.0 * level * weight;
                painter.rect_filled(
                    Rect::from_center_size(at(30.0 + index as f32 * 9.0, 32.0), vec2(4.0, bar)),
                    2,
                    RECORDING,
                );
            }
            painter.text(
                at(86.0, 23.0),
                Align2::LEFT_CENTER,
                "正在听…",
                FontId::proportional(15.0),
                TEXT,
            );
            painter.text(
                at(rect.width() - 22.0, 23.0),
                Align2::RIGHT_CENTER,
                elapsed,
                FontId::monospace(14.0),
                MUTED,
            );
            painter.text(
                at(86.0, 43.0),
                Align2::LEFT_CENTER,
                hint,
                FontId::proportional(11.0),
                MUTED,
            );
        }
        PillState::Processing { time } => {
            for index in 0..3 {
                let pulse = ((time * 3.0) - index as f32 * 0.6).sin() * 0.5 + 0.5;
                painter.circle_filled(at(30.0 + index as f32 * 9.0, 32.0), 2.0 + 1.8 * pulse, DONE);
            }
            painter.text(
                at(86.0, 23.0),
                Align2::LEFT_CENTER,
                "处理中…",
                FontId::proportional(15.0),
                TEXT,
            );
            painter.text(
                at(86.0, 43.0),
                Align2::LEFT_CENTER,
                "松开后就交给本地模型",
                FontId::proportional(11.0),
                MUTED,
            );
        }
    }
}

fn paint_card(
    painter: &egui::Painter,
    rect: Rect,
    ok: bool,
    waiting: bool,
    text: &str,
    note: Option<&str>,
) {
    let radius = 18.0;
    painter.rect_filled(
        rect.translate(vec2(0.0, 4.0)),
        radius,
        Color32::from_black_alpha(80),
    );
    painter.rect_filled(rect.shrink(1.0), radius - 1.0, SURFACE);
    painter.rect_stroke(
        rect.shrink(1.0),
        radius - 1.0,
        Stroke::new(1.0_f32, BORDER),
        StrokeKind::Inside,
    );

    let color = if ok { DONE } else { ERROR };
    let left = rect.left() + CARD_PAD;
    let top = rect.top();

    if ok {
        painter.add(egui::Shape::line(
            vec![
                Pos2::new(left, top + 24.0),
                Pos2::new(left + 4.0, top + 28.0),
                Pos2::new(left + 11.0, top + 18.0),
            ],
            Stroke::new(2.0_f32, color),
        ));
    } else {
        painter.text(
            Pos2::new(left + 4.0, top + 24.0),
            Align2::LEFT_CENTER,
            "!",
            FontId::proportional(16.0),
            color,
        );
    }
    painter.text(
        Pos2::new(left + 22.0, top + 23.0),
        Align2::LEFT_CENTER,
        if waiting {
            "需要确认"
        } else if ok {
            "已识别"
        } else {
            "识别失败"
        },
        FontId::proportional(12.0),
        color,
    );
    painter.text(
        Pos2::new(rect.right() - CARD_PAD, top + 23.0),
        Align2::RIGHT_CENTER,
        format!("{} 字", text.chars().count()),
        FontId::proportional(11.0),
        MUTED,
    );

    let wrap = rect.width() - CARD_PAD * 2.0;
    let galley = painter.layout(
        text.to_owned(),
        FontId::proportional(RESULT_FONT),
        if ok { TEXT } else { ERROR },
        wrap,
    );
    painter.galley(Pos2::new(left, top + 46.0), galley, TEXT);

    let hint = if ok {
        note.unwrap_or("结果已保留")
    } else {
        "稍后自动收起"
    };
    painter.text(
        Pos2::new(left, rect.bottom() - 17.0),
        Align2::LEFT_CENTER,
        hint,
        FontId::proportional(11.0),
        MUTED,
    );
}

/// Height of the result card for text that already fits.
fn card_height(ctx: &Context, text: &str, wrap: f32) -> f32 {
    let galley = ctx.fonts(|fonts| {
        fonts.layout(
            text.to_owned(),
            FontId::proportional(RESULT_FONT),
            TEXT,
            wrap,
        )
    });
    (46.0 + galley.size().y + 30.0).clamp(CARD_MIN_H, CARD_MAX_H)
}

/// Cut the transcript down until it fits the tallest card, keeping an ellipsis.
fn fit_result(ctx: &Context, text: &str, wrap: f32, max_height: f32) -> String {
    let clipped = truncate(text, RESULT_CHARS);
    if card_height(ctx, &clipped, wrap) <= max_height {
        return clipped;
    }
    let total = clipped.chars().count();
    let mut low = 0usize;
    let mut high = total;
    while low + 1 < high {
        let mid = (low + high) / 2;
        if card_height(ctx, &truncate(&clipped, mid), wrap) <= max_height {
            low = mid;
        } else {
            high = mid;
        }
    }
    truncate(&clipped, low.max(16))
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    let mut output: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        output.push('…');
    }
    output
}

/// Resize and reposition the window, keeping it centred and above the taskbar.
fn resize_bar(scale: f32, width_pts: f32, height_pts: f32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW,
        SetWindowLongPtrW, SetWindowPos, WS_EX_NOACTIVATE,
    };
    let hwnd = own_window();
    if hwnd == 0 {
        return;
    }
    let Some((left, _, right, bottom)) = work_area_physical() else {
        return;
    };
    let scale = scale.max(0.5);
    let width = (width_pts * scale).round().max(1.0) as i32;
    let height = (height_pts * scale).round().max(1.0) as i32;
    let x = (left + right) / 2.0 - width as f32 / 2.0;
    let y = bottom - 24.0 * scale - height as f32;
    unsafe {
        // Clicking a question card must not take the editor's keyboard focus.
        let style = GetWindowLongPtrW(hwnd as _, GWL_EXSTYLE);
        if style & WS_EX_NOACTIVATE as isize == 0 {
            SetWindowLongPtrW(hwnd as _, GWL_EXSTYLE, style | WS_EX_NOACTIVATE as isize);
        }
        SetWindowPos(
            hwnd as _,
            HWND_TOPMOST,
            x.round() as i32,
            y.round() as i32,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

fn work_area_physical() -> Option<(f32, f32, f32, f32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    unsafe {
        let mut cursor = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut cursor) == 0 {
            return None;
        }
        let monitor = MonitorFromPoint(
            POINT {
                x: cursor.x,
                y: cursor.y,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        if monitor.is_null() {
            return None;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        Some((
            info.rcWork.left as f32,
            info.rcWork.top as f32,
            info.rcWork.right as f32,
            info.rcWork.bottom as f32,
        ))
    }
}

/// Handle of our own top-level bar window, found by process id and title.
fn own_window() -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindow,
    };

    static HANDLE: AtomicIsize = AtomicIsize::new(0);
    let cached = HANDLE.load(Ordering::Relaxed);
    let mut owner = 0;
    if cached != 0 && unsafe { IsWindow(cached as _) } != 0 {
        unsafe {
            GetWindowThreadProcessId(cached as _, &mut owner);
        }
    }
    if owner == std::process::id() {
        return cached;
    }
    HANDLE.store(0, Ordering::Relaxed);

    unsafe extern "system" fn callback(hwnd: *mut core::ffi::c_void, _param: isize) -> i32 {
        let mut pid = 0u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
        }
        if pid == std::process::id() {
            let mut buffer = [0u16; 64];
            let len = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
            if len > 0 {
                let title = String::from_utf16_lossy(&buffer[..len as usize]);
                if title == WINDOW_TITLE {
                    HANDLE.store(hwnd as isize, Ordering::Relaxed);
                    return 0;
                }
            }
        }
        1
    }

    unsafe {
        EnumWindows(Some(callback), 0);
    }
    HANDLE.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> std::mem::ManuallyDrop<VoiceLiteApp> {
        let state = NativeAppState {
            tts_enabled: false,
            ..Default::default()
        };
        let (settings_tx, settings_rx) = mpsc::channel();
        std::mem::ManuallyDrop::new(VoiceLiteApp {
            speech: crate::speech::SpeechOutput::default(),
            ctx: Context::default(),
            settings_view: Arc::new(Mutex::new(SettingsView::from_state(&state))),
            saved_config: config::snapshot(&state),
            state,
            phase: Phase::Idle,
            recording_started_at: None,
            pending: None,
            active_task: None,
            health_pending: None,
            model_ready: true,
            tray: None,
            settings_open: false,
            settings_tx,
            settings_rx,
            result_text: String::new(),
            result_shown: String::new(),
            paste_note: None,
            last_down: false,
            started_at: Instant::now(),
            placed: false,
            geometry: [0.0, 0.0],
        })
    }

    #[test]
    fn editing_only_the_api_key_updates_the_persisted_snapshot() {
        let mut app = test_app();
        let before = config::snapshot(&app.state);
        let mut ai = app.state.ai.clone();
        ai.api_key = "test-only-ui-secret".into();
        app.settings_tx.send(SettingsAction::Ai(ai)).unwrap();
        app.drain_settings();
        assert_ne!(before, config::snapshot(&app.state));
        assert_eq!(
            crate::credentials::unprotect(&app.state.ai.protected_key, &app.state.ai.endpoint)
                .unwrap(),
            "test-only-ui-secret"
        );
        let mut ai = app.state.ai.clone();
        ai.api_key.clear();
        app.settings_tx.send(SettingsAction::Ai(ai)).unwrap();
        app.drain_settings();
        assert!(app.state.ai.protected_key.is_empty());
    }

    #[test]
    fn recording_error_clears_busy_state_and_stale_delivery_note() {
        let mut app = test_app();
        app.state.input_state = InputState::Recording;
        app.recording_started_at = Some(Instant::now());
        app.paste_note = Some("old delivery failure".into());
        app.set_error("microphone disconnected");
        assert_eq!(app.state.input_state, InputState::Error);
        assert!(app.recording_started_at.is_none());
        assert!(app.paste_note.is_none());
    }

    #[test]
    fn model_change_discards_in_flight_readiness_result() {
        let mut app = test_app();
        let (tx, rx) = mpsc::channel();
        app.health_pending = Some(rx);
        tx.send((true, "old model ready".into())).unwrap();
        app.settings_tx.send(SettingsAction::Online(true)).unwrap();
        app.drain_settings();
        app.poll_health();
        assert!(!app.model_ready);
        assert!(app.health_pending.is_none());
    }

    #[test]
    fn next_press_interrupts_result_card_but_held_key_does_not_restart() {
        let mut app = test_app();
        app.model_ready = false; // Exercise the start path without a real microphone.
        app.phase = Phase::Done {
            until: Instant::now() + RESULT_LINGER,
            ok: true,
        };
        app.last_down = true;
        app.handle_hold_key(true, false);
        assert!(!app.settings_open);
        app.handle_hold_key(false, false);
        app.handle_hold_key(true, false);
        assert!(app.settings_open);
        assert_eq!(app.state.input_state, InputState::Error);
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        assert_eq!(truncate("你好世界", 10), "你好世界");
        assert_eq!(truncate("一二三四五", 3), "一二三…");
        assert_eq!(truncate("", 3), "");
    }
    fn question_result() -> FinishInputResult {
        FinishInputResult {
            clarification: Some(crate::ai::Clarification {
                original: "原始要求".into(),
                question: "访问令牌还是刷新令牌？".into(),
                mode: InputMode::CodeEdit,
                answers: Vec::new(),
            }),
            review_required: true,
            ai_note: Some("AI 需要确认".into()),
            input_state: InputState::Success,
            status_message: "需要确认".into(),
            raw_text: Some("原始要求".into()),
            delivered_text: Some("整理正文\n待确认".into()),
            local_model_ready: true,
            local_model_status: String::new(),
            local_model_summary: String::new(),
            qwen_ready: false,
            qwen_status: String::new(),
            last_recording_info: String::new(),
            auto_paste: false,
        }
    }
    #[test]
    fn question_card_stays_visible_and_answer_clears_context() {
        let mut app = test_app();
        app.accept_finish_result(question_result());
        assert!(matches!(app.phase, Phase::Awaiting));
        app.expire_phase();
        assert!(matches!(app.phase, Phase::Awaiting));
        assert!(app.state.clarification.is_some());
        assert_eq!(app.result_text, "访问令牌还是刷新令牌？");
        for _ in 0..3 {
            app.accept_finish_result(question_result());
            app.expire_phase();
            assert!(matches!(app.phase, Phase::Awaiting));
            assert!(app.state.clarification.is_some());
        }
        let mut answer = question_result();
        answer.clarification = None;
        answer.review_required = false;
        answer.ai_note = Some("AI 已整理".into());
        app.accept_finish_result(answer);
        assert!(app.state.clarification.is_none());
        assert!(matches!(app.phase, Phase::Done { ok: true, .. }));
    }
    #[test]
    fn voice_cancellation_ends_questions_without_pasting() {
        let mut app = test_app();
        app.accept_finish_result(question_result());
        let mut result = question_result();
        result.clarification = None;
        result.review_required = false;
        result.ai_note = Some("已取消整理".into());
        result.delivered_text = Some(String::new());
        result.auto_paste = false;
        app.accept_finish_result(result);
        assert!(app.state.clarification.is_none());
        assert!(matches!(app.phase, Phase::Done { ok: true, .. }));
        assert_eq!(app.result_text, "已取消本次整理");
        assert!(app.state.delivered_text.is_empty());
    }
    #[test]
    fn ai_failure_displays_retry_instead_of_repeating_old_question() {
        let mut app = test_app();
        let mut result = question_result();
        result.ai_note = Some("AI 整理失败，格式无效".into());
        assert!(services::spoken_question(&result).is_none());
        app.accept_finish_result(result);
        assert!(app.state.clarification.is_some());
        assert!(app.result_text.contains("AI 整理失败"));
        assert!(!app.result_text.contains("访问令牌"));
    }
    #[test]
    fn cancelling_clarification_discards_inflight_answer() {
        let mut app = test_app();
        app.accept_finish_result(question_result());
        let task = TaskControl::new(Duration::from_secs(10));
        app.active_task = Some(task.clone());
        let (tx, rx) = mpsc::channel();
        app.pending = Some(rx);
        app.cancel_clarification();
        assert!(task.check().is_err());
        assert!(tx.send(question_result()).is_err());
        assert!(app.state.clarification.is_none());
        assert!(matches!(app.phase, Phase::Idle));
    }
    #[test]
    fn switching_modes_cancels_interview_and_pending_answer() {
        let mut app = test_app();
        app.set_mode(InputMode::Interview);
        let mut question = question_result();
        question.clarification.as_mut().unwrap().mode = InputMode::Interview;
        app.accept_finish_result(question);
        assert!(app.paste_note.as_deref().unwrap().contains("先整理到这里"));
        let task = TaskControl::new(Duration::from_secs(10));
        app.active_task = Some(task.clone());
        app.set_mode(InputMode::CodeEdit);
        assert!(task.check().is_err());
        assert!(app.state.clarification.is_none());
        assert!(matches!(app.phase, Phase::Idle));
    }
}
