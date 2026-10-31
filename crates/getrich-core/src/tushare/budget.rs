//! Per-account, per-interface quotas. Existing schema 6 tables store namespaced keys.
use super::*;
use std::collections::HashMap;

const APIS: &[&str] = &[
    "rt_k",
    "daily",
    "stock_basic",
    "trade_cal",
    "daily_basic",
    "index_daily",
    "stk_mins",
    "rt_min_daily",
    "stock_company",
    "fina_indicator",
    "top10_holders",
    "moneyflow",
    "anns_d",
];

#[derive(Debug, Default)]
pub(super) struct Memory {
    hits: HashMap<String, VecDeque<u64>>,
    blocks: HashMap<(String, String), u64>,
}

fn namespace(account: &str, api: &str) -> String {
    format!("{account}:{api}")
}

/// Honor vendor hour/day cooldowns instead of retrying all interfaces every minute.
pub(crate) fn cooldown_ms(message: &str) -> u64 {
    if let Some(seconds) = message
        .split("请在 ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse::<u64>().ok())
    {
        return seconds.clamp(1, 86_400) * 1000;
    }
    let lower = message.to_ascii_lowercase();
    if lower.contains("小时") || lower.contains("/hour") || lower.contains("per hour") {
        3_600_000
    } else if lower.contains("每天")
        || lower.contains("/天")
        || lower.contains("/day")
        || lower.contains("per day")
    {
        86_400_000
    } else {
        60_000
    }
}
pub(crate) fn permission_denied(message: &str) -> bool {
    let s = message.to_ascii_lowercase();
    [
        "没有接口",
        "没有访问",
        "无访问权限",
        "无权限",
        "权限不足",
        "未开通",
        "permission denied",
        "no permission",
        "没有权限",
    ]
    .iter()
    .any(|v| s.contains(v))
}

impl Client {
    pub(crate) fn api_limit(&self, api: &str) -> u32 {
        if api == "rt_k" {
            self.per_min.min(50)
        } else {
            self.per_min
        }
    }
    fn active_blocks_at(&self, api: &str, now: u64) -> Result<Vec<(String, u64)>, AppError> {
        let key = namespace(&self.account, api);
        if let Some(store) = &self.shared_store {
            let conn = store.connect()?;
            let mut out = Vec::new();
            for kind in ["rate", "permission"] {
                let until: u64 = conn
                    .query_row(
                        "SELECT COALESCE(MAX(until_ms),0) FROM provider_backoff WHERE account=?1",
                        [format!("{key}:{kind}")],
                        |r| r.get(0),
                    )
                    .map_err(crate::db::DbError::from)?;
                if until > now {
                    out.push((kind.into(), until));
                }
            }
            Ok(out)
        } else {
            let memory = self.budget.lock().unwrap_or_else(|e| e.into_inner());
            Ok(memory
                .blocks
                .iter()
                .filter(|((a, _), until)| a == api && **until > now)
                .map(|((_, kind), until)| (kind.clone(), *until))
                .collect())
        }
    }
    pub(super) fn shared_take_at(&self, api: &str, now: u64) -> Result<u64, AppError> {
        let key = namespace(&self.account, api);
        if let Some(store) = &self.shared_store {
            let mut conn = store.connect()?;
            let tx = conn
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(crate::db::DbError::from)?;
            let blocked:u64=tx.query_row("SELECT COALESCE(MAX(until_ms),0) FROM provider_backoff WHERE account IN (?1,?2)",rusqlite::params![format!("{key}:rate"),format!("{key}:permission")],|r|r.get(0)).map_err(crate::db::DbError::from)?;
            if blocked > now {
                return Ok(blocked - now);
            }
            tx.execute(
                "DELETE FROM provider_requests WHERE requested_at_ms<=?1",
                [now.saturating_sub(60_000)],
            )
            .map_err(crate::db::DbError::from)?;
            let (count,oldest):(u32,u64)=tx.query_row("SELECT COUNT(*),COALESCE(MIN(requested_at_ms),0) FROM provider_requests WHERE account=?1",[&key],|r|Ok((r.get(0)?,r.get(1)?))).map_err(crate::db::DbError::from)?;
            if count >= self.api_limit(api) {
                return Ok(oldest.saturating_add(60_000).saturating_sub(now).max(1));
            }
            tx.execute(
                "INSERT INTO provider_requests(account,requested_at_ms) VALUES (?1,?2)",
                rusqlite::params![key, now],
            )
            .map_err(crate::db::DbError::from)?;
            tx.commit().map_err(crate::db::DbError::from)?;
            Ok(0)
        } else {
            let mut memory = self.budget.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(until) = memory
                .blocks
                .iter()
                .filter(|((a, _), _)| a == api)
                .map(|(_, v)| *v)
                .max()
                .filter(|t| *t > now)
            {
                return Ok(until - now);
            }
            Ok(admit(
                memory.hits.entry(api.into()).or_default(),
                now,
                self.api_limit(api),
            )
            .err()
            .unwrap_or(0))
        }
    }
    pub(super) fn shared_block(&self, api: &str, kind: &str, until: u64) -> Result<(), AppError> {
        if let Some(store) = &self.shared_store {
            store.connect()?.execute("INSERT INTO provider_backoff(account,until_ms) VALUES (?1,?2) ON CONFLICT(account) DO UPDATE SET until_ms=MAX(until_ms,excluded.until_ms)",rusqlite::params![format!("{}:{kind}",namespace(&self.account,api)),until]).map_err(crate::db::DbError::from)?;
        } else {
            let mut memory = self.budget.lock().unwrap_or_else(|e| e.into_inner());
            let entry = memory.blocks.entry((api.into(), kind.into())).or_default();
            *entry = (*entry).max(until);
        }
        Ok(())
    }
    pub(super) fn record_failure_at(
        &self,
        api: &str,
        message: &str,
        now: u64,
    ) -> Result<(), AppError> {
        if permission_denied(message) {
            self.shared_block(api, "permission", now.saturating_add(600_000))?;
        } else if is_rate_limited(&AppError::Message(message.into())) {
            self.shared_block(api, "rate", now.saturating_add(cooldown_ms(message)))?;
        }
        Ok(())
    }
    pub(crate) fn retry_after_ms(&self, api: &str) -> Result<u64, AppError> {
        let now = now_ms();
        Ok(self
            .active_blocks_at(api, now)?
            .into_iter()
            .map(|(_, until)| until.saturating_sub(now))
            .max()
            .unwrap_or(0))
    }
    pub(crate) fn api_limits(&self) -> Result<Value, AppError> {
        let now = now_ms();
        let mut blocks = Vec::new();
        if let Some(store) = &self.shared_store {
            let conn = store.connect()?;
            let mut stmt = conn.prepare("SELECT account,until_ms FROM provider_backoff WHERE until_ms>?1 AND account LIKE ?2").map_err(crate::db::DbError::from)?;
            let entries = stmt
                .query_map(rusqlite::params![now, format!("{}:%", self.account)], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?))
                })
                .map_err(crate::db::DbError::from)?;
            for entry in entries {
                let (key, until) = entry.map_err(crate::db::DbError::from)?;
                for api in APIS {
                    for kind in ["rate", "permission"] {
                        if key == format!("{}:{kind}", namespace(&self.account, api)) {
                            blocks.push((api.to_string(), kind.to_string(), until));
                        }
                    }
                }
            }
        } else {
            let memory = self.budget.lock().unwrap_or_else(|e| e.into_inner());
            blocks.extend(
                memory
                    .blocks
                    .iter()
                    .filter(|(_, until)| **until > now)
                    .map(|((api, kind), until)| (api.clone(), kind.clone(), *until)),
            );
        }
        Ok(json!(blocks.into_iter().map(|(api,kind,until)|json!({"api":api,"kind":kind,"remaining_seconds":(until-now).div_ceil(1000),"retry_at_ms":until,"message":if kind=="permission" {"权限不可用，暂缓重试；请核对该接口授权"}else{"该接口限流冷却，其他接口继续可用"}})).collect::<Vec<_>>()))
    }
    pub(super) fn check_budget(&self, api: &str) -> Result<(), AppError> {
        let now = now_ms();
        let wait = self.shared_take_at(api, now)?;
        if wait > 0 {
            let permission = self
                .active_blocks_at(api, now)?
                .iter()
                .any(|(kind, _)| kind == "permission");
            return Err(AppError::Message(format!(
                "Tushare {api}: {}，请在 {} 秒后重试；其他接口不受影响",
                if permission {
                    "接口权限不可用，已暂停重复请求"
                } else {
                    "该接口额度等待/限流冷却"
                },
                wait.div_ceil(1000)
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hourly_and_permission_cooldowns_do_not_block_quotes_or_other_accounts() {
        let root = std::env::temp_dir().join(format!(
            "getrich-api-budget-{}-{}.db",
            std::process::id(),
            now_ms()
        ));
        let store = crate::db::Database::at(&root);
        let first = Client::new("".into(), "fixture-budget".into(), 1000, 1000, 2)
            .unwrap()
            .with_shared_store(store.clone());
        let second = Client::new("".into(), "fixture-budget".into(), 1000, 1000, 2)
            .unwrap()
            .with_shared_store(store.clone());
        let other = Client::new("".into(), "other-budget".into(), 1000, 1000, 2)
            .unwrap()
            .with_shared_store(store);
        first
            .record_failure_at("stk_mins", "频率超限(1次/小时)", 100_000)
            .unwrap();
        assert_eq!(
            second.shared_take_at("stk_mins", 100_001).unwrap(),
            3_599_999
        );
        assert_eq!(second.shared_take_at("rt_k", 100_001).unwrap(), 0);
        assert_eq!(other.shared_take_at("stk_mins", 100_001).unwrap(), 0);
        first
            .record_failure_at("anns_d", "没有接口(anns_d)访问权限", 100_000)
            .unwrap();
        assert!(second.shared_take_at("anns_d", 100_001).unwrap() > 0);
        assert_eq!(second.shared_take_at("daily", 100_001).unwrap(), 0);
        assert_eq!(second.shared_take_at("stk_mins", 3_700_000).unwrap(), 0);
        let _ = std::fs::remove_file(root);
    }
    #[test]
    fn standalone_clients_enforce_independent_interface_windows() {
        let c = Client::new("".into(), "fixture".into(), 1000, 1000, 1).unwrap();
        assert_eq!(c.shared_take_at("rt_k", 100_000).unwrap(), 0);
        assert!(c.shared_take_at("rt_k", 100_001).unwrap() > 0);
        assert_eq!(c.shared_take_at("daily", 100_001).unwrap(), 0);
        c.record_failure_at("stk_mins", "频率超限(1次/小时)", 100_000)
            .unwrap();
        assert!(c.shared_take_at("stk_mins", 160_000).unwrap() > 0);
        assert_eq!(cooldown_ms("每天频次超限"), 86_400_000);
    }
}
