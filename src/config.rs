use crate::state::{InputMode, ModelProvider, NativeAppState};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

fn path() -> PathBuf {
    std::env::var_os("VIBE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("VibeVoiceLite"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("VibeVoiceLite"))
        .join("settings.json")
}

pub fn has_saved_settings() -> bool {
    path().is_file()
}

pub fn snapshot(s: &NativeAppState) -> Value {
    json!({"version":2, "hold_key":s.hold_key, "auto_paste":s.auto_paste,
        "code_mode":s.input_mode == InputMode::CodeEdit,
        "interview_mode":s.input_mode == InputMode::Interview,
        "online":s.model_provider == ModelProvider::OnlineQwen,
        "local_model_dir":s.local_model_dir, "qwen_model":s.qwen_model,
        "qwen_url":s.qwen_url, "qwen_language":s.qwen_language,
        "ai_enabled":s.ai.enabled, "ai_provider":s.ai.provider, "ai_endpoint":s.ai.endpoint,
        "ai_model":s.ai.model, "ai_glossary":s.ai.glossary,
        "ai_key_protected":s.ai.protected_key, "tts_enabled":s.tts_enabled})
}

pub fn apply(s: &mut NativeAppState, v: &Value) {
    s.tts_enabled = v["tts_enabled"].as_bool().unwrap_or(true);
    for (key, field) in [
        ("ai_endpoint", &mut s.ai.endpoint),
        ("ai_model", &mut s.ai.model),
        ("ai_glossary", &mut s.ai.glossary),
        ("ai_key_protected", &mut s.ai.protected_key),
        ("hold_key", &mut s.hold_key),
        ("local_model_dir", &mut s.local_model_dir),
        ("qwen_model", &mut s.qwen_model),
        ("qwen_url", &mut s.qwen_url),
        ("qwen_language", &mut s.qwen_language),
    ] {
        if let Some(value) = v.get(key).and_then(Value::as_str) {
            *field = value.to_owned();
        }
    }
    s.ai.enabled = v["ai_enabled"].as_bool().unwrap_or(false);
    if let Some(provider) = v["ai_provider"].as_str() {
        s.ai.provider = provider.into();
    } else if !s.ai.endpoint.trim().is_empty() {
        s.ai.provider = match url::Url::parse(&s.ai.endpoint)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .as_deref()
        {
            Some("api.deepseek.com") => "DeepSeek",
            Some("api.openai.com") => "OpenAI",
            Some("localhost" | "127.0.0.1") => "Ollama",
            _ => "自定义兼容接口",
        }
        .into();
    } else {
        let defaults = crate::ai::AiConfig::default();
        s.ai.provider = defaults.provider;
        s.ai.endpoint = defaults.endpoint;
        if s.ai.model.trim().is_empty() {
            s.ai.model = defaults.model;
        }
    }
    if let Some(value) = v.get("auto_paste").and_then(Value::as_bool) {
        s.auto_paste = value;
    }
    s.input_mode = if v["interview_mode"].as_bool() == Some(true) {
        InputMode::Interview
    } else if v["code_mode"].as_bool() == Some(true) {
        InputMode::CodeEdit
    } else {
        InputMode::DirectPrompt
    };
    s.model_provider = if v["online"].as_bool() == Some(true) {
        ModelProvider::OnlineQwen
    } else {
        ModelProvider::Local
    };
}

pub fn load(s: &mut NativeAppState) -> Result<()> {
    let path = path();
    if !path.exists() {
        return Ok(());
    }
    let v: Value = serde_json::from_slice(&fs::read(path)?).context("读取设置失败")?;
    apply(s, &v);
    s.ai.api_key = crate::credentials::unprotect(&s.ai.protected_key, &s.ai.endpoint)?;
    // Preserve intentional custom paths; only recover an unavailable old location.
    if !std::path::Path::new(&s.local_model_dir).is_dir() {
        let bundled = crate::sensevoice::default_model_dir();
        if bundled.join("tokens.txt").is_file()
            && (bundled.join("model.int8.onnx").is_file() || bundled.join("model.onnx").is_file())
        {
            s.local_model_dir = bundled.to_string_lossy().into_owned();
        }
    }
    Ok(())
}

pub fn save(v: &Value) -> Result<()> {
    let path = path();
    fs::create_dir_all(path.parent().expect("settings parent"))?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp, serde_json::to_vec_pretty(v)?)?;
    // Windows rename cannot overwrite an existing target. ReplaceFileW preserves the old file on failure.
    if path.exists() {
        use std::os::windows::ffi::OsStrExt;
        let wide = |p: &std::path::Path| {
            p.as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>()
        };
        let target = wide(&path);
        let source = wide(&temp);
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::ReplaceFileW(
                target.as_ptr(),
                source.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
    } else {
        fs::rename(temp, path)?;
    }
    Ok(())
}

/// Lifecycle diagnostics contain no audio, transcript, or credentials.
pub fn lifecycle(event: &str) {
    use std::io::Write;
    let log = path().with_file_name("lifecycle.log");
    let _ = fs::create_dir_all(log.parent().expect("log parent"));
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(log) {
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(file, "{time} pid={} {event}", std::process::id());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn interview_mode_persists_and_legacy_code_mode_still_loads() {
        let mut state = crate::state::NativeAppState::default();
        state.input_mode = crate::state::InputMode::Interview;
        let saved = super::snapshot(&state);
        let mut loaded = crate::state::NativeAppState::default();
        super::apply(&mut loaded, &saved);
        assert_eq!(loaded.input_mode, crate::state::InputMode::Interview);
        super::apply(&mut loaded, &serde_json::json!({"code_mode":true}));
        assert_eq!(loaded.input_mode, crate::state::InputMode::CodeEdit);
    }
    use super::*;
    #[test]
    fn encrypted_key_survives_process_restart_and_can_be_deleted() {
        const TEST_KEY: &str = "test-only-restart-secret";
        if let Ok(phase) = std::env::var("VIBE_KEY_TEST_PHASE") {
            match phase.as_str() {
                "write" => {
                    assert!(!has_saved_settings());
                    let mut state = NativeAppState::default();
                    state.ai.protected_key =
                        crate::credentials::protect(TEST_KEY, &state.ai.endpoint).unwrap();
                    save(&snapshot(&state)).unwrap();
                    assert!(!fs::read_to_string(path()).unwrap().contains(TEST_KEY));
                }
                "read" => {
                    assert!(has_saved_settings());
                    let mut state = NativeAppState::default();
                    load(&mut state).unwrap();
                    assert_eq!(state.ai.api_key, TEST_KEY);
                }
                "clear" => {
                    let mut state = NativeAppState::default();
                    load(&mut state).unwrap();
                    state.ai.api_key.clear();
                    state.ai.protected_key =
                        crate::credentials::protect("", &state.ai.endpoint).unwrap();
                    save(&snapshot(&state)).unwrap();
                }
                "read-empty" => {
                    let mut state = NativeAppState::default();
                    load(&mut state).unwrap();
                    assert!(state.ai.api_key.is_empty());
                    assert!(state.ai.protected_key.is_empty());
                }
                _ => panic!("unknown test phase"),
            }
            return;
        }
        let dir =
            std::env::temp_dir().join(format!("vibe-key-restart-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for phase in ["write", "read", "clear", "read-empty"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "config::tests::encrypted_key_survives_process_restart_and_can_be_deleted",
                ])
                .env("VIBE_CONFIG_DIR", &dir)
                .env("VIBE_KEY_TEST_PHASE", phase)
                .output()
                .unwrap();
            assert!(output.status.success(), "restart test phase {phase} failed");
        }
        fs::remove_file(dir.join("settings.json")).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn old_ai_settings_migrate_without_overwriting_custom_endpoints() {
        let mut state = NativeAppState::default();
        apply(
            &mut state,
            &json!({"ai_endpoint":"","ai_model":"","code_mode":true}),
        );
        assert_eq!(state.ai.provider, "DeepSeek");
        assert_eq!(state.ai.model, "deepseek-flash");
        assert!(state.ai.active_for(state.input_mode));
        apply(
            &mut state,
            &json!({"ai_endpoint":"https://example.com/custom/chat/completions","ai_model":"my-model"}),
        );
        assert_eq!(state.ai.provider, "自定义兼容接口");
        assert_eq!(state.ai.model, "my-model");
        assert_eq!(
            state.ai.endpoint,
            "https://example.com/custom/chat/completions"
        );
    }
    #[test]
    fn settings_roundtrip_excludes_secrets_and_transcripts() {
        let s = NativeAppState {
            ai: crate::ai::AiConfig {
                enabled: true,
                api_key: "ai-secret-key".into(),
                model: "test-model".into(),
                endpoint: "https://example.com/v1/chat/completions".into(),
                glossary: "Rust, egui".into(),
                ..Default::default()
            },
            qwen_api_key: "secret-key".into(),
            raw_text: "private speech".into(),
            hold_key: "F10".into(),
            auto_paste: false,
            ..Default::default()
        };
        let v = snapshot(&s);
        let serialized = v.to_string();
        assert!(!serialized.contains("secret-key"));
        assert!(!serialized.contains("private speech"));
        assert!(!serialized.contains("ai-secret-key"));
        let mut restored = NativeAppState::default();
        apply(&mut restored, &v);
        assert!(restored.ai.enabled);
        assert_eq!(restored.ai.model, "test-model");
        assert!(restored.ai.api_key.is_empty());
        apply(&mut restored, &json!({"version": 2}));
        assert!(!restored.ai.enabled);
        let mut restored = NativeAppState::default();
        apply(&mut restored, &v);
        assert_eq!(restored.hold_key, "F10");
        assert!(!restored.auto_paste);
        assert!(restored.qwen_api_key.is_empty());
    }
}
