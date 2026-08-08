use std::{
    error::Error,
    fmt,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use tokio::time::Instant;

use super::{HostedService, ServiceHost, ServicePolicy, ShutdownContext};

#[derive(Debug, Clone, Copy)]
enum ShutdownBehavior {
    Complete,
    Fail,
    Pending,
}

struct TestService {
    id: &'static str,
    behavior: ShutdownBehavior,
    events: Arc<Mutex<Vec<String>>>,
}

impl TestService {
    fn new(id: &'static str, behavior: ShutdownBehavior, events: Arc<Mutex<Vec<String>>>) -> Self {
        Self { id, behavior, events }
    }

    fn record(&self, event: &str) {
        self.events.lock().unwrap().push(format!("{event}:{}", self.id));
    }
}

#[async_trait]
impl HostedService for TestService {
    const NAME: &'static str = "test service";

    type ServiceHandle = &'static str;
    type Error = TestError;

    fn service_handle(&self) -> Self::ServiceHandle {
        self.id
    }

    fn prepare_shutdown(&mut self, _context: ShutdownContext) {
        self.record("prepare");
    }

    async fn shutdown(&mut self, _context: ShutdownContext) -> Result<(), Self::Error> {
        self.record("shutdown");
        match self.behavior {
            ShutdownBehavior::Complete => Ok(()),
            ShutdownBehavior::Fail => Err(TestError),
            ShutdownBehavior::Pending => std::future::pending().await,
        }
    }

    fn cancel(&mut self) {
        self.record("cancel");
    }
}

#[derive(Debug)]
struct TestError;

impl fmt::Display for TestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("shutdown failed")
    }
}

impl Error for TestError {}

fn recorded_events(log: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    log.lock().unwrap().clone()
}

#[test]
fn bounded_deadline_uses_the_earlier_shared_or_relative_limit() {
    let shared = Instant::now() + Duration::from_secs(1);
    assert_eq!(ShutdownContext::new(Some(shared)).bounded_deadline(Duration::from_secs(2)), shared);

    let before = Instant::now();
    let relative = ShutdownContext::new(Some(before + Duration::from_secs(2)))
        .bounded_deadline(Duration::from_secs(1));
    let after = Instant::now();
    assert!(relative >= before + Duration::from_secs(1));
    assert!(relative <= after + Duration::from_secs(1));

    let expired = Instant::now() - Duration::from_millis(1);
    assert_eq!(ShutdownContext::new(Some(expired)).bounded_deadline(Duration::ZERO), expired);
}

#[tokio::test]
async fn returns_typed_handles_and_stops_services_in_stable_phase_order() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut host = ServiceHost::new();

    let last = host.install(
        TestService::new("last", ShutdownBehavior::Complete, events.clone()),
        ServicePolicy::new(2),
    );
    let first = host.install(
        TestService::new("first", ShutdownBehavior::Complete, events.clone()),
        ServicePolicy::new(1).prepare_early(),
    );
    let second = host.install(
        TestService::new("second", ShutdownBehavior::Complete, events.clone()),
        ServicePolicy::new(1),
    );

    assert_eq!((first, second, last), ("first", "second", "last"));
    host.prepare_shutdown(ShutdownContext::default());
    assert_eq!(recorded_events(&events), ["prepare:first"]);

    assert!(host.shutdown(ShutdownContext::default()).await.is_empty());
    assert!(host.is_empty());
    assert_eq!(
        recorded_events(&events),
        [
            "prepare:first",
            "prepare:first",
            "shutdown:first",
            "prepare:second",
            "shutdown:second",
            "prepare:last",
            "shutdown:last",
        ]
    );
}

#[tokio::test]
async fn cancels_a_failed_service_and_continues_shutdown() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut host = ServiceHost::new();
    host.install(
        TestService::new("failed", ShutdownBehavior::Fail, events.clone()),
        ServicePolicy::new(0),
    );
    host.install(
        TestService::new("next", ShutdownBehavior::Complete, events.clone()),
        ServicePolicy::new(1),
    );

    let failures = host.shutdown(ShutdownContext::default()).await;

    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].service(), TestService::NAME);
    assert_eq!(failures[0].source().unwrap().to_string(), "shutdown failed");
    assert_eq!(
        recorded_events(&events),
        ["prepare:failed", "shutdown:failed", "cancel:failed", "prepare:next", "shutdown:next",]
    );
}

#[test]
fn explicit_cancellation_uses_stable_phase_order() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut host = ServiceHost::new();
    host.install(
        TestService::new("last", ShutdownBehavior::Complete, events.clone()),
        ServicePolicy::new(2),
    );
    host.install(
        TestService::new("first", ShutdownBehavior::Complete, events.clone()),
        ServicePolicy::new(1),
    );
    host.install(
        TestService::new("second", ShutdownBehavior::Complete, events.clone()),
        ServicePolicy::new(1),
    );

    host.cancel();

    assert!(host.is_empty());
    assert_eq!(recorded_events(&events), ["cancel:first", "cancel:second", "cancel:last"]);
}

