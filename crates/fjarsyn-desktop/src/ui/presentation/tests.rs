use fjarsyn_engine::peer_session::Phase;

use super::{Presence, Session, fingerprint_grid, project_peer};

const PHASES: [Option<Phase>; 7] = [
    None,
    Some(Phase::Requesting),
    Some(Phase::Incoming),
    Some(Phase::Negotiating),
    Some(Phase::Connected),
    Some(Phase::Reconnecting),
    Some(Phase::Disconnecting),
];

#[test]
fn projects_every_presence_and_session_combination_independently() {
    for nearby in [false, true] {
        for phase in PHASES {
            let projection = project_peer(nearby, phase);
            assert_eq!(projection.presence, if nearby { Presence::Nearby } else { Presence::Away });
            assert_eq!(
                projection.session,
                match phase {
                    None => Session::Disconnected,
                    Some(Phase::Requesting) => Session::Requesting,
                    Some(Phase::Incoming) => Session::Incoming,
                    Some(Phase::Negotiating) => Session::Negotiating,
                    Some(Phase::Connected) => Session::Connected,
                    Some(Phase::Reconnecting) => Session::Reconnecting,
                    Some(Phase::Disconnecting) => Session::Disconnecting,
                }
            );
        }
    }
}

#[test]
fn presence_loss_never_changes_a_live_session_projection() {
    for phase in PHASES.into_iter().flatten() {
        assert_eq!(
            project_peer(false, Some(phase)).session,
            project_peer(true, Some(phase)).session
        );
    }
}

#[test]
fn actions_are_gated_by_deliberate_session_state() {
    assert!(!project_peer(false, None).can_connect());
    assert!(project_peer(true, None).can_connect());
    assert!(project_peer(false, Some(Phase::Requesting)).can_disconnect());
    assert!(project_peer(false, Some(Phase::Negotiating)).can_disconnect());
    assert!(project_peer(false, Some(Phase::Connected)).can_disconnect());
    assert!(project_peer(false, Some(Phase::Reconnecting)).can_disconnect());
    assert!(!project_peer(false, Some(Phase::Disconnecting)).can_disconnect());
    assert!(project_peer(false, Some(Phase::Connected)).capabilities_ready());
    assert!(!project_peer(false, Some(Phase::Reconnecting)).capabilities_ready());
    assert!(!project_peer(true, Some(Phase::Negotiating)).capabilities_ready());
    assert!(project_peer(false, None).can_mutate_trust());
    for phase in PHASES.into_iter().flatten() {
        assert!(!project_peer(false, Some(phase)).can_mutate_trust());
    }
}

#[test]
fn full_fingerprint_uses_a_fixed_readable_four_by_four_grid() {
    let fingerprint =
        "0001 0203 0405 0607 0809 0A0B 0C0D 0E0F 1011 1213 1415 1617 1819 1A1B 1C1D 1E1F";
    let grid = fingerprint_grid(fingerprint);

    assert_eq!(grid.lines().count(), 4);
    assert!(grid.lines().all(|line| line.split_whitespace().count() == 4));
    assert_eq!(grid.split_whitespace().collect::<Vec<_>>().join(" "), fingerprint);
}
