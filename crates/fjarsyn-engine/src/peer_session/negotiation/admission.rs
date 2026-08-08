//! Authentication-attempt admission control for signaling listeners.

use std::{
    collections::HashMap,
    net::IpAddr,
    time::{Duration, Instant},
};

use crate::peer_session::negotiation::Limits;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AuthenticationRateLimitExceeded {
    Global,
    PerIp,
    TrackingCapacity,
}

#[derive(Debug)]
struct TokenBucket {
    capacity: usize,
    available: usize,
    refill_interval: Duration,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: usize, refill_interval: Duration, now: Instant) -> Self {
        debug_assert!(capacity > 0);
        debug_assert!(!refill_interval.is_zero());
        Self { capacity, available: capacity, refill_interval, last_refill: now }
    }

    fn refill(&mut self, now: Instant) {
        if self.available == self.capacity {
            self.last_refill = now;
            return;
        }

        let elapsed = now.saturating_duration_since(self.last_refill);
        let interval_nanos = self.refill_interval.as_nanos();
        let intervals = elapsed.as_nanos() / interval_nanos;
        if intervals == 0 {
            return;
        }

        let refill = usize::try_from(intervals).unwrap_or(usize::MAX);
        self.available = self.capacity.min(self.available.saturating_add(refill));
        if self.available == self.capacity {
            self.last_refill = now;
            return;
        }

        let remainder_nanos = elapsed.as_nanos() % interval_nanos;
        let remainder_seconds = u64::try_from(remainder_nanos / 1_000_000_000).unwrap_or(u64::MAX);
        let remainder_subseconds = (remainder_nanos % 1_000_000_000) as u32;
        let remainder = Duration::new(remainder_seconds, remainder_subseconds);
        self.last_refill = now.checked_sub(remainder).unwrap_or(now);
    }

    fn has_token(&self) -> bool {
        self.available > 0
    }

    fn take(&mut self) {
        debug_assert!(self.has_token());
        self.available -= 1;
    }

    fn is_full(&mut self, now: Instant) -> bool {
        self.refill(now);
        self.available == self.capacity
    }
}

#[derive(Debug)]
pub(super) struct AuthenticationAttemptLimiter {
    global: TokenBucket,
    per_ip: HashMap<IpAddr, TokenBucket>,
    per_ip_burst: usize,
    per_ip_refill_interval: Duration,
    max_tracked_ips: usize,
    next_tracking_cleanup: Option<Instant>,
}

impl AuthenticationAttemptLimiter {
    pub(super) fn new(limits: &Limits, now: Instant) -> Self {
        Self {
            global: TokenBucket::new(
                limits.authentication_global_burst,
                limits.authentication_global_refill_interval,
                now,
            ),
            per_ip: HashMap::new(),
            per_ip_burst: limits.authentication_per_ip_burst,
            per_ip_refill_interval: limits.authentication_per_ip_refill_interval,
            max_tracked_ips: limits.max_authentication_tracked_ips,
            // `None` means the configured interval is beyond this platform's
            // representable Instant range. Such buckets cannot refill within
            // that range, so retaining them and failing closed is consistent.
            next_tracking_cleanup: now.checked_add(limits.authentication_per_ip_refill_interval),
        }
    }

    pub(super) fn try_admit(
        &mut self,
        source_ip: IpAddr,
        now: Instant,
    ) -> Result<(), AuthenticationRateLimitExceeded> {
        let source_ip = source_ip.to_canonical();
        self.global.refill(now);
        if !self.global.has_token() {
            return Err(AuthenticationRateLimitExceeded::Global);
        }

        if let Some(bucket) = self.per_ip.get_mut(&source_ip) {
            bucket.refill(now);
            if !bucket.has_token() {
                return Err(AuthenticationRateLimitExceeded::PerIp);
            }
        } else {
            if self.per_ip.len() >= self.max_tracked_ips
                && self.next_tracking_cleanup.is_some_and(|deadline| now >= deadline)
            {
                self.per_ip.retain(|_, bucket| !bucket.is_full(now));
                self.next_tracking_cleanup = now.checked_add(self.per_ip_refill_interval);
            }
            if self.per_ip.len() >= self.max_tracked_ips {
                return Err(AuthenticationRateLimitExceeded::TrackingCapacity);
            }
            self.per_ip.insert(
                source_ip,
                TokenBucket::new(self.per_ip_burst, self.per_ip_refill_interval, now),
            );
        }

        self.global.take();
        self.per_ip.get_mut(&source_ip).expect("source bucket was admitted").take();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::{super::test_limits, *};

    #[test]
    fn enforces_global_and_per_ip_bursts_atomically() {
        let now = Instant::now();
        let first_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let second_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 11));

