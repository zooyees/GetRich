//! UI-only command bridge. No storage or network waits run in an egui frame.
use getrich_core::{
    service::App,
    view::{BarPage, QuoteBoardPage, StockDetail, StoreStats},
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{
    mpsc::{self, Receiver, SyncSender},
    Arc, Mutex,
};

pub enum Payload {
    Map(Box<getrich_core::market_map::MarketMap>),
    Board(QuoteBoardPage, bool),
    WatchSearch(String, String, QuoteBoardPage),
    Stock(Box<StockDetail>, BarPage),
    Quote(Box<StockDetail>),
    Bars(getrich_core::models::InstrumentKey, BarPage, Option<String>),
    Research(getrich_core::models::InstrumentKey, Value),
    Notebook(getrich_core::models::InstrumentKey, Value),
    NotebookHistory(getrich_core::models::InstrumentKey, Value),
    Screen(Value),
    Workbench(String, Value),
    Stats(StoreStats),
    Settings(Value),
    Provider(Value),
    Jobs(Value),
    Action(String, Value),
}

pub struct Reply {
    pub tag: String,
    pub generation: u64,
    pub result: Result<Payload, String>,
}
type Operation = Box<dyn FnOnce(&App) -> Result<Payload, String> + Send>;
struct Command {
    tag: String,
    generation: u64,
    operation: Operation,
}
pub struct Controller {
    sender: SyncSender<Command>,
    data_sender: SyncSender<Command>,
    action_sender: SyncSender<Command>,
    pending: Mutex<HashMap<String, u64>>,
    repaint: Arc<Mutex<Option<eframe::egui::Context>>>,
    receiver: Receiver<Reply>,
    generations: Arc<Mutex<HashMap<String, u64>>>,
}
impl Controller {
    pub fn new(core: Arc<App>) -> Self {
        let (sender, commands) = mpsc::sync_channel::<Command>(32);
        let (data_sender, data_commands) = mpsc::sync_channel::<Command>(16);
        let (action_sender, action_commands) = mpsc::sync_channel::<Command>(16);
        let (replies, receiver) = mpsc::channel();
        let generations = Arc::new(Mutex::new(HashMap::<String, u64>::new()));
        let repaint = Arc::new(Mutex::new(None::<eframe::egui::Context>));
        for (name, commands) in [
            ("getrich-ui-fast", commands),
            ("getrich-ui-data", data_commands),
            ("getrich-ui-actions", action_commands),
        ] {
            let core = Arc::clone(&core);
            let latest = Arc::clone(&generations);
            let replies = replies.clone();
            let repaint = Arc::clone(&repaint);
            if let Err(error) = std::thread::Builder::new()
                .name(name.into())
                .spawn(move || {
                    while let Ok(command) = commands.recv() {
                        if command.tag.starts_with("read:")
                            && latest
                                .lock()
                                .ok()
                                .and_then(|g| g.get(&command.tag).copied())
                                != Some(command.generation)
                        {
                            continue;
                        }
                        let outcome =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                (command.operation)(&core)
                            }))
                            .unwrap_or_else(|_| Err("操作遇到无效输入，已隔离本次错误".into()));
                        if replies
                            .send(Reply {
                                tag: command.tag,
                                generation: command.generation,
                                result: outcome,
                            })
                            .is_err()
                        {
                            break;
                        }
                        if let Some(ctx) = repaint.lock().ok().and_then(|ctx| ctx.clone()) {
                            ctx.request_repaint();
                        }
                    }
                })
            {
                tracing::error!(%error, "界面后台服务启动失败");
            }
        }
        Self {
            sender,
            data_sender,
            action_sender,
            receiver,
            generations,
            pending: Mutex::new(HashMap::new()),
            repaint,
        }
    }
    pub fn set_repaint(&self, ctx: &eframe::egui::Context) {
        if let Ok(mut target) = self.repaint.lock() {
            if target.is_none() {
                *target = Some(ctx.clone());
            }
        }
    }
    pub fn is_pending(&self, tag: &str) -> bool {
        self.pending.lock().is_ok_and(|p| p.contains_key(tag))
    }
    pub fn submit(
        &self,
        tag: &str,
        operation: impl FnOnce(&App) -> Result<Payload, String> + Send + 'static,
    ) -> Result<(), String> {
        self.enqueue(tag, operation, false)
    }
    /// A timer must never invalidate the response it is waiting for.
    pub fn submit_background(
        &self,
        tag: &str,
        operation: impl FnOnce(&App) -> Result<Payload, String> + Send + 'static,
    ) -> Result<(), String> {
        self.enqueue(tag, operation, true)
    }
    fn enqueue(
        &self,
        tag: &str,
        operation: impl FnOnce(&App) -> Result<Payload, String> + Send + 'static,
        coalesce: bool,
    ) -> Result<(), String> {
        let mut generations = self.generations.lock().map_err(|e| e.to_string())?;
        let mut pending = self.pending.lock().map_err(|e| e.to_string())?;
        if coalesce && pending.contains_key(tag) {
            return Ok(());
        }
        let generation = generations.get(tag).copied().unwrap_or(0) + 1;
        let sender = if tag.starts_with("action:") {
            &self.action_sender
        } else if tag.starts_with("read:workbench")
            || [
                "read:map",
                "read:stock",
                "read:research",
                "read:stats",
                "read:screen",
                "read:note-history",
            ]
            .contains(&tag)
        {
            &self.data_sender
        } else {
            &self.sender
        };
        sender
            .try_send(Command {
                tag: tag.into(),
                generation,
                operation: Box::new(operation),
            })
            .map_err(|_| "界面操作队列忙，请稍后重试")?;
        generations.insert(tag.into(), generation);
        pending.insert(tag.into(), generation);
        Ok(())
    }
    pub fn drain(&self) -> Vec<Reply> {
        self.receiver
            .try_iter()
            .filter(|reply| {
                let latest = self
                    .generations
                    .lock()
                    .ok()
                    .and_then(|g| g.get(&reply.tag).copied())
                    == Some(reply.generation);
                if let Ok(mut pending) = self.pending.lock() {
                    if pending.get(&reply.tag) == Some(&reply.generation) {
                        pending.remove(&reply.tag);
                    }
                }
                latest
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slow_history_and_network_actions_cannot_starve_quotes_or_invalidate_a_waiting_refresh() {
        use std::time::Duration;
        let core = Arc::new(App::at_path(
            Default::default(),
            std::env::temp_dir().join("getrich-controller-lanes.db"),
        ));
        let controller = Controller::new(core);
        let (started, ready) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        controller
            .submit("read:stock", move |_| {
                started.send(()).unwrap();
                blocked.recv().unwrap();
                Ok(Payload::Action("history".into(), Value::Null))
            })
            .unwrap();
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let (started, ready) = mpsc::channel();
        let (release_action, blocked) = mpsc::channel();
        controller
            .submit("action:network-test", move |_| {
                started.send(()).unwrap();
                blocked.recv().unwrap();
                Ok(Payload::Action("action".into(), Value::Null))
            })
            .unwrap();
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        for _ in 0..100 {
            controller
                .submit_background("read:stock", |_| {
                    panic!("duplicate refresh must be coalesced")
                })
                .unwrap();
        }
        controller
            .submit_background("read:quote", |_| {
                Ok(Payload::Action("quote".into(), Value::Null))
            })
            .unwrap();
        let reply = controller
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert_eq!(reply.tag, "read:quote");
        release.send(()).unwrap();
        release_action.send(()).unwrap();
        let mut history = false;
        for _ in 0..2 {
            let reply = controller
                .receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            if reply.tag == "read:stock" {
                assert_eq!(reply.generation, 1);
                assert!(reply.result.is_ok());
                history = true;
            }
        }
        assert!(
            history,
            "a slow accepted history response must remain publishable"
        );
    }
    #[test]
    fn a_full_queue_does_not_invalidate_the_last_accepted_read() {
        let (sender, commands) = mpsc::sync_channel(1);
        let (replies, receiver) = mpsc::channel();
        let controller = Controller {
            data_sender: sender.clone(),
            action_sender: sender.clone(),
            pending: Mutex::new(HashMap::new()),
            repaint: Arc::new(Mutex::new(None)),
            sender,
            receiver,
            generations: Arc::new(Mutex::new(HashMap::new())),
        };
        controller
            .submit("read:map", |_| Err("fixture".into()))
            .unwrap();
        assert!(controller
            .submit("read:map", |_| Err("rejected".into()))
            .is_err());
        let command = commands.recv().unwrap();
        replies
            .send(Reply {
                tag: command.tag,
                generation: command.generation,
                result: Err("fixture".into()),
            })
            .unwrap();
        assert_eq!(
            controller.drain().len(),
            1,
            "accepted reply must survive queue saturation"
        );
    }
}
