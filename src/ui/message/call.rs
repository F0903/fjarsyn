#[derive(Debug, Clone, PartialEq)]
pub enum CallTarget {
    PeerId(String),
    Address(String),
    ContactId(i64),
}

#[derive(Debug, Clone)]
pub enum CallActionMessage {
    AcceptCall,
    DeclineCall,
    StartCall(CallTarget),
}
