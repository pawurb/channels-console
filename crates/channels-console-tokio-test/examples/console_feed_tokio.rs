use tokio::time::{sleep, Duration};

#[allow(unused)]
#[derive(Debug)]
struct UserData {
    id: u64,
    name: String,
    email: String,
    age: u8,
}

impl UserData {
    fn random() -> Self {
        Self {
            id: rand::random(),
            name: format!("User {}", rand::random::<u8>()),
            email: format!("user{}@example.com", rand::random::<u8>()),
            age: rand::random(),
        }
    }
}

#[allow(unused_mut)]
#[tokio::main]
async fn main() {
    #[cfg(feature = "tokio-console")]
    console_subscriber::init();

    #[cfg(feature = "channels-console")]
    let _channels_guard = channels_console::ChannelsGuard::new();

    println!("Open the TUI console to watch live updates!");
    println!("   Run: cargo run -p channels-console --features tui -- console\n");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Channel 1: Fast data stream - unbounded, rapid messages
    let (tx_fast, mut rx_fast) = tokio::sync::mpsc::unbounded_channel::<String>();
    #[cfg(feature = "channels-console")]
    let (tx_fast, mut rx_fast) =
        channels_console::instrument!((tx_fast, rx_fast), label = "fast-data-stream", log = true);

    // Channel 2: Slow consumer - bounded(5), will back up!
    let (tx_slow, mut rx_slow) = tokio::sync::mpsc::channel::<UserData>(5);
    #[cfg(feature = "channels-console")]
    let (tx_slow, mut rx_slow) =
        channels_console::instrument!((tx_slow, rx_slow), label = "slow-consumer", log = true);

    // Channel 3: Burst traffic - bounded(10), bursts every 3 seconds
    let (tx_burst, mut rx_burst) = tokio::sync::mpsc::channel::<u64>(10);
    #[cfg(feature = "channels-console")]
    let (tx_burst, mut rx_burst) =
        channels_console::instrument!((tx_burst, rx_burst), label = "burst-traffic", log = true);

    // Channel 4: Gradual flow - bounded(20), increasing rate
    let (tx_gradual, mut rx_gradual) = tokio::sync::mpsc::channel::<f64>(20);
    #[cfg(feature = "channels-console")]
    let (tx_gradual, mut rx_gradual) =
        channels_console::instrument!((tx_gradual, rx_gradual), log = true);

    // Channel 5: Dropped early - unbounded, producer dies at 10s
    let (tx_drop_early, mut rx_drop_early) = tokio::sync::mpsc::unbounded_channel::<bool>();
    #[cfg(feature = "channels-console")]
    let (tx_drop_early, mut rx_drop_early) =
        channels_console::instrument!((tx_drop_early, rx_drop_early), log = true);

    // Channel 6: Consumer dies - bounded(8), consumer stops at 15s
    let (tx_consumer_dies, mut rx_consumer_dies) = tokio::sync::mpsc::channel::<Vec<u8>>(8);
    #[cfg(feature = "channels-console")]
    let (tx_consumer_dies, mut rx_consumer_dies) = channels_console::instrument!(
        (tx_consumer_dies, rx_consumer_dies),
        label = "consumer-dies",
        log = true
    );

    // Channel 7: Steady stream - unbounded, consistent 500ms rate
    let (tx_steady, mut rx_steady) = tokio::sync::mpsc::unbounded_channel::<&str>();
    #[cfg(feature = "channels-console")]
    let (tx_steady, mut rx_steady) =
        channels_console::instrument!((tx_steady, rx_steady), log = true);

    // Channel 8: Oneshot early - fires at 5 seconds
    let (tx_oneshot_early, rx_oneshot_early) = tokio::sync::oneshot::channel::<String>();
    #[cfg(feature = "channels-console")]
    let (tx_oneshot_early, rx_oneshot_early) =
        channels_console::instrument!((tx_oneshot_early, rx_oneshot_early), log = true);

    // Channel 9: Oneshot mid - fires at 15 seconds
    let (tx_oneshot_mid, rx_oneshot_mid) = tokio::sync::oneshot::channel::<u32>();
    #[cfg(feature = "channels-console")]
    let (tx_oneshot_mid, rx_oneshot_mid) =
        channels_console::instrument!((tx_oneshot_mid, rx_oneshot_mid), log = true);

    // Channel 10: Oneshot late - fires at 25 seconds
    let (tx_oneshot_late, rx_oneshot_late) = tokio::sync::oneshot::channel::<i64>();
    #[cfg(feature = "channels-console")]
    let (tx_oneshot_late, rx_oneshot_late) = channels_console::instrument!(
        (tx_oneshot_late, rx_oneshot_late),
        label = "oneshot-late",
        log = true
    );

    println!("Creating 3 bounded iter channels...");
    for i in 0..3 {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u32>(5);

        #[cfg(feature = "channels-console")]
        let (tx, mut rx) = channels_console::instrument!((tx, rx), log = true);

        tokio::spawn(async move {
            for j in 0..50 {
                let _ = tx.send(i * 10 + j).await;
                sleep(Duration::from_millis(500)).await;
            }
        });

        tokio::spawn(async move {
            while let Some(_msg) = rx.recv().await {
                sleep(Duration::from_millis(200)).await;
            }
        });
    }

    // === Task 1: Fast data stream producer (10ms interval) ===
    tokio::spawn(async move {
        let messages = ["foo", "baz", "bar"];
        for i in 0..3000 {
            let msg = messages[i % messages.len()].to_string();
            if tx_fast.send(msg).is_err() {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    });

    // === Task 2: Fast data stream consumer ===
    tokio::spawn(async move {
        while let Some(msg) = rx_fast.recv().await {
            let _ = msg;
            sleep(Duration::from_millis(15)).await;
        }
    });

    // === Task 3: Slow consumer producer (fast sends) ===
    tokio::spawn(async move {
        for _ in 0..200 {
            if tx_slow.send(UserData::random()).await.is_err() {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
    });

    // === Task 4: Slow consumer (very slow, queue backs up!) ===
    tokio::spawn(async move {
        while let Some(msg) = rx_slow.recv().await {
            println!("Slow consumer processing: {:?}", msg);
            sleep(Duration::from_millis(800)).await; // Much slower than producer!
        }
    });

    // === Task 5: Burst traffic producer ===
    tokio::spawn(async move {
        for burst_num in 0..10 {
            println!("Burst #{} starting!", burst_num + 1);
            // Send burst of 15 messages
            for i in 0..15 {
                if tx_burst.send(burst_num * 1000 + i).await.is_err() {
                    return;
                }
            }
            sleep(Duration::from_secs(3)).await;
        }
    });

    // === Task 6: Burst traffic consumer ===
    tokio::spawn(async move {
        while let Some(msg) = rx_burst.recv().await {
            let _ = msg;
            sleep(Duration::from_millis(200)).await;
        }
    });

    // === Task 7: Gradual flow producer (accelerating rate) ===
    tokio::spawn(async move {
        for i in 0..100 {
            if tx_gradual
                .send(i as f64 * std::f64::consts::PI)
                .await
                .is_err()
            {
                break;
            }
            // Delay decreases over time (speeds up)
            let delay = 500 - (i * 4).min(400);
            sleep(Duration::from_millis(delay)).await;
        }
    });

    // === Task 8: Gradual flow consumer ===
    tokio::spawn(async move {
        while rx_gradual.recv().await.is_some() {
            sleep(Duration::from_millis(200)).await;
        }
    });

    // === Task 9: Dropped early producer (dies at 10s) ===
    tokio::spawn(async move {
        for i in 0..100 {
            if i == 50 {
                println!("'dropped-early' producer dying at 10s!");
                break;
            }
            let _ = tx_drop_early.send(i % 2 == 0);
            sleep(Duration::from_millis(200)).await;
        }
    });

    // === Task 10: Dropped early consumer ===
    tokio::spawn(async move {
        while rx_drop_early.recv().await.is_some() {
            sleep(Duration::from_millis(100)).await;
        }
        println!("'dropped-early' consumer detected channel closed");
    });

    // === Task 11: Consumer dies producer ===
    let consumer_dies_handle = tokio::spawn(async move {
        for i in 0..300 {
            if tx_consumer_dies.send(vec![i as u8; 10]).await.is_err() {
                println!("'consumer-dies' producer detected closed channel");
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
    });

    // === Task 12: Consumer dies consumer (dies at 15s) ===
    tokio::spawn(async move {
        for _ in 0..75 {
            if rx_consumer_dies.recv().await.is_some() {
                sleep(Duration::from_millis(200)).await;
            }
        }
        println!("'consumer-dies' consumer stopping at 15s!");
        drop(rx_consumer_dies);
    });

    // === Task 13: Steady stream producer ===
    tokio::spawn(async move {
        let messages = ["tick", "tock", "ping", "pong", "beep", "boop"];
        for i in 0..60 {
            let _ = tx_steady.send(messages[i % messages.len()]);
            sleep(Duration::from_millis(500)).await;
        }
    });

    // === Task 14: Steady stream consumer ===
    tokio::spawn(async move {
        while let Some(msg) = rx_steady.recv().await {
            let _ = msg;
            sleep(Duration::from_millis(400)).await;
        }
    });

    // === Task 15: Oneshot receivers ===
    tokio::spawn(async move {
        match rx_oneshot_early.await {
            Ok(msg) => println!("Oneshot-early received: {}", msg),
            Err(_) => println!("Oneshot-early sender dropped"),
        }
    });

    tokio::spawn(async move {
        match rx_oneshot_mid.await {
            Ok(msg) => println!("Oneshot-mid received: {}", msg),
            Err(_) => println!("Oneshot-mid sender dropped"),
        }
    });

    tokio::spawn(async move {
        match rx_oneshot_late.await {
            Ok(msg) => println!("Oneshot-late received: {}", msg),
            Err(_) => println!("Oneshot-late sender dropped"),
        }
    });

    // === Task 16: Oneshot senders (fire at specific times) ===
    tokio::spawn(async move {
        sleep(Duration::from_secs(5)).await;
        println!("Firing oneshot-early at 5s");
        let _ = tx_oneshot_early.send("Early bird gets the worm!".to_string());
    });

    tokio::spawn(async move {
        sleep(Duration::from_secs(15)).await;
        println!("Firing oneshot-mid at 15s");
        let _ = tx_oneshot_mid.send(42);
    });

    tokio::spawn(async move {
        sleep(Duration::from_secs(25)).await;
        println!("Firing oneshot-late at 25s");
        let _ = tx_oneshot_late.send(9000);
    });

    let _progress_handle = tokio::spawn(async move {
        for i in 0..=60 {
            println!("Time: {}s / 60s", i);
            sleep(Duration::from_secs(1)).await;
        }
    });

    sleep(Duration::from_secs(60)).await;

    drop(consumer_dies_handle);
}
