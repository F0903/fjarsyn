pub use fjarsyn_core::call::CallTarget;

#[derive(Debug, Clone)]
pub enum CallActionMessage {
    AcceptCall,
    DeclineCall,
    StartCall(CallTarget),
}
