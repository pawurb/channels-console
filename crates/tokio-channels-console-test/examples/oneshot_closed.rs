use tokio::sync::oneshot;

#[tokio::main]
async fn main() {
    #[cfg(feature = "tokio-channels-console")]
    let _channels_guard = tokio_channels_console::ChannelsGuard::new();

    let (tx, rx) = oneshot::channel::<String>();

    #[cfg(feature = "tokio-channels-console")]
    let (tx, rx) = tokio_channels_console::instrument!((tx, rx));

    drop(rx);

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    match tx.send("Hello oneshot!".to_string()) {
        Ok(_) => panic!("Not expected: send succeeded"),
        Err(_) => println!("Expected: Failed to send"),
    }
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    println!("\nExample completed!");
}
