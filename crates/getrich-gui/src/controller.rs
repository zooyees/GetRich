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
    Research(getrich_core::models::InstrumentKey, Value),
    Stats(StoreStats),
    Settings(Value),
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
    receiver: Receiver<Reply>,
    generations: Arc<Mutex<HashMap<String, u64>>>,
}
impl Controller {
    pub fn new(core: Arc<App>) -> Self {
        let (sender, commands) = mpsc::sync_channel::<Command>(32);
        let (replies, receiver) = mpsc::channel();
        let generations = Arc::new(Mutex::new(HashMap::<String, u64>::new()));
        let latest = Arc::clone(&generations);
        let spawned = std::thread::Builder::new()
            .name("getrich-ui-io".into())
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
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
                }
            });
        if let Err(error) = spawned {
            tracing::error!(%error,"界面后台服务启动失败");
        }
        Self {
            sender,
            receiver,
            generations,
        }
    }
    pub fn submit(
        &self,
        tag: &str,
        operation: impl FnOnce(&App) -> Result<Payload, String> + Send + 'static,
    ) -> Result<(), String> {
        let mut generations = self.generations.lock().map_err(|e| e.to_string())?;
        let generation = generations.entry(tag.into()).or_default();
        *generation += 1;
        self.sender
            .try_send(Command {
                tag: tag.into(),
                generation: *generation,
                operation: Box::new(operation),
            })
            .map_err(|_| "界面操作队列忙，请稍后重试".into())
    }
    pub fn drain(&self) -> Vec<Reply> {
        self.receiver
            .try_iter()
            .filter(|reply| {
                self.generations
                    .lock()
                    .ok()
                    .and_then(|g| g.get(&reply.tag).copied())
                    == Some(reply.generation)
            })
            .collect()
    }
}
