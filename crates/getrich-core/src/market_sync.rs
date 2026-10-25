//! One refresh acquires quotes first; optional datasets cannot discard usable quotes.
use crate::{error::AppError, jobs::stage, providers::DataProvider, service::App, session};
use serde_json::{json, Value};
#[cfg(test)]
mod tests;

pub(crate) fn execute(app: &App, id: i64, provider: &dyn DataProvider) -> Result<Value, AppError> {
    stage(app, id, "获取股票与行业列表", 12)?;
    let listings = provider.listings()?;
    if listings.listings.is_empty() {
        return Err(AppError::Message(
            "股票列表为空，请检查 stock_basic 接口权限与数据返回".into(),
        ));
    }
    stage(app, id, "获取最新可用交易日行情", 25)?;
    let mut quotes = provider.snapshot()?;
    if quotes.quotes.is_empty() {
        return Err(AppError::Message(
            "行情接口未返回有效报价；连接测试成功不代表行情接口可用，请检查 daily 接口权限".into(),
        ));
    }
    let date = quotes
        .quotes
        .iter()
        .filter_map(|q| session::calendar_date_of(&q.ts))
        .max()
        .ok_or_else(|| AppError::Message("行情未返回有效交易日期".into()))?;
    let source_date = session::yyyymmdd(date);
    quotes.listings = listings.listings;
    stage(app, id, "保存行情与行业数据", 40)?;
    let quote_report = app.import_bundle(&quotes, provider.id())?;
    if quote_report.quotes_upserted == 0 && app.market_map(Default::default())?.coverage.quoted == 0
    {
        return Err(AppError::Message(
            "报价未通过校验，星图暂无有效数据；请查看导入异常记录".into(),
        ));
    }
    let mut warnings = Vec::<String>::new();
    if quote_report.status == "partial" {
        warnings.push("部分行情记录未通过校验，请查看导入异常记录".into());
    }
    let mut datasets = json!({"quotes":quote_report});
    for (name, label, progress) in [
        ("valuations", "同日流通市值", 55),
        ("indices", "同日指数日线", 78),
    ] {
        stage(app, id, &format!("获取{label}"), progress)?;
        let outcome = (|| {
            let bundle = if name == "valuations" {
                provider.valuations_at(&source_date)?
            } else {
                provider.indices_at(&source_date)?
            };
            if (name == "valuations" && bundle.listings.is_empty())
                || (name == "indices" && bundle.quotes.is_empty())
            {
                return Err(AppError::Message(format!("{label}接口返回空数据")));
            }
            stage(app, id, &format!("保存{label}"), progress + 8)?;
            let report = app.import_bundle(&bundle, provider.id())?;
            if report.status == "partial" {
                warnings.push(format!("{label}部分记录未通过校验"));
            }
            serde_json::to_value(report).map_err(|e| AppError::Message(e.to_string()))
        })();
        match outcome {
            Ok(value) => datasets[name] = value,
            Err(e) if e.to_string() == "任务已取消" => return Err(e),
            Err(e) => {
                let error = e.to_string();
                warnings.push(format!("{label}：{error}"));
                datasets[name] = json!({"status":"failed","error":error});
            }
        }
    }
    stage(app, id, "更新星图截面", 96)?;
    let map = app.market_map(Default::default())?;
    Ok(
        json!({"status":if warnings.is_empty(){"succeeded"}else{"partial"},"trade_date":date.to_string(),"displayed":map.coverage.displayed,"metric":map.metric,"datasets":datasets,"warnings":warnings,"message":format!("已更新 {} 只同日有效股票",map.coverage.quoted)}),
    )
}
