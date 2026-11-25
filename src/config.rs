use std::{collections::HashMap, time::Duration};

#[derive(Clone)]
pub struct Config {
    pub listen_address: String,
    pub metrics_path: String,
    pub mode: String,
    pub cpa_url: String,
    pub management_key: String,
    pub poll_interval: Duration,
    pub poll_idle_interval: Duration,
    pub poll_count: usize,
    pub redis_addr: String,
    pub verbose: bool,
    pub label_provider_name: bool,
    pub label_source: bool,
    pub label_auth_index: bool,
    pub api_key_prefix_length: usize,
}

const OPTIONS: &[(&str, &str, &str)] = &[
    ("listen-address", "CPA_EXPORTER_LISTEN_ADDRESS", ":9321"),
    ("metrics-path", "CPA_EXPORTER_METRICS_PATH", "/metrics"),
    ("mode", "CPA_EXPORTER_MODE", "http"),
    ("cpa-url", "CPA_URL", "http://127.0.0.1:8317"),
    ("management-key", "CPA_MANAGEMENT_KEY", ""),
    ("poll-interval", "CPA_EXPORTER_POLL_INTERVAL", "5s"),
    (
        "poll-idle-interval",
        "CPA_EXPORTER_POLL_IDLE_INTERVAL",
        "30s",
    ),
    ("poll-count", "CPA_EXPORTER_POLL_COUNT", "100"),
    ("redis-addr", "CPA_REDIS_ADDR", "127.0.0.1:8317"),
    ("verbose", "CPA_EXPORTER_VERBOSE", "false"),
    (
        "label-provider-name",
        "CPA_EXPORTER_LABEL_PROVIDER_NAME",
        "true",
    ),
    ("label-source", "CPA_EXPORTER_LABEL_SOURCE", "false"),
    ("label-auth-index", "CPA_EXPORTER_LABEL_AUTH_INDEX", "false"),
    (
        "label-api-key-prefix-length",
        "CPA_EXPORTER_LABEL_API_KEY_PREFIX_LENGTH",
        "0",
    ),
];
const BOOLS: &[&str] = &[
    "verbose",
    "label-provider-name",
    "label-source",
    "label-auth-index",
];

fn boolean(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err("expected a boolean".into()),
    }
}

impl Config {
    pub fn help() -> String {
        let mut help = String::from(
            "CPA v8 Prometheus exporter\nUsage: cpa-exporter [options]\nSingle- and double-dash options are accepted.\n",
        );
        for (flag, env, default) in OPTIONS {
            help.push_str(&format!("  --{flag} (env: {env}, default: {default:?})\n"));
        }
        help
    }

    pub fn parse(
        args: impl IntoIterator<Item = String>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let mut values = HashMap::new();
        for &(flag, variable, default) in OPTIONS {
            let value = env(variable)
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| default.into());
            values.insert(flag, value.trim().to_string());
        }
        let mut args = args.into_iter().peekable();
        while let Some(arg) = args.next() {
            if !arg.starts_with('-') {
                return Err("unexpected positional argument".into());
            }
            let option = arg.trim_start_matches('-');
            let (name, value) = match option.split_once('=') {
                Some((name, value)) => (name, value.to_owned()),
                None if BOOLS.contains(&option) => (option, "true".into()),
                None => (
                    option,
                    args.next()
                        .ok_or_else(|| format!("missing value for --{option}"))?,
                ),
            };
            let Some(slot) = values.get_mut(name) else {
                return Err(format!("unknown option --{name}"));
            };
            *slot = value;
        }
        let get = |name: &str| values[name].clone();
        let flag =
            |name: &str| boolean(&get(name)).map_err(|_| format!("invalid boolean for --{name}"));
        let duration = |name: &str| {
            humantime::parse_duration(&get(name))
                .map_err(|_| format!("invalid duration for --{name}"))
        };
        let number = |name: &str| {
            get(name)
                .parse::<usize>()
                .map_err(|_| format!("invalid integer for --{name}"))
        };
        let path = get("metrics-path").trim().to_owned();
        let cfg = Self {
            listen_address: get("listen-address"),
            metrics_path: if path.is_empty() {
                "/metrics".into()
            } else if path.starts_with('/') {
                path
            } else {
                format!("/{path}")
            },
            mode: get("mode").trim().to_ascii_lowercase(),
            cpa_url: get("cpa-url"),
            management_key: get("management-key"),
            poll_interval: duration("poll-interval")?,
            poll_idle_interval: duration("poll-idle-interval")?,
            poll_count: number("poll-count")?,
            redis_addr: get("redis-addr"),
            verbose: flag("verbose")?,
            label_provider_name: flag("label-provider-name")?,
            label_source: flag("label-source")?,
            label_auth_index: flag("label-auth-index")?,
            api_key_prefix_length: number("label-api-key-prefix-length")?,
        };
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<(), String> {
        if self.management_key.is_empty() {
            return Err(
                "management key is required via --management-key or CPA_MANAGEMENT_KEY".into(),
            );
        }
        if self.api_key_prefix_length > 64 {
            return Err("--label-api-key-prefix-length must be <= 64".into());
        }
        if self.mode != "http" && self.mode != "redis" {
            return Err("--mode must be http or redis".into());
        }
        if self.mode == "http"
            && (self.poll_interval.is_zero()
                || self.poll_idle_interval.is_zero()
                || self.poll_count == 0)
        {
            return Err("poll intervals and count must be positive".into());
        }
        if self.mode == "redis" && self.redis_addr.trim().is_empty() {
            return Err("--redis-addr is required in redis mode".into());
        }
        if self.mode == "http" || self.label_provider_name {
            let u = url::Url::parse(&self.cpa_url).map_err(|_| "invalid --cpa-url")?;
            if !matches!(u.scheme(), "http" | "https")
                || u.host_str().is_none()
                || !u.username().is_empty()
                || u.password().is_some()
            {
                return Err("--cpa-url must be an HTTP(S) URL without embedded credentials".into());
            }
        }
        if self.metrics_path == "/-/healthy" {
            return Err("metrics path conflicts with health endpoint".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn go_style_flags_and_env() {
        let cfg = Config::parse(
            [
                "-label-source",
                "--label-auth-index=true",
                "-label-provider-name=false",
                "-label-api-key-prefix-length=6",
                "--poll-interval",
                "250ms",
            ]
            .map(str::to_owned),
            |key| (key == "CPA_MANAGEMENT_KEY").then(|| "secret".into()),
        )
        .unwrap();
        assert!(cfg.label_source && cfg.label_auth_index && !cfg.label_provider_name);
        assert_eq!(cfg.api_key_prefix_length, 6);
        assert_eq!(cfg.poll_interval, Duration::from_millis(250));
    }
    #[test]
    fn validation_and_defaults() {
        let parse = |args: Vec<&str>| {
            Config::parse(args.into_iter().map(str::to_owned), |key| {
                (key == "CPA_MANAGEMENT_KEY").then(|| "secret".into())
            })
        };
        assert!(parse(vec![]).unwrap().label_provider_name);
        for args in [
            vec!["--poll-count=0"],
            vec!["--poll-interval=0s"],
            vec!["--label-api-key-prefix-length=65"],
            vec!["--label-api-key-prefix-length=-1"],
            vec!["--mode=unknown"],
            vec!["--cpa-url=file:///tmp/a"],
            vec!["--metrics-path=/-/healthy"],
        ] {
            assert!(parse(args).is_err());
        }
        assert!(Config::parse([], |_| None).is_err());
    }
}
