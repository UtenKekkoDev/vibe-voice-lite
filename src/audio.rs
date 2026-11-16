use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU32, Ordering},
};
use std::time::Instant;
const TARGET_SAMPLE_RATE: u32 = 16_000;
static LEVEL: AtomicU32 = AtomicU32::new(0);
struct RecordingSession {
    _stream: Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
    pub sample_rate: u32,
    pub channels: u16,
    started_at: Instant,
}

pub struct RecordingCapture {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_secs: f32,
}

fn recording_slot() -> &'static Mutex<Option<RecordingSession>> {
    static SLOT: OnceLock<Mutex<Option<RecordingSession>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}
fn error_slot() -> &'static Mutex<Option<String>> {
    static SLOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}
pub fn take_recording_error() -> Option<String> {
    error_slot().lock().ok()?.take()
}
pub fn cancel_recording() {
    if let Ok(mut slot) = recording_slot().lock() {
        slot.take();
    }
    LEVEL.store(0, Ordering::Relaxed);
}
pub fn recording_level() -> f32 {
    f32::from_bits(LEVEL.load(Ordering::Relaxed))
}
pub fn start_recording() -> Result<String> {
    let slot = recording_slot();
    let mut guard = slot
        .lock()
        .map_err(|_| anyhow!("录音状态锁已损坏，建议重启程序。"))?;

    if guard.is_some() {
        return Err(anyhow!("当前已经在录音中。"));
    }

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("没有找到可用的默认麦克风。"))?;
    let device_name = device.name().unwrap_or_else(|_| "未知设备".to_string());
    let supported_config = device
        .default_input_config()
        .context("读取默认麦克风配置失败。")?;
    let sample_format = supported_config.sample_format();
    let config: StreamConfig = supported_config.config();

    let buffer = Arc::new(Mutex::new(Vec::new()));
    let error_label = device_name.clone();
    if let Ok(mut error) = error_slot().lock() {
        *error = None;
    }
    let stream = build_input_stream(
        &device,
        &config,
        sample_format,
        Arc::clone(&buffer),
        error_label,
    )?;
    stream.play().context("启动麦克风采集失败。")?;

    *guard = Some(RecordingSession {
        _stream: stream,
        buffer,
        sample_rate: config.sample_rate.0,
        channels: config.channels,
        started_at: Instant::now(),
    });

    Ok(format!(
        "正在使用默认麦克风 `{}` 录音，输入配置为 {} Hz / {} 通道。",
        device_name, config.sample_rate.0, config.channels
    ))
}

pub fn stop_recording() -> Result<RecordingCapture> {
    let slot = recording_slot();
    let mut guard = slot
        .lock()
        .map_err(|_| anyhow!("录音状态锁已损坏，建议重启程序。"))?;
    let session = guard
        .take()
        .ok_or_else(|| anyhow!("当前没有可停止的录音会话。"))?;

    let elapsed = session.started_at.elapsed().as_secs_f32();
    let _ = session._stream.pause();
    let samples = std::mem::take(
        &mut *session
            .buffer
            .lock()
            .map_err(|_| anyhow!("录音缓冲区锁已损坏。"))?,
    );

    Ok(RecordingCapture {
        samples,
        sample_rate: session.sample_rate,
        channels: session.channels,
        duration_secs: elapsed,
    })
}

