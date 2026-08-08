use std::time::Duration;

use tokio::time::Instant;

use super::Coordinator;
use crate::peer_session::TransportGeneration;

#[test]
fn restart_generation_is_exactly_next_and_commits_once() {
    let now = Instant::now();
    let mut coordinator = Coordinator::default();
    let generation = coordinator.begin_local(now + Duration::from_secs(1)).unwrap();
    assert_eq!(generation, TransportGeneration::from_value(1));
    assert!(coordinator.begin_local(now).is_err());
    assert!(coordinator.commit(generation).is_err());
    coordinator.engage(generation).unwrap();
    coordinator.authorize(generation).unwrap();
    coordinator.commit(generation).unwrap();
    assert_eq!(coordinator.committed(), generation);
}

#[test]
fn stale_future_and_duplicate_remote_attempts_are_rejected() {
    let now = Instant::now();
    let mut coordinator = Coordinator::default();
    assert!(coordinator.begin_remote(TransportGeneration::from_value(2), now).is_err());
    coordinator.begin_remote(TransportGeneration::from_value(1), now).unwrap();
    assert!(coordinator.begin_remote(TransportGeneration::from_value(1), now).is_err());
    assert!(coordinator.require_active(TransportGeneration::INITIAL).is_err());
}

#[test]
fn only_an_unengaged_attempt_can_be_cancelled() {
    let now = Instant::now();
    let mut coordinator = Coordinator::default();
    let generation = coordinator.begin_local(now).unwrap();
    coordinator.cancel().unwrap();
    coordinator
        .begin_remote(generation, now)
        .unwrap_or_else(|error| panic!("cancelled generation remains reusable: {error}"));
    coordinator.engage(TransportGeneration::from_value(1)).unwrap();
    coordinator.authorize(TransportGeneration::from_value(1)).unwrap();
    assert!(coordinator.cancel().is_err());
}
