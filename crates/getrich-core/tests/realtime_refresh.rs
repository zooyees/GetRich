//! Explicit load audit; run with --ignored --nocapture on the target machine.
use getrich_core::{
    import::{self, ImportKind},
    market_map::MapQuery,
    service::App,
};
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

#[test]
#[ignore = "6000-stock concurrent refresh load audit"]
fn six_thousand_quotes_remain_atomic_across_continuous_updates() {
    let root = std::env::temp_dir().join(format!(
        "getrich-refresh-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let app = Arc::new(App::at_path(Default::default(), root.join("test.db")));
    app.migrate().unwrap();
    let listings:Vec<_>=(0..6000).map(|i|json!({"symbol":format!("{:06}",600000+i),"market":"CN.SH","industry":format!("行业{}",i%100),"currency":"CNY","metadata":[{"key":"float_market_cap","value":(100000000+i*1000).to_string(),"as_of":"2026-10-22","source":"fixture"}]})).collect();
    let bundle = import::parse_json(
        &json!({"listings":listings}).to_string(),
        ImportKind::Bundle,
    )
    .unwrap();
    app.import_bundle(&bundle, "fixture").unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let reader = {
        let app = Arc::clone(&app);
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            let mut versions = Vec::new();
            while !stop.load(Ordering::SeqCst) {
                let map = app
                    .market_map(MapQuery {
                        weight_policy: "latest_available".into(),
                        ..Default::default()
                    })
                    .unwrap();
                if map.coverage.displayed > 0 {
                    assert_eq!(map.coverage.displayed, 6000);
                    let first = map.groups[0].tiles[0].quote.last;
                    assert!(
                        map.groups
                            .iter()
                            .flat_map(|g| &g.tiles)
                            .all(|t| t.quote.last == first),
                        "a read must not mix two committed rounds"
                    );
                    versions.push(map.data_version);
                }
            }
            assert!(versions.windows(2).all(|w| w[1] >= w[0]));
            versions.len()
        })
    };
    let mut writes = Vec::new();
    for round in 0..8 {
        let price = 10.0 + round as f64 / 100.0;
        let quotes:Vec<_>=(0..6000).map(|i|json!({"symbol":format!("{:06}",600000+i),"market":"CN.SH","ts":format!("2026-10-23T02:00:{round:02}Z"),"last":price,"prev_close":10.0,"currency":"CNY","source":"fixture.rt_k"})).collect();
        let bundle =
            import::parse_json(&json!({"quotes":quotes}).to_string(), ImportKind::Bundle).unwrap();
        let start = Instant::now();
        let report =
            import::apply_quotes_hot(&app.db, &bundle.quotes, &[], "fixture.rt_k", 1000).unwrap();
        writes.push(start.elapsed().as_millis());
        assert_eq!(report.accepted, 6000);
        assert_eq!(report.invalid, 0);
        println!(
            "round={} accepted={} write_ms={}",
            round + 1,
            report.accepted,
            writes.last().unwrap()
        );
    }
    stop.store(true, Ordering::SeqCst);
    let reads = reader.join().unwrap();
    assert!(reads > 0);
    println!("refresh_audit={{\"stocks\":6000,\"rounds\":8,\"coherent_reads\":{reads},\"write_ms\":{writes:?}}}");
    drop(app);
    let _ = std::fs::remove_dir_all(root);
}
