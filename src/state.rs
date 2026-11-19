use crate::sensevoice;

const DEFAULT_QWEN_MODEL: &str = "qwen3-asr-flash-realtime";
const DEFAULT_QWEN_URL: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/realtime";
const DEFAULT_QWEN_LANGUAGE: &str = "zh";
const DEFAULT_QWEN_STATUS: &str = "尚未配置 Qwen-ASR Realtime。";

const DEFAULT_STATUS_MESSAGE: &str = "按住触发键说话。";
const DEFAULT_MODEL_STATUS: &str = "尚未检查 SenseVoice 本地模型。";
const DEFAULT_MODEL_SUMMARY: &str = "当前默认指向官方 sherpa-onnx SenseVoice int8 目录；如果你切回旧 FunASR 风格目录，程序也会尝试把 `tokens.json` 转成 `tokens.txt` 再探测加载。";
const DEFAULT_RECORDING_INFO: &str = "尚未开始真实录音。";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    CodeEdit,
    DirectPrompt,
    Interview,
}

impl InputMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::DirectPrompt => "普通口述",
            Self::CodeEdit => "代码整理",
            Self::Interview => "需求访谈",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelProvider {
    Local,
    OnlineQwen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputState {
    Idle,
    Recording,
    Processing,
    Success,
    Error,
}

#[derive(Clone)]
pub struct NativeAppState {
    pub tts_enabled: bool,
    pub clarification: Option<crate::ai::Clarification>,
    pub ai: crate::ai::AiConfig,
    pub ai_status: String,
    /// Physical key held down to dictate, e.g. `RightCtrl`.
    pub hold_key: String,
    pub input_mode: InputMode,
    pub input_state: InputState,
    pub status_message: String,
    pub raw_text: String,
    pub delivered_text: String,
    /// Completed output available from the tray until this process exits.
    pub last_final_result: String,
    pub delivery_target: Option<crate::services::DeliveryTarget>,
    pub auto_paste: bool,
    pub model_provider: ModelProvider,
    pub local_model_dir: String,
    pub local_model_ready: bool,
    pub local_model_status: String,
    pub local_model_summary: String,
    pub qwen_api_key: String,
    pub qwen_model: String,
    pub qwen_url: String,
    pub qwen_language: String,
    pub qwen_ready: bool,
    pub qwen_status: String,
    pub last_recording_info: String,
}

impl NativeAppState {
    pub fn model_ready(&self) -> bool {
        match self.model_provider {
            ModelProvider::Local => self.local_model_ready,
            ModelProvider::OnlineQwen => self.qwen_ready,
        }
    }
}

impl Default for NativeAppState {
    fn default() -> Self {
        Self {
            tts_enabled: true,
            clarification: None,
            ai: crate::ai::AiConfig::default(),
            ai_status: String::new(),
            hold_key: "RightCtrl".to_string(),
            input_mode: InputMode::DirectPrompt,
            input_state: InputState::Idle,
            status_message: DEFAULT_STATUS_MESSAGE.to_string(),
            raw_text: String::new(),
            delivered_text: String::new(),
            last_final_result: String::new(),
            delivery_target: None,
            auto_paste: true,
            model_provider: ModelProvider::Local,
            local_model_dir: sensevoice::default_model_dir()
                .to_string_lossy()
                .into_owned(),
            local_model_ready: false,
            local_model_status: DEFAULT_MODEL_STATUS.to_string(),
            local_model_summary: DEFAULT_MODEL_SUMMARY.to_string(),
            qwen_api_key: String::new(),
            qwen_model: DEFAULT_QWEN_MODEL.to_string(),
            qwen_url: DEFAULT_QWEN_URL.to_string(),
            qwen_language: DEFAULT_QWEN_LANGUAGE.to_string(),
            qwen_ready: false,
            qwen_status: DEFAULT_QWEN_STATUS.to_string(),
            last_recording_info: DEFAULT_RECORDING_INFO.to_string(),
        }
    }
}
