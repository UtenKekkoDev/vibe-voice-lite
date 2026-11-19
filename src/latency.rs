//! Recent in-memory latency samples. No audio, text, addresses or credentials.
use crate::state::InputMode;
use crate::task::TaskControl;
use egui::Ui;
use std::{
    collections::VecDeque,
    sync::OnceLock,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

pub struct ReleaseTime {
    instant: Instant,
    counter: i64,
}

impl ReleaseTime {
    pub fn now() -> Self {
        let instant = Instant::now();
        let mut counter = 0;
        unsafe {
            QueryPerformanceCounter(&mut counter);
        }
        Self { instant, counter }
    }
}

fn counter_elapsed(start: i64, end: i64, frequency: i64) -> Option<Duration> {
    if frequency <= 0 || start <= 0 || end < start {
        return None;
    }
    Some(Duration::from_secs_f64(
        (end - start) as f64 / frequency as f64,
    ))
}

pub struct OutputMark {
    pub request: u64,
    pub counter: i64,
    pub worker_first: bool,
}

struct Turn {
    number: u64,
    mode: InputMode,
    followup: bool,
    released: ReleaseTime,
    task: TaskControl,
    speech_request: Option<u64>,
    first_output: Option<(Duration, bool)>,
}

#[derive(Default)]
pub struct LatencyHistory {
    turns: VecDeque<Turn>,
}

impl LatencyHistory {
    pub fn begin(
        &mut self,
        released: ReleaseTime,
        mode: InputMode,
        followup: bool,
        task: &TaskControl,
    ) {
        let number = self.turns.back().map_or(1, |last| last.number + 1);
        if self.turns.len() == 8 {
            self.turns.pop_front();
        }
        self.turns.push_back(Turn {
            number,
            mode,
            followup,
            released,
            task: task.clone(),
            speech_request: None,
            first_output: None,
        });
    }

    pub fn attach_speech(&mut self, request: u64) {
        if let Some(turn) = self.turns.back_mut() {
            turn.speech_request = Some(request);
        }
    }

    pub fn first_output(&mut self, mark: OutputMark) {
        if let Some(turn) = self
            .turns
            .iter_mut()
            .find(|t| t.speech_request == Some(mark.request))
            && turn.first_output.is_none()
        {
            static FREQUENCY: OnceLock<i64> = OnceLock::new();
            let frequency = FREQUENCY.get_or_init(|| {
                let mut value = 0;
                unsafe {
                    QueryPerformanceFrequency(&mut value);
                }
                value
            });
            turn.first_output = counter_elapsed(turn.released.counter, mark.counter, *frequency)
                .map(|delay| (delay, mark.worker_first));
        }
    }

    pub fn show(&self, ui: &mut Ui) {
        ui.collapsing("本次运行耗时", |ui| {
            ui.label("从检测到松键起累计计时，单位毫秒。仅保留最近 8 轮，退出后清空。");
            egui::Grid::new("turn_latencies").striped(true).show(ui, |ui| {
                for name in ["轮次", "识别完成", "AI 返回", "播放首音"] { ui.strong(name); }
                ui.end_row();
                for turn in self.turns.iter().rev() {
                    let label = match (turn.mode, turn.followup) {
                        (InputMode::Interview, true) => "访谈续答",
                        (InputMode::Interview, false) => "访谈首轮",
                        (_, true) => "补充回答",
                        (mode, _) => mode.label(),
                    };
                    ui.label(format!("#{} {label}", turn.number));
                    let elapsed = |at: &Instant| at.saturating_duration_since(turn.released.instant);
                    for value in [turn.task.asr_finished.get().map(elapsed), turn.task.ai_received.get().map(elapsed)] {
                        ui.label(value.map_or_else(|| "—".to_owned(), |d| d.as_millis().to_string()));
                    }
                    ui.label(turn.first_output.map_or_else(|| "—".into(), |(delay, first)| {
                        format!("{} · {}", delay.as_millis(), if first { "首播" } else { "后续" })
                    }));
                    ui.end_row();
                }
            });
            ui.small("AI 返回计至收到完整响应。播放首音计至设备输出回调写入超过 −60 dBFS 的音频，扬声器及蓝牙另有延迟。首播指语音后台此次启动后第一次输出，即使已预热也标为首播。空栏表示该阶段尚无记录。");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_audio_updates_its_own_turn_once_and_history_is_bounded() {
        let task = TaskControl::new(Duration::from_secs(1));
        let mut history = LatencyHistory::default();
        for request in 1..=9 {
            history.begin(ReleaseTime::now(), InputMode::Interview, request > 1, &task);
            history.attach_speech(request);
        }
        assert_eq!(history.turns.len(), 8);
        let counter = history.turns[0].released.counter;
        let mark = |request, counter| OutputMark {
            request,
            counter,
            worker_first: true,
        };
        history.first_output(mark(1, counter + 10));
        history.first_output(mark(2, counter - 1));
        assert!(history.turns[0].first_output.is_none());
        history.first_output(mark(2, counter));
        history.first_output(mark(2, counter + 10));
        assert_eq!(history.turns[0].first_output, Some((Duration::ZERO, true)));
        assert!(history.turns.back().unwrap().first_output.is_none());
        assert_eq!(
            counter_elapsed(50, 150, 1000),
            Some(Duration::from_millis(100))
        );
    }
}
