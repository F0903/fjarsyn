use fjarsyn_core::config::Config;

#[derive(Debug, Clone)]
pub enum ConfigMessage {
    SaveRequested(Config),
}
