use crate::signaling_server::SignalingServer;

mod signaling_server;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let server = SignalingServer::new();
    server.listen("0.0.0.0:30000").await;
}