#[test]
fn drop_cancellation_uses_stable_phase_order() {
    let events = Arc::new(Mutex::new(Vec::new()));
    {
        let mut host = ServiceHost::new();
        host.install(
            TestService::new("last", ShutdownBehavior::Complete, events.clone()),
            ServicePolicy::new(2),
        );
        host.install(
            TestService::new("first", ShutdownBehavior::Complete, events.clone()),
            ServicePolicy::new(1),
        );
        host.install(
            TestService::new("second", ShutdownBehavior::Complete, events.clone()),
            ServicePolicy::new(1),
        );
    }

    assert_eq!(recorded_events(&events), ["cancel:first", "cancel:second", "cancel:last"]);
}

#[tokio::test]
async fn cancelling_shutdown_cancels_the_current_and_remaining_services() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut host = ServiceHost::new();
    host.install(
        TestService::new("pending", ShutdownBehavior::Pending, events.clone()),
        ServicePolicy::new(0),
    );
    host.install(
        TestService::new("waiting", ShutdownBehavior::Complete, events.clone()),
        ServicePolicy::new(1),
    );

    let result =
        tokio::time::timeout(Duration::from_millis(10), host.shutdown(ShutdownContext::default()))
            .await;

    assert!(result.is_err());
    assert!(host.is_empty());
    let events = recorded_events(&events);
    assert_eq!(&events[..2], ["prepare:pending", "shutdown:pending"]);
    assert_eq!(events.len(), 4);
    assert!(events.iter().any(|event| event == "cancel:pending"));
    assert!(events.iter().any(|event| event == "cancel:waiting"));
}

#[tokio::test]
async fn completed_services_are_not_recancelled_when_shutdown_is_dropped() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut host = ServiceHost::new();
    host.install(
        TestService::new("complete", ShutdownBehavior::Complete, events.clone()),
        ServicePolicy::new(0),
    );
    host.install(
        TestService::new("pending", ShutdownBehavior::Pending, events.clone()),
        ServicePolicy::new(1),
    );

    assert!(
        tokio::time::timeout(Duration::from_millis(10), host.shutdown(ShutdownContext::default()),)
            .await
            .is_err()
    );

    assert!(host.is_empty());
    assert_eq!(
        recorded_events(&events),
        [
            "prepare:complete",
            "shutdown:complete",
            "prepare:pending",
            "shutdown:pending",
            "cancel:pending",
        ]
    );
}

#[tokio::test]
async fn an_exhausted_shared_deadline_cancels_every_service_without_starting_shutdown() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut host = ServiceHost::new();
    host.install(
        TestService::new("first", ShutdownBehavior::Pending, events.clone()),
        ServicePolicy::new(0),
    );
    host.install(
        TestService::new("second", ShutdownBehavior::Complete, events.clone()),
        ServicePolicy::new(1),
    );

    let failures = host.shutdown(ShutdownContext::new(Some(Instant::now()))).await;

    assert!(host.is_empty());
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].to_string(), "test service: shared shutdown deadline exceeded");
    assert_eq!(recorded_events(&events), ["cancel:first", "cancel:second"]);
}

#[tokio::test]
async fn reaching_the_shared_deadline_cancels_the_current_and_remaining_services() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut host = ServiceHost::new();
    host.install(
        TestService::new("pending", ShutdownBehavior::Pending, events.clone()),
        ServicePolicy::new(0),
    );
    host.install(
        TestService::new("waiting", ShutdownBehavior::Complete, events.clone()),
        ServicePolicy::new(1),
    );

    let deadline = Instant::now() + Duration::from_millis(10);
    let failures = host.shutdown(ShutdownContext::new(Some(deadline))).await;

    assert!(host.is_empty());
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].to_string(), "test service: shared shutdown deadline exceeded");
    assert_eq!(
        recorded_events(&events),
        ["prepare:pending", "shutdown:pending", "cancel:pending", "cancel:waiting"]
    );
}

#[tokio::test]
async fn earlier_failures_are_retained_when_a_later_service_reaches_the_deadline() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut host = ServiceHost::new();
    host.install(
        TestService::new("failed", ShutdownBehavior::Fail, events.clone()),
        ServicePolicy::new(0),
    );
    host.install(
        TestService::new("pending", ShutdownBehavior::Pending, events.clone()),
        ServicePolicy::new(1),
    );

    let deadline = Instant::now() + Duration::from_millis(10);
    let failures = host.shutdown(ShutdownContext::new(Some(deadline))).await;

    assert_eq!(failures.len(), 2);
    assert_eq!(failures[0].source().unwrap().to_string(), "shutdown failed");
    assert_eq!(failures[1].source().unwrap().to_string(), "shared shutdown deadline exceeded");
    assert_eq!(
        recorded_events(&events),
        [
            "prepare:failed",
            "shutdown:failed",
            "cancel:failed",
            "prepare:pending",
            "shutdown:pending",
            "cancel:pending",
        ]
    );
}
