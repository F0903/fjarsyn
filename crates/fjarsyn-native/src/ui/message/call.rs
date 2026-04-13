pub use fjarsyn_core::communication::call::CallTarget;

#[derive(Debug, Clone)]
pub enum CallActionMessage {
    AcceptCall,
    AcceptFailed { error: String, peer_id: Option<String> },
    DeclineCall,
    DeclineFailed { error: String, peer_id: Option<String> },
    StartCall(CallTarget),
    StartFailed(String),
}
