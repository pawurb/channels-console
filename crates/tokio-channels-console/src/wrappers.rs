use std::mem;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;

use crate::{init_stats_state, ChannelType, StatsEvent};

/// Wrap the inner channel with proxy ends. Returns (outer_tx, outer_rx).
/// All messages pass through the two forwarders.
pub(crate) fn wrap_channel<T: Send + 'static>(
    inner: (Sender<T>, Receiver<T>),
    channel_id: &'static str,
    label: Option<&'static str>,
) -> (Sender<T>, Receiver<T>) {
    let (inner_tx, mut inner_rx) = inner;
    let type_name = std::any::type_name::<T>();

    let capacity = inner_tx.capacity();
    let (outer_tx, mut to_inner_rx) = mpsc::channel::<T>(capacity);
    let (from_inner_tx, outer_rx) = mpsc::channel::<T>(capacity);

    let (stats_tx, _) = init_stats_state();

    let _ = stats_tx.send(StatsEvent::Created {
        id: channel_id,
        display_label: label,
        channel_type: ChannelType::Bounded(capacity),
        type_name,
        type_size: mem::size_of::<T>(),
    });

    let stats_tx_send = stats_tx.clone();
    let stats_tx_recv = stats_tx.clone();

    // Create a signal channel to notify send-forwarder when outer_rx is closed
    let (close_signal_tx, mut close_signal_rx) = oneshot::channel::<()>();

    // Forward outer -> inner (proxy the send path)
    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = to_inner_rx.recv() => {
                    match msg {
                        Some(msg) => {
                            if inner_tx.send(msg).await.is_err() {
                                to_inner_rx.close();
                                break;
                            }
                            let _ = stats_tx_send.send(StatsEvent::MessageSent { id: channel_id });
                        }
                        None => break, // Outer sender dropped
                    }
                }
                _ = &mut close_signal_rx => {
                    // Outer receiver was closed/dropped, close our receiver to reject further sends
                    to_inner_rx.close();
                    break;
                }
            }
        }
        // Channel is closed
        let _ = stats_tx_send.send(StatsEvent::Closed { id: channel_id });
    });

    // Forward inner -> outer (proxy the recv path)
    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = inner_rx.recv() => {
                    match msg {
                        Some(msg) => {
                            if from_inner_tx.send(msg).await.is_ok() {
                                let _ = stats_tx_recv.send(StatsEvent::MessageReceived { id: channel_id });
                            } else {
                                let _ = close_signal_tx.send(());
                                break;
                            }
                        }
                        None => break, // Inner sender dropped
                    }
                }
                _ = from_inner_tx.closed() => {
                    // Outer receiver was closed/dropped
                    let _ = close_signal_tx.send(());
                    break;
                }
            }
        }
        // Channel is closed (either inner sender dropped or outer receiver closed)
        let _ = stats_tx_recv.send(StatsEvent::Closed { id: channel_id });
    });

    (outer_tx, outer_rx)
}

/// Wrap an unbounded channel with proxy ends. Returns (outer_tx, outer_rx).
pub(crate) fn wrap_unbounded<T: Send + 'static>(
    inner: (UnboundedSender<T>, UnboundedReceiver<T>),
    channel_id: &'static str,
    label: Option<&'static str>,
) -> (UnboundedSender<T>, UnboundedReceiver<T>) {
    let (inner_tx, mut inner_rx) = inner;
    let type_name = std::any::type_name::<T>();

    let (outer_tx, mut to_inner_rx) = mpsc::unbounded_channel::<T>();
    let (from_inner_tx, outer_rx) = mpsc::unbounded_channel::<T>();

    let (stats_tx, _) = init_stats_state();

    let _ = stats_tx.send(StatsEvent::Created {
        id: channel_id,
        display_label: label,
        channel_type: ChannelType::Unbounded,
        type_name,
        type_size: mem::size_of::<T>(),
    });

    let stats_tx_send = stats_tx.clone();
    let stats_tx_recv = stats_tx.clone();

    // Create a signal channel to notify send-forwarder when outer_rx is closed
    let (close_signal_tx, mut close_signal_rx) = oneshot::channel::<()>();

    // Forward outer -> inner (proxy the send path)
    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = to_inner_rx.recv() => {
                    match msg {
                        Some(msg) => {
                            if inner_tx.send(msg).is_err() {
                                to_inner_rx.close();
                                break;
                            }
                            let _ = stats_tx_send.send(StatsEvent::MessageSent { id: channel_id });
                        }
                        None => break, // Outer sender dropped
                    }
                }
                _ = &mut close_signal_rx => {
                    // Outer receiver was closed/dropped, close our receiver to reject further sends
                    to_inner_rx.close();
                    break;
                }
            }
        }
        // Channel is closed
        let _ = stats_tx_send.send(StatsEvent::Closed { id: channel_id });
    });

    // Forward inner -> outer (proxy the recv path)
    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = inner_rx.recv() => {
                    match msg {
                        Some(msg) => {
                            if from_inner_tx.send(msg).is_ok() {
                                let _ = stats_tx_recv.send(StatsEvent::MessageReceived { id: channel_id });
                            } else {
                                // Outer receiver was closed
                                let _ = close_signal_tx.send(());
                                break;
                            }
                        }
                        None => break, // Inner sender dropped
                    }
                }
                _ = from_inner_tx.closed() => {
                    // Outer receiver was closed/dropped
                    let _ = close_signal_tx.send(());
                    break;
                }
            }
        }
        // Channel is closed (either inner sender dropped or outer receiver closed)
        let _ = stats_tx_recv.send(StatsEvent::Closed { id: channel_id });
    });

    (outer_tx, outer_rx)
}

