use crate::{config::Config, metrics::Metrics, record::Record, resp};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::{
    collections::HashMap,
    io::{BufReader, Write},
    net::{TcpStream, ToSocketAddrs},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const QUEUE_LIMIT: u64 = 16 * 1024 * 1024;

pub struct Collector {
    cfg: Config,
    metrics: Arc<Metrics>,
    stop: Arc<AtomicBool>,
    agent: ureq::Agent,
    names: HashMap<String, String>,
    credential_notes: HashMap<String, String>,
    next_refresh: Instant,
}

impl Collector {
    pub fn new(cfg: Config, metrics: Arc<Metrics>, stop: Arc<AtomicBool>) -> Self {
        Self {
            cfg,
            metrics,
            stop,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(15))
                .redirects(0)
                .build(),
            names: HashMap::new(),
            credential_notes: HashMap::new(),
            next_refresh: Instant::now(),
        }
    }
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
    fn wait(&self, duration: Duration) {
        let end = Instant::now() + duration;
        while !self.stopped() && Instant::now() < end {
            std::thread::sleep(
                end.saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(100)),
            );
        }
    }
    fn endpoint(&self, suffix: &str) -> Result<String, String> {
        let mut url = url::Url::parse(&self.cfg.cpa_url).map_err(|_| "invalid CPA URL")?;
        url.set_path(&format!(
            "{}/v8/management/{suffix}",
            url.path().trim_end_matches('/')
        ));
        if suffix == "observability/usage/queue" {
            url.query_pairs_mut()
                .append_pair("count", &self.cfg.poll_count.to_string());
        }
        Ok(url.to_string())
    }
    fn get(&self, endpoint: &str) -> Result<ureq::Response, String> {
        self.agent
            .get(endpoint)
            .set(
                "Authorization",
                &format!("Bearer {}", self.cfg.management_key),
            )
            .set("Accept", "application/json")
            .call()
            .map_err(|error| match error {
                ureq::Error::Status(status, _) => format!("CPA returned HTTP {status}"),
                _ => "CPA HTTP transport failed".into(),
            })
    }
    pub fn refresh_names(&mut self) -> Result<(), String> {
        #[derive(Deserialize)]
        struct Key {
            #[serde(default)]
            auth_index: String,
        }
        #[derive(Deserialize)]
        struct Group {
            #[serde(default)]
            name: String,
            #[serde(default)]
            auth_index: String,
            #[serde(default)]
            keys: Vec<Key>,
        }
        let endpoint = self.endpoint("config/api-keys")?;
        let response = self
            .agent
            .get(&endpoint)
            .set(
                "Authorization",
                &format!("Bearer {}", self.cfg.management_key),
            )
            .set("Accept", "application/json")
            .call();
        let response = match response {
            Err(ureq::Error::Status(404, _)) => {
                self.names.clear();
                return Ok(());
            }
            Err(ureq::Error::Status(code, _)) => {
                return Err(format!("provider configuration returned HTTP {code}"));
            }
            Err(_) => return Err("provider configuration transport failed".into()),
            Ok(response) => response,
        };
        // Unknown fields (including API keys) are discarded during decoding.
        // Never log response bodies or serde errors that could include field values.
        use std::io::Read;
        let providers: HashMap<String, Vec<Group>> =
            serde_json::from_reader(response.into_reader().take(QUEUE_LIMIT))
                .map_err(|_| "invalid provider configuration JSON")?;
        let mut names = HashMap::new();
        let mut add = |index: String, name: &str| {
            let (index, name) = (index.trim(), name.trim());
            if index.is_empty() || name.is_empty() {
                return;
            }
            names
                .entry(index.to_owned())
                .and_modify(|old| {
                    if old != name {
                        *old = "unknown".into();
                    }
                })
                .or_insert_with(|| name.into());
        };
        for groups in providers.into_values() {
            for group in groups {
                add(group.auth_index, &group.name);
                for key in group.keys {
                    add(key.auth_index, &group.name);
                }
            }
        }
        self.names = names;
        Ok(())
    }
    fn refresh_credential_notes(&mut self) -> Result<(), String> {
        #[derive(Deserialize)]
        struct Credential {
            #[serde(default)]
            auth_index: String,
            #[serde(default)]
            note: Option<String>,
        }
        #[derive(Deserialize)]
        struct Credentials {
            files: Vec<Credential>,
        }
        let response = self.get(&self.endpoint("credentials")?)?;
        use std::io::Read;
        let credentials: Credentials =
            serde_json::from_reader(response.into_reader().take(QUEUE_LIMIT))
                .map_err(|_| "invalid credential metadata JSON")?;
        let mut notes = HashMap::new();
        for credential in credentials.files {
            let index = credential.auth_index.trim();
            let note = credential.note.as_deref().unwrap_or("").trim();
            if index.is_empty() || note.is_empty() {
                continue;
            }
            notes
                .entry(index.to_owned())
                .and_modify(|old| {
                    if old != note {
                        *old = "unknown".into();
                    }
                })
                .or_insert_with(|| note.to_owned());
        }
        self.credential_notes = notes;
        Ok(())
    }
    fn refresh_if_due(&mut self) {
        if self.cfg.label_name && Instant::now() >= self.next_refresh {
            if let Err(err) = self.refresh_names() {
                eprintln!("provider name lookup failed (keeping previous catalog): {err}");
            }
            if let Err(err) = self.refresh_credential_notes() {
                eprintln!("credential note lookup failed (keeping previous catalog): {err}");
            }
            self.next_refresh = Instant::now() + Duration::from_secs(60);
        }
    }
    fn name(&self, record: &Record) -> String {
        if let Some(note) = self.credential_notes.get(record.auth_index.trim()) {
            return note.clone();
        }
        if let Some(name) = self.names.get(record.auth_index.trim()) {
            return name.clone();
        }
        record
            .codex_oauth_email()
            .unwrap_or_else(|| "unknown".into())
    }
    pub fn process(&self, payload: &[u8]) -> bool {
        match serde_json::from_slice::<Record>(payload) {
            Ok(record) => {
                let name = self.name(&record);
                self.metrics.observe(record, &name);
                true
            }
            Err(_) => {
                self.metrics
                    .decode_errors
                    .with_label_values(&[&self.cfg.mode])
                    .inc();
                false
            }
        }
    }
    pub fn fetch(&self) -> Result<usize, String> {
        let endpoint = self.endpoint("observability/usage/queue")?;
        self.metrics
            .polls
            .with_label_values(&[&self.cfg.mode])
            .inc();
        let response = self.get(&endpoint)?;
        use std::io::Read;
        // Raw records let one malformed record fail independently of its batch.
        let items: Vec<Box<RawValue>> =
            serde_json::from_reader(response.into_reader().take(QUEUE_LIMIT))
                .map_err(|_| "invalid usage queue JSON")?;
        if !items.is_empty() {
            self.metrics
                .batches
                .with_label_values(&[&self.cfg.mode])
                .inc();
        }
        for item in &items {
            self.process(item.get().as_bytes());
        }
        Ok(items.len())
    }
    fn run_http(&mut self) {
        let mut next_poll = Instant::now();
        while !self.stopped() {
            self.refresh_if_due();
            if Instant::now() >= next_poll {
                let mut had_data = false;
                loop {
                    if self.stopped() {
                        return;
                    }
                    self.refresh_if_due();
                    match self.fetch() {
                        Ok(count) => {
                            self.metrics.up.with_label_values(&[&self.cfg.mode]).set(1);
                            had_data |= count > 0;
                            if self.cfg.verbose {
                                eprintln!("http usage queue fetched {count} record(s)");
                            }
                            if count < self.cfg.poll_count {
                                break;
                            }
                        }
                        Err(error) => {
                            eprintln!("http collector error: {error}");
                            self.metrics.error();
                            break;
                        }
                    }
                }
                next_poll = Instant::now()
                    + if had_data {
                        self.cfg.poll_interval
                    } else {
                        self.cfg.poll_idle_interval
                    };
            }
            let mut delay = next_poll.saturating_duration_since(Instant::now());
            if self.cfg.label_name {
                delay = delay.min(self.next_refresh.saturating_duration_since(Instant::now()));
            }
            self.wait(delay);
        }
    }
    fn redis_once(&mut self) -> Result<(), String> {
        let mut stream = None;
        for address in self
            .cfg
            .redis_addr
            .to_socket_addrs()
            .map_err(|_| "Redis address resolution failed")?
        {
            if let Ok(conn) = TcpStream::connect_timeout(&address, Duration::from_secs(10)) {
                stream = Some(conn);
                break;
            }
        }
        let mut stream = stream.ok_or("Redis connection failed")?;
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .map_err(|_| "Redis read timeout setup failed")?;
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .map_err(|_| "Redis write timeout setup failed")?;
        resp::write_command(&mut stream, &["AUTH", &self.cfg.management_key])
            .map_err(|_| "Redis AUTH write failed")?;
        stream.flush().map_err(|_| "Redis flush failed")?;
        let mut reader = BufReader::new(stream);
        let value =
            resp::read(&mut reader, &self.stop).map_err(|_| "Redis AUTH response failed")?;
        if value.bytes() != Some(b"OK") {
            return Err("Redis authentication failed".into());
        }
        resp::write_command(reader.get_mut(), &["SUBSCRIBE", "usage"])
            .map_err(|_| "Redis SUBSCRIBE write failed")?;
        let value =
            resp::read(&mut reader, &self.stop).map_err(|_| "Redis SUBSCRIBE response failed")?;
        if !resp::is_subscription(&value) {
            return Err("unexpected Redis subscription response".into());
        }
        self.metrics.up.with_label_values(&[&self.cfg.mode]).set(1);
        while !self.stopped() {
            self.refresh_if_due();
            match resp::read(&mut reader, &self.stop) {
                Ok(value) => {
                    if let Some(payload) = resp::usage_message(&value) {
                        self.metrics
                            .batches
                            .with_label_values(&[&self.cfg.mode])
                            .inc();
                        self.process(payload);
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue;
                }
                Err(_) => return Err("Redis stream read failed".into()),
            }
        }
        Ok(())
    }
}
pub fn run(cfg: Config, metrics: Arc<Metrics>, stop: Arc<AtomicBool>) {
    let mut collector = Collector::new(cfg, metrics, stop);
    if collector.cfg.mode == "http" {
        collector.run_http();
        return;
    }
    let mut backoff = Duration::from_secs(1);
    while !collector.stopped() {
        collector.refresh_if_due();
        if let Err(error) = collector.redis_once() {
            if collector.stopped() {
                break;
            }
            eprintln!("redis collector error: {error}");
            collector.metrics.error();
        }
        collector.wait(backoff);
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[test]
    fn metadata_refresh_failures_and_record_isolation() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_ip().unwrap();
        let response = Arc::new(Mutex::new((
            200u16,
            String::from(
                r#"{"codex":[{"name":"主上游","keys":[{"auth_index":"a","api-key":"sk-do-not-export"},{"auth_index":"b"}]}],"openai-compatibility":[{"name":"Keyless","auth_index":"c","keys":[]}]}"#,
            ),
        )));
        let worker_response = response.clone();
        let handle = std::thread::spawn(move || {
            for _ in 0..7 {
                let request = server.recv().unwrap();
                assert!(
                    request
                        .headers()
                        .iter()
                        .any(|h| h.field.equiv("Authorization")
                            && h.value.as_str() == "Bearer secret")
                );
                let (status, body) = if request
                    .url()
                    .starts_with("/base/v8/management/observability/usage/queue?count=7")
                {
                    (
                        200,
                        String::from(
                            r#"[{"auth_index":"a","source":"sk-do-not-export","tokens":{"total_tokens":1}},"malformed",{"auth_index":"c"}]"#,
                        ),
                    )
                } else {
                    assert_eq!(request.url(), "/base/v8/management/config/api-keys");
                    worker_response.lock().unwrap().clone()
                };
                request
                    .respond(tiny_http::Response::from_string(body).with_status_code(status))
                    .unwrap();
            }
        });
        let cfg = Config::parse(
            [
                format!("--cpa-url=http://{addr}/base"),
                "--poll-count=7".into(),
            ],
            |k| (k == "CPA_MANAGEMENT_KEY").then(|| "secret".into()),
        )
        .unwrap();
        let metrics = Arc::new(Metrics::new(&cfg).unwrap());
        let mut c = Collector::new(cfg, metrics.clone(), Arc::new(AtomicBool::new(false)));
        c.refresh_names().unwrap();
        assert_eq!(c.names.get("a").unwrap(), "主上游");
        assert_eq!(c.names.get("b").unwrap(), "主上游");
        assert_eq!(c.names.get("c").unwrap(), "Keyless");
        assert_eq!(c.fetch().unwrap(), 3);
        let rendered = String::from_utf8(metrics.render().unwrap()).unwrap();
        assert!(rendered.contains("name=\"主上游\""));
        assert!(!rendered.contains("sk-do-not-export"));
        assert_eq!(metrics.records.with_label_values(&["http"]).get(), 2);
        assert_eq!(metrics.decode_errors.with_label_values(&["http"]).get(), 1);
        *response.lock().unwrap() = (500, "sk-secret-response".into());
        assert!(c.refresh_names().is_err());
        assert_eq!(c.names.get("a").unwrap(), "主上游");
        *response.lock().unwrap() = (200, "not json".into());
        assert!(c.refresh_names().is_err());
        assert_eq!(c.names.get("a").unwrap(), "主上游");
        *response.lock().unwrap() = (
            200,
            r#"{"codex":[{"name":"Renamed","keys":[{"auth_index":"a"}]}]}"#.into(),
        );
        c.refresh_names().unwrap();
        assert_eq!(c.names.get("a").unwrap(), "Renamed");
        assert!(!c.names.contains_key("b"));
        *response.lock().unwrap()=(200,r#"{"codex":[{"name":"A","keys":[{"auth_index":"a"}]},{"name":"B","keys":[{"auth_index":"a"}]}]}"#.into());
        c.refresh_names().unwrap();
        assert_eq!(c.names.get("a").unwrap(), "unknown");
        *response.lock().unwrap() = (404, "not found".into());
        c.refresh_names().unwrap();
        assert!(c.names.is_empty());
        handle.join().unwrap();
    }
}

#[cfg(test)]
mod oauth_name_tests {
    use super::*;
    #[test]
    fn configured_name_precedes_email_and_all_metrics_use_fallback() {
        let cfg =
            Config::parse([], |k| (k == "CPA_MANAGEMENT_KEY").then(|| "secret".into())).unwrap();
        let metrics = Arc::new(Metrics::new(&cfg).unwrap());
        let mut c = Collector::new(cfg, metrics.clone(), Arc::new(AtomicBool::new(false)));
        let payload=br#"{"provider":"codex","auth_type":"oauth","source":"user@example.com","auth_index":"oauth-1","timestamp":"2026-11-25T00:00:00Z","latency_ms":100,"tokens":{"total_tokens":5}}"#;
        assert!(c.process(payload));
        let text = String::from_utf8(metrics.render().unwrap()).unwrap();
        for family in [
            "cpa_requests_total",
            "cpa_tokens_total",
            "cpa_request_latency_seconds_count",
            "cpa_usage_record_timestamp_seconds",
        ] {
            assert!(
                text.lines()
                    .any(|line| line.starts_with(&format!("{family}{{"))
                        && line.contains("name=\"***r@example.com\""))
            );
        }
        assert!(!text.contains("user@example.com"));
        assert!(!text.contains("source="));
        c.names.insert("oauth-1".into(), "Configured".into());
        let r: Record = serde_json::from_slice(payload).unwrap();
        assert_eq!(c.name(&r), "Configured");
        c.names.insert("oauth-1".into(), "unknown".into());
        assert_eq!(c.name(&r), "unknown");
    }
}

#[cfg(test)]
mod note_tests {
    use super::*;
    #[test]
    fn note_refresh_precedence_removal_and_outage() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_ip().unwrap();
        let worker = std::thread::spawn(move || {
            for (status, body) in [
                (
                    200,
                    r#"{"files":[{"auth_index":"oauth","note":"  工作账号  ","email":"never-expose@example.com"}]}"#,
                ),
                (503, "secret-error"),
                (200, "bad-json"),
                (
                    200,
                    r#"{"files":[{"auth_index":"oauth","note":"Renamed"}]}"#,
                ),
                (200, r#"{"files":[{"auth_index":"oauth","note":"  "}]}"#),
                (
                    200,
                    r#"{"files":[{"auth_index":"oauth","note":"A"},{"auth_index":"oauth","note":"B"}]}"#,
                ),
                (200, r#"{"files":[]}"#),
            ] {
                let r = server.recv().unwrap();
                assert_eq!(r.url(), "/base/v8/management/credentials");
                assert!(
                    r.headers()
                        .iter()
                        .any(|h| h.field.equiv("Authorization")
                            && h.value.as_str() == "Bearer secret")
                );
                r.respond(tiny_http::Response::from_string(body).with_status_code(status))
                    .unwrap();
            }
        });
        let cfg = Config::parse([format!("--cpa-url=http://{addr}/base")], |k| {
            (k == "CPA_MANAGEMENT_KEY").then(|| "secret".into())
        })
        .unwrap();
        let m = Arc::new(Metrics::new(&cfg).unwrap());
        let mut c = Collector::new(cfg, m.clone(), Arc::new(AtomicBool::new(false)));
        let record:Record=serde_json::from_str(r#"{"auth_index":"oauth","provider":"codex","auth_type":"oauth","source":"user@example.com"}"#).unwrap();
        c.names.insert("oauth".into(), "Group".into());
        c.refresh_credential_notes().unwrap();
        assert_eq!(c.name(&record), "工作账号");
        assert!(c.refresh_credential_notes().is_err());
        assert_eq!(c.name(&record), "工作账号");
        assert!(c.refresh_credential_notes().is_err());
        assert_eq!(c.name(&record), "工作账号");
        c.refresh_credential_notes().unwrap();
        assert_eq!(c.name(&record), "Renamed");
        c.refresh_credential_notes().unwrap();
        assert_eq!(c.name(&record), "Group");
        c.refresh_credential_notes().unwrap();
        assert_eq!(c.name(&record), "unknown");
        c.refresh_credential_notes().unwrap();
        c.names.clear();
        assert_eq!(c.name(&record), "***r@example.com");
        worker.join().unwrap();
    }
}
