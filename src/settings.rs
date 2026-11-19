//! Settings live in a small separate window opened from the tray, so the
//! product itself can stay a single always-on-top bar.

use crate::holdkey;
use crate::qwen_asr::{self, QwenAsrConfig};
use crate::services;
use crate::state::{InputMode, ModelProvider, NativeAppState};
use eframe::egui::{self, Context, RichText, ViewportBuilder, ViewportId};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

pub enum SettingsAction {
    Tts(bool),
    TestSpeech,
    RestartSpeech,
    CancelClarification,
    Ai(crate::ai::AiConfig),
    HoldKey(String),
    AutoPaste(bool),
    Mode(InputMode),
    Online(bool),
    LocalDir(String),
    QwenKey(String),
    QwenModel(String),
    QwenUrl(String),
    Check,
    Close,
}

pub struct SettingsView {
    pub tts_enabled: bool,
    pub tts_status: String,
    pub latency: crate::latency::LatencyHistory,
    pub clarifying: bool,
    pub ai: crate::ai::AiConfig,
    pub ai_status: String,
    pub raw_text: String,
    pub delivered_text: String,
    pub hold_key: String,
    pub auto_paste: bool,
    pub input_mode: InputMode,
    pub online: bool,
    pub local_dir: String,
    pub qwen_key: String,
    pub qwen_model: String,
    pub qwen_url: String,
    pub status: String,
    pub checking: bool,
    pub microphone: String,
    pub save_warning: Option<String>,
    pending_browse: Option<Receiver<Option<String>>>,
}

impl SettingsView {
    pub fn from_state(state: &NativeAppState) -> Self {
        Self {
            tts_enabled: state.tts_enabled,
            tts_status: String::new(),
            latency: Default::default(),
            clarifying: state.clarification.is_some(),
            ai: state.ai.clone(),
            ai_status: state.ai_status.clone(),
            raw_text: state.raw_text.clone(),
            delivered_text: state.delivered_text.clone(),
            hold_key: state.hold_key.clone(),
            auto_paste: state.auto_paste,
            input_mode: state.input_mode,
            online: state.model_provider == ModelProvider::OnlineQwen,
            local_dir: state.local_model_dir.clone(),
            qwen_key: state.qwen_api_key.clone(),
            qwen_model: state.qwen_model.clone(),
            qwen_url: state.qwen_url.clone(),
            status: "尚未检查模型。".to_string(),
            checking: false,
            microphone: "正在检查默认输入设备…".into(),
            save_warning: None,
            pending_browse: None,
        }
    }
}

/// Load the local model, or validate the online config. Runs off the UI thread.
///
/// `full` additionally opens a WebSocket to test the online endpoint; startup
/// only validates the configuration so a missing network never delays boot.
pub fn probe(state: &NativeAppState, full: bool) -> (bool, String) {
    let mut probe = state.clone();
    match probe.model_provider {
        ModelProvider::Local => services::check_local_model(&mut probe),
        ModelProvider::OnlineQwen if full => services::check_online_model(&mut probe),
        ModelProvider::OnlineQwen => {
            let config = QwenAsrConfig {
                api_key: probe.qwen_api_key.clone(),
                model: probe.qwen_model.clone(),
                url: probe.qwen_url.clone(),
                language: probe.qwen_language.clone(),
            };
            match qwen_asr::validate_config(&config) {
                Ok(()) => {
                    probe.qwen_ready = true;
                    probe.qwen_status = "在线配置完整；首次识别时才会建立连接。".to_string();
                }
                Err(error) => {
                    probe.qwen_ready = false;
                    probe.qwen_status = format!("在线配置不可用：{error:#}");
                }
            }
        }
    }
    let ready = probe.model_ready();
    let message = match probe.model_provider {
        ModelProvider::Local => probe.local_model_status.clone(),
        ModelProvider::OnlineQwen => probe.qwen_status.clone(),
    };
    (ready, message)
}

