use std::time::Duration;

use super::super::Config;

#[test]
fn production_deadlines_are_explicit() {
    let config = Config::default();
    assert_eq!(config.call_timeout, Duration::from_secs(10));
    assert_eq!(config.stop_timeout, Duration::from_secs(3));
}