/// Wrap a oneshot channel with proxy ends. Returns (outer_tx, outer_rx).
pub(crate) fn wrap_oneshot<T: Send + 'static>(
    inner: (oneshot::Sender<T>, oneshot::Receiver<T>),
    channel_id: &'static str,
    label: Option<&'static str>,
) -> (oneshot::Sender<T>, oneshot::Receiver<T>) {
    let (inner_tx, inner_rx) = inner;
    let type_name = std::any::type_name::<T>();

    let (outer_tx, outer_rx_proxy) = oneshot::channel::<T>();
    let (mut inner_tx_proxy, outer_rx) = oneshot::channel::<T>();

    let (stats_tx, _) = init_stats_state();

    let _ = stats_tx.send(StatsEvent::Created {
        id: channel_id,
        display_label: label,
        channel_type: ChannelType::Oneshot,
        type_name,
        type_size: mem::size_of::<T>(),
    });

    let stats_tx_send = stats_tx.clone();
    let stats_tx_recv = stats_tx;

    // Create a signal channel to notify send-forwarder when outer_rx is closed
    let (close_signal_tx, mut close_signal_rx) = oneshot::channel::<()>();

    // Monitor outer receiver and drop inner receiver when outer is dropped
    tokio::spawn(async move {
        let mut inner_rx = Some(inner_rx);
        let mut message_received = false;
        tokio::select! {
            msg = async { inner_rx.take().unwrap().await }, if inner_rx.is_some() => {
                // Message received from inner
                match msg {
                    Ok(msg) => {
                        if inner_tx_proxy.send(msg).is_ok() {
                            let _ = stats_tx_recv.send(StatsEvent::MessageReceived { id: channel_id });
                            message_received = true;
                        }
                    }
                    Err(_) => {
                        // Inner sender was dropped without sending
                    }
                }
            }
            _ = inner_tx_proxy.closed() => {
                // Outer receiver was dropped - drop inner_rx to make sends fail
                drop(inner_rx);
                let _ = close_signal_tx.send(());
            }
        }
        // Only send Closed if message was not successfully received
        if !message_received {
            let _ = stats_tx_recv.send(StatsEvent::Closed { id: channel_id });
        }
    });

    // Forward outer -> inner (proxy the send path)
    tokio::spawn(async move {
        let mut message_sent = false;
        tokio::select! {
            msg = outer_rx_proxy => {
                match msg {
                    Ok(msg) => {
                        if inner_tx.send(msg).is_ok() {
                            let _ = stats_tx_send.send(StatsEvent::MessageSent { id: channel_id });
                            let _ = stats_tx_send.send(StatsEvent::Notified { id: channel_id });
                            message_sent = true;
                        }
                    }
                    Err(_) => {
                        // Outer sender was dropped without sending
                    }
                }
            }
            _ = &mut close_signal_rx => {
                // Outer receiver was closed/dropped before send
            }
        }
        // Only send Closed if message was not successfully sent
        if !message_sent {
            let _ = stats_tx_send.send(StatsEvent::Closed { id: channel_id });
        }
    });

    (outer_tx, outer_rx)
}
