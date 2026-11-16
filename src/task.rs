use anyhow::{Result, bail};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct TaskControl {
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl TaskControl {
    pub fn new(timeout: Duration) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + timeout,
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            bail!("操作已取消。");
        }
        if Instant::now() >= self.deadline {
            bail!("操作超时，请检查网络或模型后重试。");
        }
        Ok(())
    }
    pub fn remaining(&self) -> Result<Duration> {
        self.check()?;
        Ok(self.deadline.saturating_duration_since(Instant::now()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_reaches_worker_clone() {
        let task = TaskControl::new(Duration::from_secs(1));
        let worker = task.clone();
        task.cancel();
        assert!(worker.check().is_err());
    }
    #[test]
    fn expired_task_cannot_continue() {
        assert!(TaskControl::new(Duration::ZERO).check().is_err());
    }
}
