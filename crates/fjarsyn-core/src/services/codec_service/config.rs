use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub call_timeout: Duration,
    pub stop_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self { call_timeout: Duration::from_secs(10), stop_timeout: Duration::from_secs(3) }
    }
}
