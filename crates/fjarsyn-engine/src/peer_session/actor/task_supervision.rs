use std::{any::Any, future::Future, panic::AssertUnwindSafe};

use futures::FutureExt;
use tokio::{sync::mpsc, task::JoinHandle};

use super::{ActorInstanceId, TaskExit};
use crate::{identity::PeerId, peer_session::SessionId};

pub(super) fn spawn<F>(
    future: F,
    instance_id: ActorInstanceId,
    session_id: SessionId,
    peer_id: PeerId,
    exit_tx: mpsc::UnboundedSender<TaskExit>,
) -> JoinHandle<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let exit = match AssertUnwindSafe(future).catch_unwind().await {
            Ok(()) => TaskExit::Completed { instance_id, session_id, peer_id },
            Err(payload) => TaskExit::Panicked {
                instance_id,
                session_id,
                peer_id,
                reason: panic_message(payload.as_ref()),
            },
        };
        let _ = exit_tx.send(exit);
    })
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "session actor panicked with a non-string payload".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_a_panicking_actor_task() {
        let instance_id = ActorInstanceId::new();
        let session_id = SessionId::new();
        let peer_id = PeerId::new("panic-peer").unwrap();
        let (exit_tx, mut exit_rx) = mpsc::unbounded_channel();

        spawn(
            async { panic!("simulated actor failure") },
            instance_id,
            session_id,
            peer_id.clone(),
            exit_tx,
        )
        .await
        .unwrap();

        let exit = exit_rx.recv().await.unwrap();
        let (reported_instance_id, reported_session, reported_peer, reason) = exit.into_parts();
        assert_eq!(reported_instance_id, instance_id);
        assert_eq!(reported_session, session_id);
        assert_eq!(reported_peer, peer_id);
        assert_eq!(reason.as_deref(), Some("simulated actor failure"));
    }
}
