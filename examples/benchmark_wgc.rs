use std::time::{Duration, Instant};

use fjarsyn::capture_providers::{
    CaptureProvider,
    shared::CaptureFramerate,
    windows::{WgcCaptureProviderBuilder, create_capture_item_for_primary_monitor},
};
use futures::StreamExt;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder().with_max_level(Level::TRACE).finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    tracing::info!("Initializing benchmark...");

    let mut provider =
        WgcCaptureProviderBuilder::new(fjarsyn::capture_providers::shared::PixelFormat::BGRA8)
            .with_default_device()?
            .with_default_capture_item()?
            .build()?;

    tracing::info!("Creating capture item for primary monitor...");
    let item = create_capture_item_for_primary_monitor()?;

    provider.set_capture_item(item)?;

    // Start with 60 FPS
    let mut stream = provider.create_stream(CaptureFramerate::FPS120)?;

    provider.start_capture()?;
    tracing::info!("Capture started. Benchmarking for 30 seconds...");

    let mut frame_count = 0;
    let mut last_print = Instant::now();
    let start_time = Instant::now();

    while let Some(frame) = stream.next().await {
        frame_count += 1;

        if last_print.elapsed() >= Duration::from_secs(1) {
            let fps = frame_count as f64 / last_print.elapsed().as_secs_f64();
            tracing::info!("FPS: {:.2}, Frame size: {}x{}", fps, frame.size.x, frame.size.y);
            frame_count = 0;
            last_print = Instant::now();
        }

        // Run for 30 seconds
        if start_time.elapsed() >= Duration::from_secs(30) {
            break;
        }
    }

    provider.stop_capture()?;
    tracing::info!("Benchmark finished.");

    Ok(())
}