/// Draw the settings window and report whether it is open and focused.
pub fn show(
    ctx: &Context,
    view: &Arc<Mutex<SettingsView>>,
    tx: &Sender<SettingsAction>,
) -> (bool, bool) {
    let view_arc = Arc::clone(view);
    let tx = tx.clone();
    ctx.show_viewport_immediate(
        ViewportId::from_hash_of("vibe_voice_lite_settings"),
        ViewportBuilder::default()
            .with_title("Vibe Voice Lite · 设置")
            .with_icon(crate::icons::window())
            .with_inner_size([500.0, 680.0])
            .with_min_inner_size([420.0, 380.0])
            .with_resizable(true),
        move |ctx, class| {
            if class == egui::ViewportClass::Embedded {
                return (true, false);
            }
            let focused = ctx.input(|input| input.viewport().focused.unwrap_or(false));
            let mut open = true;
            if ctx.input(|input| input.viewport().close_requested()) {
                open = false;
                let _ = tx.send(SettingsAction::Close);
            }

            let Ok(mut view) = view_arc.lock() else {
                return (open, focused);
            };
            drain_browse(&mut view, &tx);

            let mut changes: Vec<SettingsAction> = Vec::new();
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.inner_margin(egui::Margin::same(18)))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.label(RichText::new("Vibe Voice Lite").size(20.0).strong());
                        ui.label(
                            RichText::new("按住触发键说话，松开后识别并粘贴到当前窗口。")
                                .size(12.0)
                                .color(egui::Color32::from_gray(140)),
                        );
                        if let Some(message) = &view.save_warning {
                            ui.colored_label(ui.visuals().warn_fg_color, message);
                        }
                        ui.add_space(14.0);

                        ui.label(RichText::new("使用前检查").strong());
                        egui::Grid::new("readiness").num_columns(2).max_col_width((ui.available_width() - 90.0).max(160.0)).spacing([12.0, 5.0]).show(ui, |ui| {
                            ui.label("麦克风");
                            ui.label(&view.microphone);
                            ui.end_row();
                            ui.label(if view.online { "在线识别" } else { "本地识别" });
                            ui.label(&view.status);
                            ui.end_row();
                            ui.label("AI 整理");
                            match view.ai.check_service() {
                                Ok(_) => ui.label("配置已填写 · 调用时验证服务"),
                                Err(error) => ui.label(format!("待配置 · {error}")),
                            };
                            ui.end_row();
                            ui.label("语音播报");
                            ui.label(if view.tts_enabled { view.tts_status.as_str() } else { "已关闭" });
                            ui.end_row();
                        });
                        if ui.add_enabled(!view.checking, egui::Button::new("重新检查设备和识别")).clicked() {
                            changes.push(SettingsAction::Check);
                        }
                        ui.add_space(10.0);
                        ui.separator();
                        ui.label(RichText::new("触发键").strong());
                        let mut selected = view.hold_key.clone();
                        egui::ComboBox::from_id_salt("hold_key_combo")
                            .width(180.0)
                            .selected_text(holdkey::label(&view.hold_key))
                            .show_ui(ui, |ui| {
                                for option in holdkey::OPTIONS {
                                    ui.selectable_value(
                                        &mut selected,
                                        option.to_string(),
                                        holdkey::label(option),
                                    );
                                }
                            });
                        if selected != view.hold_key {
                            view.hold_key = selected.clone();
                            changes.push(SettingsAction::HoldKey(selected));
                        }
                        ui.label(
                            RichText::new(format!(
                                "按住 {} 开始录音，松开结束。不要占用常用编辑键。",
                                holdkey::label(&view.hold_key)
                            ))
                            .size(11.0)
                            .color(egui::Color32::from_gray(140)),
                        );

                        ui.add_space(10.0);
                        ui.separator();
                        ui.label(RichText::new("识别方式").strong());
                        ui.horizontal(|ui| {
                            let mut online = view.online;
                            if ui
                                .selectable_value(&mut online, false, "本地 SenseVoice")
                                .changed()
                            {
                                view.online = false;
                                changes.push(SettingsAction::Online(false));
                            }
                            if ui
                                .selectable_value(&mut online, true, "在线 Qwen-ASR")
                                .changed()
                            {
                                view.online = true;
                                changes.push(SettingsAction::Online(true));
                            }
                        });

                        if view.online {
                            if field(ui, "API Key", &mut view.qwen_key, true) {
                                changes.push(SettingsAction::QwenKey(view.qwen_key.clone()));
                            }
                            if field(ui, "模型名", &mut view.qwen_model, false) {
                                changes.push(SettingsAction::QwenModel(view.qwen_model.clone()));
                            }
                            if field(ui, "WebSocket 地址", &mut view.qwen_url, false) {
                                changes.push(SettingsAction::QwenUrl(view.qwen_url.clone()));
                            }
                        } else {
                            let mut dir = view.local_dir.clone();
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut dir)
                                        .desired_width(f32::INFINITY)
                                        .hint_text("SenseVoice 模型目录"),
                                )
                                .changed()
                            {
                                view.local_dir = dir.clone();
                                changes.push(SettingsAction::LocalDir(dir));
                            }
                            ui.horizontal(|ui| {
                                if ui.button("浏览模型目录…").clicked() {
                                    view.pending_browse = Some(browse_folder(&view.local_dir));
                                }
                            });
                        }
                        if ui
                            .add_enabled(!view.checking, egui::Button::new("检查模型"))
                            .clicked()
                        {
                            view.checking = true;
                            view.status = "正在检查模型…".to_string();
                            changes.push(SettingsAction::Check);
                        }

                        ui.add_space(6.0);
                        if view.checking {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("正在检查…");
                            });
                        } else {
                            ui.label(
                                RichText::new(view.status.clone())
                                    .size(11.0)
                                    .color(egui::Color32::from_gray(150)),
                            );
                        }

                        ui.add_space(10.0);
                        ui.separator();
                        ui.label(RichText::new("输出").strong());
                        let before = view.input_mode;
                        egui::ComboBox::from_id_salt("input_mode")
                            .selected_text(view.input_mode.label()).show_ui(ui, |ui| {
                                for mode in [InputMode::DirectPrompt, InputMode::CodeEdit, InputMode::Interview] {
                                    ui.selectable_value(&mut view.input_mode, mode, mode.label());
                                }
                            });
                        if view.input_mode != before {
                            changes.push(SettingsAction::Mode(view.input_mode));
                        }
                        if view.input_mode == InputMode::Interview {
                            ui.label(format!("按住 {} 回答，每轮一个重点。说“这个先待定”跳过当前问题。出现“可以整理”后仍可补充；单独说“先整理到这里”才会输出已确定和待定事项。", holdkey::label(&view.hold_key)));
                        }
                        let mut paste = view.auto_paste;
                        if ui.checkbox(&mut paste, "识别后自动粘贴到原窗口").changed() {
                            view.auto_paste = paste;
                            changes.push(SettingsAction::AutoPaste(paste));
                        }

                        ui.add_space(12.0);
                        ui.separator();
                        ui.label(RichText::new("AI 服务").strong());
                        if view.clarifying && ui.button("取消本次补充，开始新口述").clicked()
                        {
                            changes.push(SettingsAction::CancelClarification);
                        }
                        if ui
                            .checkbox(&mut view.tts_enabled, "仅播报需要确认的问题")
                            .changed()
                        {
                            changes.push(SettingsAction::Tts(view.tts_enabled));
                        }
                        ui.label("Qwen3-TTS 1.7B · Serena · 固定风格 · 本地流式播放");
                        ui.label(&view.tts_status);
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(view.tts_enabled, egui::Button::new("试听提问"))
                                .clicked()
                            {
                                changes.push(SettingsAction::TestSpeech);
                            }
                            if ui
                                .add_enabled(view.tts_enabled, egui::Button::new("重启语音"))
                                .clicked()
                            {
                                changes.push(SettingsAction::RestartSpeech);
                            }
                        });
                        ui.add_space(8.0);
                        let mut ai_changed = false;
                        if view.input_mode != InputMode::DirectPrompt {
                            ui.label(format!("{}已启用 AI。", view.input_mode.label()));
                        } else {
                            ai_changed |= ui
                                .checkbox(&mut view.ai.enabled, "普通口述也使用 AI 整理")
                                .changed();
                        }
                        {
                            ui.label("启用 AI 时，会将转写文字和术语表发送到所选服务。");
                            let mut provider = view.ai.provider.clone();
                            egui::ComboBox::from_id_salt("ai_provider")
                                .selected_text(&provider)
                                .show_ui(ui, |ui| {
                                    for name in crate::ai::PROVIDERS {
                                        ui.selectable_value(&mut provider, name.to_string(), name);
                                    }
                                });
                            if provider != view.ai.provider {
                                view.ai.select_provider(&provider);
                                view.ai_status = "服务已切换，请填写该服务的模型和密钥。".into();
                                ai_changed = true;
                            }
                            let endpoint_changed = field(
                                ui,
                                "完整接口地址（…/v1/chat/completions）",
                                &mut view.ai.endpoint,
                                false,
                            );
                            if endpoint_changed {
                                view.ai.api_key.clear();
                                ai_changed = true;
                            }
                            ai_changed |= field(ui, "模型名称", &mut view.ai.model, false);
                            ai_changed |= field(
                                ui,
                                "API Key（Windows 用户加密保存，清空可删除）",
                                &mut view.ai.api_key,
                                true,
                            );
                            ui.label("专业术语、项目名、常用缩写");
                            ai_changed |= ui
                                .add(
                                    egui::TextEdit::multiline(&mut view.ai.glossary)
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(2),
                                )
                                .changed();
                        }
                        if ai_changed {
                            changes.push(SettingsAction::Ai(view.ai.clone()));
                        }
                        if !view.ai_status.is_empty() {
                            ui.label(&view.ai_status);
                        }
                        view.latency.show(ui);
                        if !view.raw_text.is_empty() {
                            ui.collapsing(
                                "最近一次转写与输出（可选中复制）",
                                |ui| {
                                    ui.label("原始语音转写");
                                    let mut raw = view.raw_text.as_str();
                                    ui.add(
                                        egui::TextEdit::multiline(&mut raw)
                                            .id_salt("last_raw_transcript")
                                            .desired_width(f32::INFINITY)
                                            .desired_rows(4),
                                    );
                                    ui.label("最终输出");
                                    let mut delivered = view.delivered_text.as_str();
                                    ui.add(
                                        egui::TextEdit::multiline(&mut delivered)
                                            .id_salt("last_delivered_text")
                                            .desired_width(f32::INFINITY)
                                            .desired_rows(6),
                                    );
                                },
                            );
                        }
                        ui.add_space(12.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("关闭").clicked() {
                                open = false;
                                let _ = tx.send(SettingsAction::Close);
                            }
                        });
                    });
                });

            drop(view);
            for change in changes {
                let _ = tx.send(change);
            }
            (open, focused)
        },
    )
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, password: bool) -> bool {
    ui.label(RichText::new(label).size(11.0));
    ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(f32::INFINITY)
            .password(password),
    )
    .changed()
}

