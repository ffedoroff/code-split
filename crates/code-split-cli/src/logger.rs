use chrono::Local;
use std::time::Instant;

fn ts() -> String {
    Local::now().format("%H:%M:%S%.3f").to_string()
}

pub fn info(msg: &str) {
    eprintln!("[{}] {}", ts(), msg);
}

pub struct Timer {
    label: String,
    start: Instant,
}

impl Timer {
    pub fn start(label: &str) -> Self {
        eprintln!("[{}] ▶ {}", ts(), label);
        Self {
            label: label.to_string(),
            start: Instant::now(),
        }
    }

    pub fn finish_with(self, extra: &str) -> u64 {
        let ms = self.start.elapsed().as_millis() as u64;
        eprintln!(
            "[{}] ✓ {} — {:.1}s{}",
            ts(),
            self.label,
            ms as f64 / 1000.0,
            if extra.is_empty() {
                String::new()
            } else {
                format!(" ({})", extra)
            }
        );
        ms
    }

    pub fn finish(self) -> u64 {
        self.finish_with("")
    }
}
