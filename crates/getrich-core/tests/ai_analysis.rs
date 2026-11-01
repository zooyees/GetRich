//! The model is a loopback fixture: these tests never contact a paid provider.
use getrich_core::{
    ai::SECTIONS,
    import::{parse_json, ImportKind},
    service::App,
};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::mpsc,
    time::{Duration, Instant},
};
fn workspace() -> (App, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "getrich-ai-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let app = App::at_path(Default::default(), root.join("test.db"));
    app.migrate().unwrap();
    let bundle=parse_json(&json!({"listings":[{"symbol":"600519","market":"CN.SH","name":"测试公司"}],"bars":[{"symbol":"600519","market":"CN.SH","ts":"2024-01-03","open":10,"high":11,"low":9,"close":10}]}).to_string(),ImportKind::Bundle).unwrap();
    app.import_bundle(&bundle, "synthetic.qa").unwrap();
    (app, root)
}
type Mock = (
    String,
    mpsc::Receiver<String>,
    mpsc::Sender<()>,
    std::thread::JoinHandle<()>,
);
fn server(status: u16, response: Value) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let (tx, rx) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let start = Instant::now();
        let (mut stream, _) = loop {
            if let Ok(s) = listener.accept() {
                break s;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "model request missing"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut r = BufReader::new(stream.try_clone().unwrap());
        let mut headers = String::new();
        let mut len = 0;
        loop {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                if k.eq_ignore_ascii_case("content-length") {
                    len = v.trim().parse::<usize>().unwrap();
                }
            }
            headers.push_str(&line);
        }
        let mut body = vec![0; len];
        r.read_exact(&mut body).unwrap();
        tx.send(format!("{headers}\n{}", String::from_utf8(body).unwrap()))
            .unwrap();
        gate.recv_timeout(Duration::from_secs(8)).unwrap();
        let body = response.to_string();
        write!(stream,"HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    (url, rx, release, handle)
}
fn report() -> Value {
    json!({"summary":"样本不足，暂不能得出确定趋势判断。","sections":SECTIONS.iter().map(|(id,_)|json!({"key":id,"text":"仅依据所列样本，仍需补充数据。","evidence":["E2"]})).collect::<Vec<_>>()})
}
fn wait(app: &App, p: &Value) -> Value {
    let start = Instant::now();
    loop {
        let r = app.ai_analysis_get(p).unwrap();
        if r["latest"]["status"] != "running" {
            return r;
        }
        assert!(
            start.elapsed() < Duration::from_secs(8),
            "model result not persisted"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn asynchronous_generation_is_persisted_deduplicated_and_excludes_personal_data() {
    let (app, root) = workspace();
    let (url, rx, release, handle) = server(
        200,
        json!({"choices":[{"finish_reason":"stop","message":{"content":report().to_string()}}],"usage":{"total_tokens":32}}),
    );
    app.save_settings(&json!({"ai":{"enabled":true,"base_url":url,"model":"fixture","api_key":"TEST_MODEL_SECRET"}})).unwrap();
    let p = json!({"symbol":"600519","market":"CN.SH","mode":"swing","basis":"raw"});
    app.save_research_note(&json!({"symbol":"600519","market":"CN.SH","mode":"swing","revision":0,"thesis":"PRIVATE_NOTE_MARKER","risks":"","plan":"","review":""})).unwrap();
    let generated = app.ai_analysis_generate(&p).unwrap();
    let request = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(request.contains("POST /v1/chat/completions"));
    assert!(request.contains("TEST_MODEL_SECRET"));
    let payload = request.split_once("\n\n").unwrap().1;
    let payload: Value = serde_json::from_str(payload).unwrap();
    let text = payload["messages"][1]["content"].as_str().unwrap();
    assert!(!text.contains("PRIVATE_NOTE_MARKER"));
    assert!(!text.contains("TEST_MODEL_SECRET"));
    assert!(text.contains("synthetic.qa"));
    let duplicate = app.ai_analysis_generate(&p).unwrap();
    assert_eq!(generated["id"], duplicate["id"]);
    assert_eq!(duplicate["reused"], true);
    let other = json!({"symbol":"600519","market":"CN.SH","mode":"value"});
    assert!(app
        .ai_analysis_generate(&other)
        .unwrap_err()
        .to_string()
        .contains("已有 AI 分析"));
    // A pending model cannot block quote, settings or database reads.
    assert_eq!(app.stats().unwrap().daily_bars, 1);
    assert!(app.settings_view().unwrap()["ai"]["api_key"].is_null());
    release.send(()).unwrap();
    handle.join().unwrap();
    let saved = wait(&app, &p);
    assert_eq!(saved["latest"]["status"], "succeeded");
    assert_eq!(saved["latest"]["data_changed"], false);
    assert_eq!(
        saved["latest"]["report"]["sections"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    let reused = app.ai_analysis_generate(&p).unwrap();
    assert_eq!(reused["id"], generated["id"]);
    assert_eq!(reused["reused"], true);
    app.save_research_note(&json!({"symbol":"600519","market":"CN.SH","mode":"swing","revision":1,"thesis":"PRIVATE_NOTE_MARKER","risks":"新增私人风险备注","plan":"","review":""})).unwrap();
    assert_eq!(
        app.ai_analysis_generate(&p).unwrap()["id"],
        generated["id"],
        "personal notes must not invalidate a public-evidence model cache"
    );
    let reopened = App::at_path(Default::default(), root.join("test.db"));
    assert_eq!(
        reopened.ai_analysis_get(&p).unwrap()["latest"]["id"],
        generated["id"]
    );
    let private = app.research_notes(&p, false).unwrap();
    assert_eq!(private["thesis"], "PRIVATE_NOTE_MARKER");
    drop(reopened);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn invalid_model_report_is_failed_and_never_saved_as_success() {
    let (app, root) = workspace();
    let (url, rx, release, handle) = server(
        200,
        json!({"choices":[{"message":{"content":"not a JSON report"},"finish_reason":"stop"}]}),
    );
    app.save_settings(&json!({"ai":{"enabled":true,"base_url":url,"model":"fixture"}}))
        .unwrap();
    let p = json!({"symbol":"600519","market":"CN.SH"});
    app.ai_analysis_generate(&p).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    release.send(()).unwrap();
    handle.join().unwrap();
    let r = wait(&app, &p);
    assert_eq!(r["latest"]["status"], "failed");
    assert!(r["latest"]["report"].is_null());
    assert_eq!(
        app.ai_analysis_get(&p).unwrap()["history"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn provider_errors_are_sanitized_and_restart_recovers_expired_requests() {
    let (app, root) = workspace();
    let (url, rx, release, handle) = server(401, json!({"error":"DO_NOT_LOG_VENDOR_SECRET"}));
    app.save_settings(&json!({"ai":{"enabled":true,"base_url":url,"model":"fixture"}}))
        .unwrap();
    let p = json!({"symbol":"600519","market":"CN.SH"});
    let first = app.ai_analysis_generate(&p).unwrap();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    release.send(()).unwrap();
    handle.join().unwrap();
    let failed = wait(&app, &p);
    assert!(failed["latest"]["error"].as_str().unwrap().contains("401"));
    assert!(!failed.to_string().contains("DO_NOT_LOG_VENDOR_SECRET"));
    let c = app.db.connect().unwrap();
    c.execute(
        "UPDATE ai_analyses SET status='running',created_at='2024-01-01T00:00:00Z' WHERE id=?1",
        [first["id"].as_i64().unwrap()],
    )
    .unwrap();
    assert_eq!(
        app.ai_analysis_get(&p).unwrap()["latest"]["status"],
        "failed"
    );
    drop(c);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn configuration_validation_is_transactional_and_disabled_model_never_submits() {
    let (app, root) = workspace();
    let p = json!({"symbol":"600519","market":"CN.SH"});
    assert!(app.ai_analysis_generate(&p).is_err());
    assert!(app
        .save_settings(
            &json!({"ai":{"enabled":true,"base_url":"http://public.example/v1","model":"fixture"}})
        )
        .is_err());
    assert_eq!(app.settings_view().unwrap()["ai"]["enabled"], false);
    assert!(app.ai_analysis_get(&p).unwrap()["latest"].is_null());
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
