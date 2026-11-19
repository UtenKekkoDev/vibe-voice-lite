use crate::task::TaskControl;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use transcribe_rs::{
    TranscriptionEngine,
    engines::sense_voice::{
        Language, SenseVoiceEngine, SenseVoiceInferenceParams, SenseVoiceModelParams,
    },
};

const DEFAULT_SENSEVOICE_MODEL_DIR_NAME: &str =
    "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17";

#[derive(Debug, Clone)]
pub struct SenseVoiceProbe {
    pub model_dir: PathBuf,
    pub model_file: PathBuf,
    pub tokens_txt_file: PathBuf,
    pub tokens_count: usize,
    pub source_format: &'static str,
    pub load_strategy: &'static str,
}

#[derive(Debug, Clone)]
pub struct SenseVoiceTranscript {
    pub text: String,
    pub segment_count: usize,
}

pub fn default_model_dir() -> PathBuf {
    let release_default = current_exe_dir()
        .join("models")
        .join(DEFAULT_SENSEVOICE_MODEL_DIR_NAME);
    if release_default.exists() {
        return release_default;
    }

    let release_official_models = current_exe_dir()
        .join("official-models")
        .join(DEFAULT_SENSEVOICE_MODEL_DIR_NAME);
    if release_official_models.exists() {
        return release_official_models;
    }

    let development = development_model_dir();
    if development.is_dir() {
        development
    } else {
        release_default
    }
}

pub fn prepare_and_probe(model_dir: &Path) -> Result<SenseVoiceProbe> {
    let model_dir = canonicalize_or_original(model_dir);
    let tokens_txt_file = model_dir.join("tokens.txt");
    let official_int8 = model_dir.join("model.int8.onnx");
    let official_fp32 = model_dir.join("model.onnx");
    let legacy_config = model_dir.join("config.yaml");
    let legacy_tokens_json = model_dir.join("tokens.json");

    if official_int8.exists() && tokens_txt_file.exists() {
        let tokens_count = count_tokens_from_txt(&tokens_txt_file)?;
        try_load_model(&model_dir, SenseVoiceModelParams::int8())?;

        return Ok(SenseVoiceProbe {
            model_dir,
            model_file: official_int8,
            tokens_txt_file,
            tokens_count,
            source_format: "官方 sherpa-onnx SenseVoice int8",
            load_strategy: "INT8 (`model.int8.onnx`) + 直接使用 `tokens.txt`",
        });
    }

    if official_fp32.exists() && tokens_txt_file.exists() {
        let tokens_count = count_tokens_from_txt(&tokens_txt_file)?;
        try_load_model(&model_dir, SenseVoiceModelParams::fp32())?;
        return Ok(SenseVoiceProbe {
            model_dir,
            model_file: official_fp32,
            tokens_txt_file,
            tokens_count,
            source_format: "官方 SenseVoice FP32",
            load_strategy: "FP32 + tokens.txt",
        });
    }
    ensure_file_exists(&official_fp32)?;
    ensure_file_exists(&legacy_config)?;
    ensure_file_exists(&legacy_tokens_json)?;

    let tokens = load_tokens_from_json(&legacy_tokens_json)?;
    if tokens.is_empty() {
        bail!("`tokens.json` 为空，无法生成 `tokens.txt`。");
    }

    write_tokens_txt(&tokens_txt_file, &tokens)?;
    try_load_model(&model_dir, SenseVoiceModelParams::fp32())?;

    Ok(SenseVoiceProbe {
        model_dir,
        model_file: official_fp32,
        tokens_txt_file,
        tokens_count: tokens.len(),
        source_format: "旧 FunASR 风格目录",
        load_strategy: "FP32 (`model.onnx`) + 自动生成 `tokens.txt`",
    })
}

pub fn format_probe_summary(probe: &SenseVoiceProbe) -> String {
    [
        format!("模型目录: {}", probe.model_dir.display()),
        format!("模型文件: {}", probe.model_file.display()),
        format!("目录类型: {}", probe.source_format),
        format!("兼容词表: {}", probe.tokens_txt_file.display()),
        format!("token 数量: {}", probe.tokens_count),
        format!("加载策略: {}", probe.load_strategy),
    ]
    .join("\n")
}

pub fn transcribe_audio(
    model_dir: &Path,
    samples: Vec<f32>,
    task: &TaskControl,
) -> Result<SenseVoiceTranscript> {
    task.check()?;
    let model_dir = canonicalize_or_original(model_dir);
    let params = detect_model_params(&model_dir)?;
    let mut cache = engine_cache()
        .lock()
        .map_err(|_| anyhow::anyhow!("模型工作线程异常，请重启"))?;
    load_cached(&mut cache, &model_dir, params)?;
    let engine = &mut cache.as_mut().expect("loaded model").1;

    let mut text = Vec::new();
    // Bound ONNX memory and cancellation latency. Prefer low-energy pauses near each limit.
    for range in chunk_ranges(&samples) {
        task.check()?;
        let chunk = &samples[range];
        if chunk.iter().all(|sample| sample.abs() < 0.0001) {
            continue;
        }
        let result = engine
            .transcribe_samples(
                chunk.to_vec(),
                Some(SenseVoiceInferenceParams {
                    language: Language::Auto,
                    use_itn: true,
                }),
            )
            .map_err(|error| anyhow::anyhow!("SenseVoice 转写失败: {error}"))?;
        task.check()?;
        if !result.text.trim().is_empty() {
            text.push(result.text.trim().to_owned());
        }
    }
    Ok(SenseVoiceTranscript {
        segment_count: text.len(),
        text: text.join(" "),
    })
}