        let mut limits = test_limits();
        limits.authentication_global_burst = 3;
        limits.authentication_per_ip_burst = 1;
        let mut limiter = AuthenticationAttemptLimiter::new(&limits, now);

        assert_eq!(limiter.try_admit(first_ip, now), Ok(()));
        let global_after_first = limiter.global.available;
        assert_eq!(limiter.try_admit(first_ip, now), Err(AuthenticationRateLimitExceeded::PerIp));
        assert_eq!(limiter.global.available, global_after_first);

        assert_eq!(limiter.try_admit(second_ip, now), Ok(()));
        let second_available = limiter.per_ip.get(&second_ip).unwrap().available;
        assert_eq!(limiter.try_admit(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 12)), now), Ok(()));
        assert_eq!(limiter.try_admit(second_ip, now), Err(AuthenticationRateLimitExceeded::Global));
        assert_eq!(
            limiter.per_ip.get(&second_ip).map(|bucket| bucket.available),
            Some(second_available)
        );
    }

    #[test]
    fn refills_at_exact_intervals_without_overfilling() {
        let now = Instant::now();
        let source_ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let mut limits = test_limits();
        limits.authentication_global_burst = 2;
        limits.authentication_global_refill_interval = Duration::from_millis(100);
        limits.authentication_per_ip_burst = 2;
        limits.authentication_per_ip_refill_interval = Duration::from_millis(100);
        let mut limiter = AuthenticationAttemptLimiter::new(&limits, now);

        assert_eq!(limiter.try_admit(source_ip, now), Ok(()));
        assert_eq!(limiter.try_admit(source_ip, now), Ok(()));
        assert_eq!(
            limiter.try_admit(source_ip, now + Duration::from_millis(99)),
            Err(AuthenticationRateLimitExceeded::Global)
        );
        assert_eq!(limiter.try_admit(source_ip, now + Duration::from_millis(100)), Ok(()));
        assert_eq!(
            limiter.try_admit(source_ip, now + Duration::from_millis(100)),
            Err(AuthenticationRateLimitExceeded::Global)
        );

        let much_later = now + Duration::from_secs(10);
        assert_eq!(limiter.try_admit(source_ip, much_later), Ok(()));
        assert_eq!(limiter.try_admit(source_ip, much_later), Ok(()));
        assert_eq!(
            limiter.try_admit(source_ip, much_later),
            Err(AuthenticationRateLimitExceeded::Global)
        );
    }

    #[test]
    fn canonicalizes_mapped_ipv4_sources() {
        let now = Instant::now();
        let mut limits = test_limits();
        limits.authentication_per_ip_burst = 1;
        let mut limiter = AuthenticationAttemptLimiter::new(&limits, now);
        let ipv4 = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let mapped = IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped());

        assert_eq!(limiter.try_admit(ipv4, now), Ok(()));
        assert_eq!(limiter.try_admit(mapped, now), Err(AuthenticationRateLimitExceeded::PerIp));
        assert_eq!(limiter.per_ip.len(), 1);
        assert!(limiter.per_ip.contains_key(&ipv4));
    }

    #[test]
    fn bounds_source_tracking_and_reuses_refilled_slots() {
        let now = Instant::now();
        let mut limits = test_limits();
        limits.authentication_global_burst = 8;
        limits.authentication_global_refill_interval = Duration::from_secs(1);
        limits.authentication_per_ip_burst = 1;
        limits.authentication_per_ip_refill_interval = Duration::from_secs(1);
        limits.max_authentication_tracked_ips = 2;
        let mut limiter = AuthenticationAttemptLimiter::new(&limits, now);
        let first = IpAddr::V6("fd00::1".parse().unwrap());
        let second = IpAddr::V6("fd00::2".parse().unwrap());
        let third = IpAddr::V6("fd00::3".parse().unwrap());

        assert_eq!(limiter.try_admit(first, now), Ok(()));
        assert_eq!(limiter.try_admit(second, now), Ok(()));
        assert_eq!(
            limiter.try_admit(third, now),
            Err(AuthenticationRateLimitExceeded::TrackingCapacity)
        );
        assert_eq!(limiter.per_ip.len(), 2);
        assert_eq!(limiter.try_admit(third, now + Duration::from_secs(1)), Ok(()));
        assert_eq!(limiter.per_ip.len(), 1);
        assert!(limiter.per_ip.contains_key(&third));
    }
}
