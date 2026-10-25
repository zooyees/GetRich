use super::*;
use crate::{import::ImportBundle, models::InstrumentKey};
struct Provider {
    empty: bool,
    optional_failure: bool,
}
impl DataProvider for Provider {
    fn id(&self) -> &'static str {
        "fixture"
    }
    fn markets(&self) -> &'static [&'static str] {
        &["CN.SH"]
    }
    fn test_connection(&self) -> Result<Value, AppError> {
        Ok(json!({"ok":true}))
    }
    fn listings(&self) -> Result<ImportBundle, AppError> {
        Ok(serde_json::from_value(json!({"listings":[{"symbol":"600001","market":"CN.SH","name":"样本","industry":"银行","currency":"CNY"}]})).unwrap())
    }
    fn snapshot(&self) -> Result<ImportBundle, AppError> {
        if self.empty {
            return Ok(Default::default());
        }
        Ok(serde_json::from_value(json!({"quotes":[{"symbol":"600001","market":"CN.SH","ts":"2026-10-23","last":10.0,"prev_close":9.0,"turnover":1000000.0,"currency":"CNY","source":"fixture.daily"}]})).unwrap())
    }
    fn valuations(&self) -> Result<ImportBundle, AppError> {
        panic!("must request quote date")
    }
    fn indices(&self) -> Result<ImportBundle, AppError> {
        panic!("must request quote date")
    }
    fn valuations_at(&self, date: &str) -> Result<ImportBundle, AppError> {
        assert_eq!(date, "20261023");
        if self.optional_failure {
            return Err(AppError::Message("daily_basic 权限不足".into()));
        }
        Ok(serde_json::from_value(json!({"listings":[{"symbol":"600001","market":"CN.SH","metadata":[{"key":"float_market_cap","value":"1000000","as_of":"2026-10-23","source":"fixture"}]}]})).unwrap())
    }
    fn indices_at(&self, date: &str) -> Result<ImportBundle, AppError> {
        assert_eq!(date, "20261023");
        if self.optional_failure {
            return Err(AppError::Message("index_daily 权限不足".into()));
        }
        Ok(serde_json::from_value(json!({"listings":[{"symbol":"000001","market":"CN.SH","name":"上证指数","instrument_type":"index","currency":"CNY"}],"quotes":[{"symbol":"000001","market":"CN.SH","ts":"2026-10-23","last":3000.0,"prev_close":2990.0,"source":"fixture.index_daily"}]})).unwrap())
    }
    fn history(&self, _: &InstrumentKey, _: &str, _: &str) -> Result<ImportBundle, AppError> {
        Ok(Default::default())
    }
}
fn workspace() -> (App, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "getrich-map-sync-{}-{}",
        std::process::id(),
        session::shanghai_now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let app = App::at_path(Default::default(), root.join("test.db"));
    app.migrate().unwrap();
    (app, root)
}
#[test]
fn optional_permissions_do_not_discard_quotes_and_all_datasets_use_quote_date() {
    for optional_failure in [true, false] {
        let (app, root) = workspace();
        let task = app.submit_job("market.sync", json!({})).unwrap();
        let result = execute(
            &app,
            task["id"].as_i64().unwrap(),
            &Provider {
                empty: false,
                optional_failure,
            },
        )
        .unwrap();
        assert_eq!(
            result["status"],
            if optional_failure {
                "partial"
            } else {
                "succeeded"
            }
        );
        assert_eq!(result["trade_date"], "2026-10-23");
        assert_eq!(result["displayed"], 1);
        assert_eq!(
            result["metric"],
            if optional_failure {
                "turnover"
            } else {
                "float_market_cap"
            }
        );
        assert_eq!(
            result["warnings"].as_array().unwrap().len(),
            if optional_failure { 2 } else { 0 }
        );
        assert!(app.stats().unwrap().quotes >= 1);
        execute(
            &app,
            task["id"].as_i64().unwrap(),
            &Provider {
                empty: false,
                optional_failure,
            },
        )
        .unwrap();
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }
}
#[test]
fn empty_quotes_never_become_successful_refresh() {
    let (app, root) = workspace();
    let task = app.submit_job("market.sync", json!({})).unwrap();
    assert!(execute(
        &app,
        task["id"].as_i64().unwrap(),
        &Provider {
            empty: true,
            optional_failure: false
        }
    )
    .unwrap_err()
    .to_string()
    .contains("行情接口"));
    assert_eq!(app.stats().unwrap().listings, 0);
    assert_eq!(app.stats().unwrap().quotes, 0);
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
