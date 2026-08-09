use std::time::Duration;

use chrono::Duration as ChronoDuration;

use crate::peer_session::{Error, negotiation};

#[derive(Debug, Clone)]
pub(crate) struct Limits {
    pub(crate) max_sessions: usize,
    pub(crate) service_command_capacity: usize,
    pub(crate) session_command_capacity: usize,
    pub(crate) session_update_capacity: usize,
    pub(crate) event_capacity: usize,
    pub(crate) video_input_capacity: usize,
    pub(crate) remote_video_capacity: usize,
    pub(crate) max_message_bytes: usize,
    pub(crate) max_data_message_bytes: usize,
    pub(crate) max_signaling_frame_bytes: usize,
    pub(crate) signaling_queue_capacity: usize,
    pub(crate) signaling_replay_capacity: usize,
    pub(crate) max_signaling_connections: usize,
    pub(crate) max_signaling_connections_per_ip: usize,
    pub(crate) signaling_auth_global_burst: usize,
    pub(crate) signaling_auth_global_refill_interval: Duration,
    pub(crate) signaling_auth_per_ip_burst: usize,
    pub(crate) signaling_auth_per_ip_refill_interval: Duration,
    pub(crate) max_signaling_auth_tracked_ips: usize,
    pub(crate) max_endpoint_attempts: usize,
    pub(crate) endpoint_attempt_timeout: Duration,
    pub(crate) signaling_handshake_timeout: Duration,
    pub(crate) signaling_idle_timeout: Duration,
    pub(crate) signaling_max_message_age: Duration,
    pub(crate) signaling_max_clock_skew: Duration,
    pub(crate) max_ice_candidates_per_generation: usize,
    pub(crate) request_timeout: Duration,
    pub(crate) negotiation_timeout: Duration,
    pub(crate) shutdown_timeout: Duration,
    pub(crate) event_delivery_timeout: Duration,
    pub(crate) pre_ready_data_capacity: usize,
    pub(crate) service_operation_timeout: Duration,
    pub(crate) disconnected_grace: Duration,
    pub(crate) ice_restart_timeout: Duration,
    pub(crate) rtc_operation_timeout: Duration,
    pub(crate) max_remote_timestamp_age: Duration,
    pub(crate) max_remote_clock_skew: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sessions: 8,
            service_command_capacity: 64,
            session_command_capacity: 64,
            session_update_capacity: 256,
            event_capacity: 256,
            video_input_capacity: 3,
            // Keep the initial SPS/PPS/IDR window available while the
            // authenticated ShareStarted projection reaches desktop media.
            remote_video_capacity: 64,
            max_message_bytes: 12 * 1024,
            max_data_message_bytes: 16 * 1024,
            max_signaling_frame_bytes: 256 * 1024,
            signaling_queue_capacity: 64,
            signaling_replay_capacity: 4096,
            max_signaling_connections: 32,
            max_signaling_connections_per_ip: 4,
            signaling_auth_global_burst: 64,
            signaling_auth_global_refill_interval: Duration::from_millis(100),
            signaling_auth_per_ip_burst: 8,
            signaling_auth_per_ip_refill_interval: Duration::from_millis(500),
            max_signaling_auth_tracked_ips: 4096,
            max_endpoint_attempts: 6,
            endpoint_attempt_timeout: Duration::from_secs(2),
            signaling_handshake_timeout: Duration::from_secs(10),
            signaling_idle_timeout: Duration::from_secs(60),
            signaling_max_message_age: Duration::from_secs(5 * 60),
            signaling_max_clock_skew: Duration::from_secs(30),
            max_ice_candidates_per_generation: 256,
            request_timeout: Duration::from_secs(30),
            negotiation_timeout: Duration::from_secs(45),
            shutdown_timeout: Duration::from_secs(5),
            event_delivery_timeout: Duration::from_secs(2),
            pre_ready_data_capacity: 32,
            service_operation_timeout: Duration::from_secs(15),
            disconnected_grace: Duration::from_secs(5),
            ice_restart_timeout: Duration::from_secs(20),
            rtc_operation_timeout: Duration::from_secs(2),
            max_remote_timestamp_age: Duration::from_secs(5 * 60),
            max_remote_clock_skew: Duration::from_secs(30),
        }
    }
}

pub(super) fn negotiation_limits(limits: &Limits) -> Result<negotiation::Limits, Error> {
    if limits.ice_restart_timeout.is_zero() {
        return Err(Error::InvalidLimit { name: "ice_restart_timeout" });
    }
    if limits.signaling_auth_global_burst == 0 {
        return Err(Error::InvalidLimit { name: "signaling_auth_global_burst" });
    }
    if limits.signaling_auth_global_refill_interval.is_zero() {
        return Err(Error::InvalidLimit { name: "signaling_auth_global_refill_interval" });
    }
    if limits.signaling_auth_per_ip_burst == 0 {
        return Err(Error::InvalidLimit { name: "signaling_auth_per_ip_burst" });
    }
    if limits.signaling_auth_per_ip_refill_interval.is_zero() {
        return Err(Error::InvalidLimit { name: "signaling_auth_per_ip_refill_interval" });
    }
    if limits.max_signaling_auth_tracked_ips == 0 {
        return Err(Error::InvalidLimit { name: "max_signaling_auth_tracked_ips" });
    }
    let max_message_age = ChronoDuration::from_std(limits.signaling_max_message_age)
        .map_err(|_| Error::Protocol("invalid signaling max age".into()))?;
    let max_clock_skew = ChronoDuration::from_std(limits.signaling_max_clock_skew)
        .map_err(|_| Error::Protocol("invalid signaling clock skew".into()))?;
    Ok(negotiation::Limits {
        max_frame_bytes: limits.max_signaling_frame_bytes.max(1024),
        queue_capacity: limits.signaling_queue_capacity.max(1),
        replay_capacity: limits.signaling_replay_capacity.max(1),
        max_connections: limits.max_signaling_connections.max(1),
        max_connections_per_ip: limits.max_signaling_connections_per_ip.max(1),
        authentication_global_burst: limits.signaling_auth_global_burst,
        authentication_global_refill_interval: limits.signaling_auth_global_refill_interval,
        authentication_per_ip_burst: limits.signaling_auth_per_ip_burst,
        authentication_per_ip_refill_interval: limits.signaling_auth_per_ip_refill_interval,
        max_authentication_tracked_ips: limits.max_signaling_auth_tracked_ips,
        max_endpoint_attempts: limits.max_endpoint_attempts,
        endpoint_attempt_timeout: limits.endpoint_attempt_timeout,
        handshake_timeout: limits.signaling_handshake_timeout,
        idle_timeout: limits.signaling_idle_timeout,
        max_message_age,
        max_clock_skew,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_disabled_signaling_authentication_limits() {
        macro_rules! assert_invalid_limit {
            ($field:ident, $value:expr) => {{
                let mut limits = Limits::default();
                limits.$field = $value;
                assert!(matches!(
                    negotiation_limits(&limits),
                    Err(Error::InvalidLimit { name }) if name == stringify!($field)
                ));
            }};
        }

        assert_invalid_limit!(signaling_auth_global_burst, 0);
        assert_invalid_limit!(signaling_auth_global_refill_interval, Duration::ZERO);
        assert_invalid_limit!(signaling_auth_per_ip_burst, 0);
        assert_invalid_limit!(signaling_auth_per_ip_refill_interval, Duration::ZERO);
        assert_invalid_limit!(max_signaling_auth_tracked_ips, 0);
        assert_invalid_limit!(ice_restart_timeout, Duration::ZERO);
    }
}
