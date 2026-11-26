use crate::{
    config::Config,
    record::{Record, label, masked_source},
};
use prometheus::{
    Encoder, GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, IntGaugeVec, Opts, Registry,
    TextEncoder,
};

pub struct Metrics {
    pub registry: Registry,
    cfg: Config,
    requests: IntCounterVec,
    tokens: IntCounterVec,
    latency: HistogramVec,
    timestamp: GaugeVec,
    pub records: IntCounterVec,
    pub decode_errors: IntCounterVec,
    pub collector_errors: IntCounterVec,
    pub polls: IntCounterVec,
    pub batches: IntCounterVec,
    pub up: IntGaugeVec,
}
impl Metrics {
    pub fn new(cfg: &Config) -> prometheus::Result<Self> {
        let registry = Registry::new();
        let mut base = vec!["provider", "model", "alias", "endpoint", "auth_type"];
        if cfg.label_name {
            base.push("name");
        }
        if cfg.label_source {
            base.push("source");
        }
        if cfg.label_auth_index {
            base.push("auth_index");
        }
        if cfg.api_key_prefix_length > 0 {
            base.push("api_key_prefix");
        }
        let labels = |extra: &[&'static str]| {
            let mut v = base.clone();
            v.extend_from_slice(extra);
            v
        };
        let requests = IntCounterVec::new(
            Opts::new(
                "cpa_requests_total",
                "Total CLIProxyAPI usage records grouped as requests.",
            ),
            &labels(&["failed", "status_code"]),
        )?;
        let tokens = IntCounterVec::new(
            Opts::new(
                "cpa_tokens_total",
                "Total tokens reported by CLIProxyAPI usage records.",
            ),
            &labels(&["token_type"]),
        )?;
        let latency = HistogramVec::new(
            HistogramOpts::new(
                "cpa_request_latency_seconds",
                "CLIProxyAPI request latency in seconds.",
            )
            .buckets(vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ]),
            &labels(&["failed"]),
        )?;
        let timestamp = GaugeVec::new(
            Opts::new(
                "cpa_usage_record_timestamp_seconds",
                "Unix timestamp of the latest observed CLIProxyAPI usage record.",
            ),
            &base,
        )?;
        let counter = |name, help| IntCounterVec::new(Opts::new(name, help), &["mode"]);
        let m = Self {
            registry,
            cfg: cfg.clone(),
            requests,
            tokens,
            latency,
            timestamp,
            records: counter(
                "cpa_exporter_records_total",
                "Total usage records successfully decoded by the exporter.",
            )?,
            decode_errors: counter(
                "cpa_exporter_decode_errors_total",
                "Total usage records the exporter failed to decode.",
            )?,
            collector_errors: counter(
                "cpa_exporter_collector_errors_total",
                "Total collector errors while talking to CLIProxyAPI.",
            )?,
            polls: counter(
                "cpa_exporter_polls_total",
                "Total HTTP poll requests made to CLIProxyAPI.",
            )?,
            batches: counter(
                "cpa_exporter_batches_total",
                "Total non-empty usage batches received from CLIProxyAPI.",
            )?,
            up: IntGaugeVec::new(
                Opts::new(
                    "cpa_exporter_up",
                    "Whether the collector is currently healthy.",
                ),
                &["mode"],
            )?,
        };
        for collector in [
            Box::new(m.requests.clone()) as Box<dyn prometheus::core::Collector>,
            Box::new(m.tokens.clone()),
            Box::new(m.latency.clone()),
            Box::new(m.timestamp.clone()),
            Box::new(m.records.clone()),
            Box::new(m.decode_errors.clone()),
            Box::new(m.collector_errors.clone()),
            Box::new(m.polls.clone()),
            Box::new(m.batches.clone()),
            Box::new(m.up.clone()),
        ] {
            m.registry.register(collector)?;
        }
        m.up.with_label_values(&[&cfg.mode]).set(0);
        Ok(m)
    }
    pub fn render(&self) -> prometheus::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        TextEncoder::new().encode(&self.registry.gather(), &mut bytes)?;
        Ok(bytes)
    }
    pub fn observe(&self, mut r: Record, name: &str) {
        r.normalize();
        let alias = if r.alias.trim().is_empty() {
            label(&r.model)
        } else {
            r.alias.trim()
        };
        let mut values: Vec<String> = [
            label(&r.provider),
            label(&r.model),
            alias,
            label(&r.endpoint),
            label(&r.auth_type),
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        if self.cfg.label_name {
            values.push(label(name).into());
        }
        if self.cfg.label_source {
            values.push(masked_source(&r.source));
        }
        if self.cfg.label_auth_index {
            values.push(label(&r.auth_index).into());
        }
        if self.cfg.api_key_prefix_length > 0 {
            values.push(if r.api_key.trim().is_empty() {
                "unknown".into()
            } else {
                r.api_key
                    .trim()
                    .chars()
                    .take(self.cfg.api_key_prefix_length)
                    .collect()
            });
        }
        let base: Vec<&str> = values.iter().map(String::as_str).collect();
        let failed = if r.failed { "true" } else { "false" };
        let status = r.fail.status_code.to_string();
        let mut labels = base.clone();
        labels.extend([failed, &status]);
        self.requests.with_label_values(&labels).inc();
        let mut labels = base.clone();
        labels.push(failed);
        if r.latency_ms >= 0 {
            self.latency
                .with_label_values(&labels)
                .observe(r.latency_ms as f64 / 1000.0);
        }
        let t = &r.tokens;
        for (name, count) in [
            ("input", t.input_tokens),
            ("output", t.output_tokens),
            ("reasoning", t.reasoning_tokens),
            ("cached", t.cached_tokens),
            ("cache_read", t.cache_read_tokens),
            ("cache_creation", t.cache_creation_tokens),
            ("total", t.total_tokens),
        ] {
            if count > 0 {
                let mut labels = base.clone();
                labels.push(name);
                self.tokens.with_label_values(&labels).inc_by(count as u64);
            }
        }
        if let Some(timestamp) = r.timestamp {
            self.timestamp
                .with_label_values(&base)
                .set(timestamp.timestamp() as f64);
        }
        self.records.with_label_values(&[&self.cfg.mode]).inc();
    }
    pub fn error(&self) {
        self.collector_errors
            .with_label_values(&[&self.cfg.mode])
            .inc();
        self.up.with_label_values(&[&self.cfg.mode]).set(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_and_buckets_match_go() {
        let cfg = Config::parse(
            ["--label-source", "--label-auth-index"].map(str::to_owned),
            |k| (k == "CPA_MANAGEMENT_KEY").then(|| "secret".into()),
        )
        .unwrap();
        let m = Metrics::new(&cfg).unwrap();
        let r=serde_json::from_str(r#"{"provider":"codex","model":"m","source":"user@example.com","api_key":"sk-secret","auth_index":"a","latency_ms":123,"timestamp":"2026-11-24T00:00:00Z","tokens":{"input_tokens":7,"output_tokens":5,"total_tokens":12}}"#).unwrap();
        m.observe(r, "主上游");
        let text = String::from_utf8(m.render().unwrap()).unwrap();
        for expected in [
            "name=\"主上游\"",
            "source=\"***r@example.com\"",
            "status_code=\"200\"",
            "le=\"0.005\"",
            "le=\"+Inf\"",
            "cpa_request_latency_seconds_sum",
            "cpa_usage_record_timestamp_seconds",
            "token_type=\"total\"",
        ] {
            assert!(text.contains(expected), "missing {expected}");
        }
        assert!(!text.contains("sk-secret"));
        assert!(!text.contains("user@example.com"));
    }
}
