//! Bounded in-process event bus with finite replay for reconnecting clients.

use chrono::Utc;
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const SUBSCRIBER_CAPACITY: usize = 32;
const REPLAY_CAPACITY: usize = 256;

#[derive(Debug)]
struct EventState {
    subs: Vec<Sender<Value>>,
    history: VecDeque<Value>,
}

#[derive(Debug)]
pub struct EventSubscription {
    pub receiver: Receiver<Value>,
    pub replay: Vec<Value>,
    pub cursor: u64,
    pub resync_required: bool,
}

#[derive(Debug)]
pub struct EventHub {
    epoch: String,
    seq: AtomicU64,
    coalesced: AtomicU64,
    state: Mutex<EventState>,
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHub {
    pub fn new() -> Self {
        Self {
            epoch: format!(
                "{}-{}",
                std::process::id(),
                Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ),
            seq: AtomicU64::new(0),
            coalesced: AtomicU64::new(0),
            state: Mutex::new(EventState {
                subs: Vec::new(),
                history: VecDeque::with_capacity(REPLAY_CAPACITY),
            }),
        }
    }

    pub fn seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }
    pub fn epoch(&self) -> &str {
        &self.epoch
    }

    pub fn coalesced(&self) -> u64 {
        self.coalesced.load(Ordering::Relaxed)
    }

    pub fn subscriber_count(&self) -> usize {
        self.state.lock().map(|state| state.subs.len()).unwrap_or(0)
    }

    pub fn subscribe(&self) -> Receiver<Value> {
        self.subscribe_since(None).receiver
    }

    pub fn subscribe_since(&self, since_seq: Option<u64>) -> EventSubscription {
        let (sender, receiver) = bounded(SUBSCRIBER_CAPACITY);
        let mut replay = Vec::new();
        let mut cursor = 0;
        let mut resync_required = false;
        if let Ok(mut state) = self.state.lock() {
            cursor = self.seq();
            if let Some(since) = since_seq {
                let first = state.history.front().map(message_seq).unwrap_or(cursor);
                resync_required = since > cursor || since.saturating_add(1) < first;
                replay.extend(
                    state
                        .history
                        .iter()
                        .filter(|message| message_seq(message) > since)
                        .cloned(),
                );
            }
            state.subs.push(sender);
        }
        EventSubscription {
            receiver,
            replay,
            cursor,
            resync_required,
        }
    }

    pub fn replay_since(&self, since_seq: u64) -> Vec<Value> {
        self.state
            .lock()
            .map(|state| {
                state
                    .history
                    .iter()
                    .filter(|message| message_seq(message) > since_seq)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn emit(&self, typ: &str, data: Value) -> u64 {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let message = json!({
            "type": typ,
            "seq": seq,
            "stream_epoch": self.epoch,
            "ts": Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "data": data,
        });
        {
            if state.history.len() == REPLAY_CAPACITY {
                state.history.pop_front();
            }
            state.history.push_back(message.clone());
            state
                .subs
                .retain(|sender| match sender.try_send(message.clone()) {
                    Ok(()) => true,
                    Err(TrySendError::Full(_)) => {
                        self.coalesced.fetch_add(1, Ordering::Relaxed);
                        true
                    }
                    Err(TrySendError::Disconnected(_)) => false,
                });
        }
        seq
    }
}

fn message_seq(message: &Value) -> u64 {
    message.get("seq").and_then(Value::as_u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn emit_reaches_subscriber() {
        let hub = EventHub::new();
        let rx = hub.subscribe();
        let seq = hub.emit("quote.updated", json!({"quotes_upserted": 3}));
        let msg = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(msg["type"], "quote.updated");
        assert_eq!(msg["seq"], seq);
        assert_eq!(msg["data"]["quotes_upserted"], 3);
    }

    #[test]
    fn slow_subscriber_is_bounded_and_can_replay_latest() {
        let hub = EventHub::new();
        let rx = hub.subscribe();
        for n in 0..100 {
            hub.emit("snapshot.available", json!({"version": n}));
        }
        assert!(hub.coalesced() > 0);
        assert!(rx.len() <= SUBSCRIBER_CAPACITY);
        let replay = hub.replay_since(90);
        assert_eq!(replay.last().unwrap()["seq"], 100);
    }

    #[test]
    fn dropped_subscriber_is_pruned() {
        let hub = EventHub::new();
        {
            let _rx = hub.subscribe();
            hub.emit("ping", json!({}));
        }
        hub.emit("ingest.done", json!({}));
        assert_eq!(hub.subscriber_count(), 0);
    }
}