fn chunk_ranges(samples: &[f32]) -> Vec<std::ops::Range<usize>> {
    const LIMIT: usize = 20 * 16_000;
    const WINDOW: usize = 1_600;
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < samples.len() {
        let mut end = (start + LIMIT).min(samples.len());
        if end < samples.len() {
            let search = end - 3 * 16_000;
            if let Some((index, _)) = samples[search..end]
                .as_chunks::<WINDOW>()
                .0
                .iter()
                .enumerate()
                .map(|(i, block)| (i, block.iter().map(|s| s * s).sum::<f32>()))
                .min_by(|a, b| a.1.total_cmp(&b.1))
            {
                end = search + index * WINDOW + WINDOW / 2;
            }
        }
        ranges.push(start..end);
        start = end;
    }
    ranges
}

fn ensure_file_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("缺少文件: {}", path.display());
    }

    Ok(())
}

fn load_tokens_from_json(path: &Path) -> Result<Vec<String>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("读取 `{}` 失败。", path.display()))?;
    let tokens: Vec<String> = serde_json::from_str(&content)
        .with_context(|| format!("解析 `{}` 失败，预期为字符串数组。", path.display()))?;
    Ok(tokens)
}

fn count_tokens_from_txt(path: &Path) -> Result<usize> {
    let content =
        fs::read_to_string(path).with_context(|| format!("读取 `{}` 失败。", path.display()))?;
    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count())
}

fn write_tokens_txt(path: &Path, tokens: &[String]) -> Result<()> {
    let mut content = String::with_capacity(tokens.len() * 12);
    for (idx, token) in tokens.iter().enumerate() {
        content.push_str(token);
        content.push(' ');
        content.push_str(&idx.to_string());
        content.push('\n');
    }

    let should_write = match fs::read_to_string(path) {
        Ok(existing) => existing != content,
        Err(_) => true,
    };

    if should_write {
        fs::write(path, content).with_context(|| format!("写入 `{}` 失败。", path.display()))?;
    }

    Ok(())
}

type EngineCache = Option<(PathBuf, SenseVoiceEngine)>;
fn engine_cache() -> &'static Mutex<EngineCache> {
    static CACHE: OnceLock<Mutex<EngineCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}
fn load_cached(cache: &mut EngineCache, dir: &Path, params: SenseVoiceModelParams) -> Result<()> {
    if cache.as_ref().is_some_and(|(path, _)| path == dir) {
        return Ok(());
    }
    *cache = None;
    let mut engine = SenseVoiceEngine::new();
    engine
        .load_model_with_params(dir, params)
        .map_err(|e| anyhow::anyhow!("加载 SenseVoice 失败: {e}"))?;
    *cache = Some((dir.to_owned(), engine));
    Ok(())
}
fn try_load_model(model_dir: &Path, params: SenseVoiceModelParams) -> Result<()> {
    let mut cache = engine_cache()
        .lock()
        .map_err(|_| anyhow::anyhow!("模型锁异常"))?;
    load_cached(&mut cache, model_dir, params)
}

fn detect_model_params(model_dir: &Path) -> Result<SenseVoiceModelParams> {
    let int8_path = model_dir.join("model.int8.onnx");
    let fp32_path = model_dir.join("model.onnx");

    if int8_path.exists() {
        Ok(SenseVoiceModelParams::int8())
    } else if fp32_path.exists() {
        Ok(SenseVoiceModelParams::fp32())
    } else {
        bail!(
            "在 `{}` 下找不到 `model.int8.onnx` 或 `model.onnx`。",
            model_dir.display()
        );
    }
}

fn canonicalize_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn current_exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn development_model_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .unwrap_or(manifest_dir.as_path())
        .join("official-models")
        .join(DEFAULT_SENSEVOICE_MODEL_DIR_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_audio_is_bounded_and_split_near_pause_without_losing_samples() {
        let mut audio = vec![0.5; 45 * 16000];
        audio[18 * 16000..19 * 16000].fill(0.0);
        let ranges = chunk_ranges(&audio);
        assert!(ranges[0].end >= 18 * 16000 && ranges[0].end <= 19 * 16000);
        assert_eq!(ranges.iter().map(|r| r.len()).sum::<usize>(), audio.len());
        assert!(ranges.iter().all(|r| r.len() <= 20 * 16000));
        assert!(ranges.windows(2).all(|r| r[0].end == r[1].start));
    }
    #[test]
    #[ignore = "requires VIBE_TEST_MODEL with the official model and test_wavs/zh.wav"]
    fn official_model_transcribes_twice_with_cached_engine() {
        let dir = PathBuf::from(std::env::var_os("VIBE_TEST_MODEL").expect("set VIBE_TEST_MODEL"));
        let mut wav = hound::WavReader::open(dir.join("test_wavs/zh.wav")).unwrap();
        assert_eq!(wav.spec().sample_rate, 16000);
        assert_eq!(wav.spec().channels, 1);
        let samples: Vec<f32> = wav
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / 32768.0)
            .collect();
        let start = std::time::Instant::now();
        prepare_and_probe(&dir).unwrap();
        eprintln!("model load: {:?}", start.elapsed());
        let start = std::time::Instant::now();
        let task = TaskControl::new(std::time::Duration::from_secs(180));
        let first = transcribe_audio(&dir, samples.clone(), &task).unwrap();
        eprintln!(
            "first inference: {:?}; transcript: {}",
            start.elapsed(),
            first.text
        );
        let start = std::time::Instant::now();
        let second = transcribe_audio(&dir, samples, &task).unwrap();
        eprintln!("cached inference: {:?}", start.elapsed());
        assert!(!first.text.is_empty());
        assert_eq!(first.text, second.text);
    }
}