fn build_input_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    buffer: Arc<Mutex<Vec<f32>>>,
    device_name: String,
) -> Result<Stream> {
    let max_samples = config.sample_rate.0 as usize * config.channels as usize * 300;
    let err_fn = move |error| {
        if let Ok(mut slot) = error_slot().lock() {
            *slot = Some(format!("麦克风 {device_name} 采集失败: {error}"));
        }
    };

    match sample_format {
        SampleFormat::F32 => device
            .build_input_stream(
                config,
                move |data: &[f32], _| push_samples(&buffer, data, max_samples, |s| *s),
                err_fn,
                None,
            )
            .context("创建 f32 录音流失败。"),
        SampleFormat::I16 => device
            .build_input_stream(
                config,
                move |data: &[i16], _| {
                    push_samples(&buffer, data, max_samples, |s| *s as f32 / 32768.0)
                },
                err_fn,
                None,
            )
            .context("创建 i16 录音流失败。"),
        SampleFormat::U16 => device
            .build_input_stream(
                config,
                move |data: &[u16], _| {
                    push_samples(&buffer, data, max_samples, |s| {
                        (*s as f32 - 32768.0) / 32768.0
                    })
                },
                err_fn,
                None,
            )
            .context("创建 u16 录音流失败。"),
        other => Err(anyhow!("暂不支持的采样格式: {:?}", other)),
    }
}

fn push_samples<T>(
    buffer: &Arc<Mutex<Vec<f32>>>,
    data: &[T],
    max_samples: usize,
    convert: impl Fn(&T) -> f32,
) {
    let mut energy = 0.0;
    if let Ok(mut guard) = buffer.lock() {
        let take = data.len().min(max_samples.saturating_sub(guard.len()));
        guard.extend(data[..take].iter().map(|s| {
            let v = convert(s);
            energy += v * v;
            v
        }));
        let level = if take == 0 {
            0.0
        } else {
            (energy / take as f32).sqrt() * 4.0
        };
        LEVEL.store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
}

pub fn resample_to_target(samples: Vec<f32>, sample_rate: u32, channels: u16) -> Vec<f32> {
    let mono = downmix_to_mono(samples, channels);
    if mono.is_empty() {
        return mono;
    }

    if sample_rate == TARGET_SAMPLE_RATE {
        return mono;
    }

    linear_resample(&mono, sample_rate, TARGET_SAMPLE_RATE)
}

fn downmix_to_mono(samples: Vec<f32>, channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples;
    }

    let channels = channels as usize;
    let mut mono = Vec::with_capacity(samples.len() / channels.max(1));
    for frame in samples.chunks(channels) {
        let sum: f32 = frame.iter().copied().sum();
        mono.push(sum / frame.len() as f32);
    }
    mono
}

fn linear_resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if samples.len() < 2 || from_rate == to_rate {
        return samples.to_vec();
    }

    let ratio = to_rate as f64 / from_rate as f64;
    let target_len = ((samples.len() as f64) * ratio).round().max(1.0) as usize;
    let mut output = Vec::with_capacity(target_len);

    for idx in 0..target_len {
        let src_pos = idx as f64 / ratio;
        let left = (src_pos.floor() as usize).min(samples.len() - 1);
        let right = (left + 1).min(samples.len() - 1);
        let frac = (src_pos - left as f64) as f32;
        let sample = samples[left] * (1.0 - frac) + samples[right] * frac;
        output.push(sample);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recording_buffer_is_bounded_and_i16_conversion_is_normalized() {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        push_samples(&buffer, &[-32768i16, 0, 32767, 10], 3, |s| {
            *s as f32 / 32768.0
        });
        push_samples(&buffer, &[100i16; 10], 3, |s| *s as f32 / 32768.0);
        let samples = buffer.lock().unwrap();
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0], -1.0);
        assert!(samples[2] < 1.0);
    }
    #[test]
    fn stereo_48k_is_downmixed_and_resampled_to_16k() {
        let audio = [0.25, 0.75].repeat(480);
        let result = resample_to_target(audio, 48000, 2);
        assert_eq!(result.len(), 160);
        assert!(result.iter().all(|s| (*s - 0.5).abs() < 0.00001));
    }
    #[test]
    fn upsampling_last_sample_does_not_index_past_buffer() {
        assert_eq!(linear_resample(&[1.0, 1.0], 8000, 16000), vec![1.0; 4]);
        assert!(resample_to_target(vec![], 48000, 2).is_empty());
    }
}
