//! Batch quote acquisition by exchange, with room left in the quota for manual reads.
use super::*;
use std::collections::{BTreeMap, HashSet};

pub fn quote_batches(codes: &str) -> Vec<String> {
    let mut exchanges = BTreeMap::<String, Vec<String>>::new();
    let mut seen = HashSet::new();
    for code in codes.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let code = code.to_ascii_uppercase();
        if seen.insert(code.clone()) {
            let suffix = code
                .rsplit_once('.')
                .map(|(_, s)| s)
                .unwrap_or(&code)
                .to_string();
            exchanges.entry(suffix).or_default().push(code);
        }
    }
    exchanges
        .values()
        .flat_map(|codes| codes.chunks(50).map(|c| c.join(",")))
        .collect()
}

pub fn effective_poll_ms(requested: u64, batch_count: usize, rate_per_min: u32) -> u64 {
    // rt_k has its own vendor limit; reserve 20% for user-triggered quote reads.
    let usable = (u64::from(rate_per_min.clamp(1, 50)) * 4 / 5).max(1);
    requested
        .max(
            60_000u64
                .saturating_mul(batch_count.max(1) as u64)
                .div_ceil(usable),
        )
        .max(1000)
}

impl Client {
    pub fn live_board_bundle(&self, batches: &[String]) -> Result<ImportBundle, AppError> {
        let today = session::iso_date(session::shanghai_today());
        self.live_board_at(batches, &today)
    }
    pub(super) fn live_board_at(
        &self,
        batches: &[String],
        date: &str,
    ) -> Result<ImportBundle, AppError> {
        if batches.is_empty() {
            return Err(AppError::Message("实时行情未配置股票范围".into()));
        }
        let mut bundle = ImportBundle::default();
        let mut seen = HashSet::new();
        // Bound concurrency to the three exchanges. Quotas remain shared and each
        // chunk must succeed in full before any result can reach the hot writer.
        for chunk in batches.chunks(3) {
            let tables = std::thread::scope(|scope| {
                let reads: Vec<_> = chunk
                    .iter()
                    .map(|batch| scope.spawn(move || self.rt_k_table(batch)))
                    .collect();
                reads
                    .into_iter()
                    .map(|read| {
                        read.join().unwrap_or_else(|_| {
                            Err(AppError::Message("实时行情读取线程异常；本轮未提交".into()))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()
            })?;
            for (batch, table) in chunk.iter().zip(tables) {
                if table.items.len() >= 6000 {
                    return Err(AppError::Message(
                        "rt_k 单批触及 6000 条上限，请缩小股票批次；本轮未提交".into(),
                    ));
                }
                let mapped = map_rt_k(&table);
                let quotes: Vec<_> = mapped
                    .quotes
                    .into_iter()
                    .filter(|q| {
                        session::calendar_date_of(&q.ts)
                            .map(|d| d.to_string())
                            .as_deref()
                            == Some(date)
                    })
                    .collect();
                if quotes.is_empty() {
                    return Err(AppError::Message(format!(
                        "rt_k 批次 {batch} 没有 {date} 的有效报价；保留原有行情"
                    )));
                }
                for q in quotes {
                    if seen.insert((q.market.clone(), q.symbol.clone())) {
                        bundle.quotes.push(q);
                    }
                }
                bundle
                    .bars
                    .extend(mapped.bars.into_iter().filter(|b| b.ts == date));
            }
        }
        Ok(bundle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn polling_calendar_uses_the_live_timeout_instead_of_the_ninety_second_research_timeout() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut b = [0];
                if stream.read_exact(&mut b).is_err() {
                    return;
                }
                header.push(b[0]);
            }
            let length = String::from_utf8(header)
                .unwrap()
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            let mut body = vec![0; length];
            let _ = stream.read_exact(&mut body);
            std::thread::sleep(Duration::from_millis(1500));
            let reply =
                json!({"code":0,"data":{"fields":["cal_date","is_open"],"items":[["20261027",1]]}})
                    .to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                reply.len(),
                reply
            );
        });
        let mut client =
            Client::new("".into(), "fixture-calendar-timeout".into(), 1000, 1000, 50).unwrap();
        client.endpoint = endpoint;
        assert!(
            client.poll_open_day().is_err(),
            "a slow calendar must not hold realtime polling for 90 seconds"
        );
        server.join().unwrap();
    }
    #[test]
    fn full_market_is_three_exchange_batches_and_interval_respects_quota() {
        let batches = quote_batches("6*.SH,0*.SZ,3*.SZ,1*.SH,8*.BJ,4*.BJ,9*.BJ,6*.SH");
        assert_eq!(batches.len(), 3);
        assert!(batches.contains(&"0*.SZ,3*.SZ".into()));
        assert_eq!(effective_poll_ms(1000, batches.len(), 50), 4500);
        assert_eq!(effective_poll_ms(5000, batches.len(), 50), 5000);
        assert_eq!(effective_poll_ms(1000, 7, 50), 10500);
        assert_eq!(effective_poll_ms(1000, 3, 500), 4500);
    }

    #[test]
    fn realtime_rows_without_a_source_time_are_not_stamped_with_the_current_time() {
        let table = Table {
            fields: vec!["ts_code".into(), "close".into()],
            items: vec![json!(["600001.SH", 10.0])],
        };
        assert!(map_rt_k(&table).quotes.is_empty());
    }

    #[test]
    fn three_batches_are_in_flight_before_the_first_response_and_merge_atomically() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::Instant,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let started = Instant::now();
            let mut streams = Vec::new();
            while streams.len() < 3 && started.elapsed() < Duration::from_secs(4) {
                match listener.accept() {
                    Ok((stream, _)) => streams.push(stream),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
            assert_eq!(
                streams.len(),
                3,
                "all three requests must start without awaiting the first response"
            );
            for (i, mut stream) in streams.into_iter().enumerate() {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    head.push(byte[0]);
                    assert!(head.len() < 16384);
                }
                let header = String::from_utf8(head).unwrap();
                let length = header
                    .lines()
                    .find_map(|line| {
                        line.split_once(':')
                            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                stream.read_exact(&mut vec![0; length]).unwrap();
                let body=json!({"code":0,"data":{"fields":["ts_code","close","trade_time"],"items":[[format!("60000{}.SH",i+1),10.0,"2026-10-23 10:00:00"]]}}).to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            }
        });
        let mut client = Client::new("".into(), "fixture".into(), 1000, 1000, 50).unwrap();
        client.endpoint = endpoint;
        let bundle = client
            .live_board_at(
                &["600001.SH".into(), "600002.SH".into(), "600003.SH".into()],
                "2026-10-23",
            )
            .unwrap();
        assert_eq!(bundle.quotes.len(), 3);
        assert_eq!(
            bundle
                .quotes
                .iter()
                .map(|q| &q.symbol)
                .collect::<HashSet<_>>()
                .len(),
            3
        );
        server.join().unwrap();
    }
    #[test]
    fn missing_or_limited_second_exchange_cannot_commit_a_partial_live_batch() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", server.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            for reply in [
                json!({"code":0,"data":{"fields":["ts_code","close","trade_time"],"items":[["600001.SH",10.0,"2026-10-23 10:00:00"]]}}),
                json!({"code":-2001,"msg":"频率超限(1次/小时)"}),
            ] {
                let (mut stream, _) = server.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    head.push(byte[0]);
                    assert!(head.len() < 16_384);
                }
                let header = String::from_utf8(head).unwrap();
                let length = header
                    .lines()
                    .find_map(|line| {
                        line.split_once(':')
                            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, n)| n.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let mut body = vec![0; length];
                stream.read_exact(&mut body).unwrap();
                let body = reply.to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            }
        });
        let mut c = Client::new("".into(), "fixture".into(), 1000, 1000, 50).unwrap();
        c.endpoint = endpoint;
        let error = c
            .live_board_at(&["6*.SH".into(), "0*.SZ".into()], "2026-10-23")
            .unwrap_err()
            .to_string();
        assert!(error.contains("1次/小时"), "{error}");
        assert!(c.shared_take_at("rt_k", now_ms()).unwrap() > 3_500_000);
        assert_eq!(c.shared_take_at("daily", now_ms()).unwrap(), 0);
        thread.join().unwrap();
    }
}