fn drain_browse(view: &mut SettingsView, tx: &Sender<SettingsAction>) {
    let Some(rx) = view.pending_browse.take() else {
        return;
    };
    match rx.try_recv() {
        Ok(Some(path)) => {
            view.local_dir = path.clone();
            let _ = tx.send(SettingsAction::LocalDir(path));
        }
        Ok(None) => {}
        Err(mpsc::TryRecvError::Empty) => view.pending_browse = Some(rx),
        Err(mpsc::TryRecvError::Disconnected) => {}
    }
}

fn browse_folder(current_dir: &str) -> Receiver<Option<String>> {
    let (tx, rx) = mpsc::channel();
    let current = current_dir.to_string();
    thread::spawn(move || {
        let _ = tx.send(pick_folder(&current));
    });
    rx
}

fn pick_folder(current_dir: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'Select SenseVoice model folder'
if ($env:VIBE_MODEL_DIR -and (Test-Path -LiteralPath $env:VIBE_MODEL_DIR)) {
    $dialog.SelectedPath = $env:VIBE_MODEL_DIR
}
if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
    Write-Output $dialog.SelectedPath
}
"#;

    let output = Command::new("powershell.exe")
        .creation_flags(0x0800_0000)
        .args(["-NoProfile", "-STA", "-Command", script])
        .env("VIBE_MODEL_DIR", current_dir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }
    let folder = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!folder.is_empty()).then_some(folder)
}
