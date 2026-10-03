//! Starts one container with the watchdog enabled, prints its id, then waits to be interrupted.

use std::time::Duration;

use testcontainers::{runners::AsyncRunner, GenericImage, ImageExt};

#[tokio::main]
async fn main() {
    let image = std::env::var("PROBE_IMAGE").expect("PROBE_IMAGE is set");
    let (name, tag) = image.rsplit_once(':').expect("PROBE_IMAGE has a tag");
    let cmd: Vec<String> = std::env::var("PROBE_CMD")
        .expect("PROBE_CMD is set")
        .split_whitespace()
        .map(String::from)
        .collect();

    let container = GenericImage::new(name, tag)
        .with_cmd(cmd)
        .start()
        .await
        .expect("container starts");
    println!("CONTAINER {}", container.id());

    tokio::time::sleep(Duration::from_secs(300)).await;
    drop(container);
    println!("TIMED OUT WITHOUT SIGNAL");
}
