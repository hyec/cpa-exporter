use chrono::{DateTime, FixedOffset};
use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Record {
    pub timestamp: Option<DateTime<FixedOffset>>,
    pub latency_ms: i64,
    pub source: String,
    pub auth_index: String,
    pub tokens: Tokens,
    pub token_breakdown: Option<Breakdown>,
    pub failed: bool,
    pub fail: Failure,
    pub provider: String,
    pub model: String,
    pub alias: String,
    pub endpoint: String,
    pub auth_type: String,
    pub api_key: String,
}
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Failure {
    pub status_code: i32,
}
#[derive(Default, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct Tokens {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_tokens: i64,
}
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Breakdown {
    pub total_tokens: i64,
    pub input: Input,
    pub output: Output,
}
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Input {
    pub total_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Output {
    pub total_tokens: i64,
    pub reasoning_tokens: i64,
}

pub fn label(value: &str) -> &str {
    let v = value.trim();
    if v.is_empty() { "unknown" } else { v }
}

impl Record {
    pub fn normalize(&mut self) {
        if let Some(b) = &self.token_breakdown {
            self.tokens = Tokens {
                input_tokens: b.input.total_tokens,
                output_tokens: b.output.total_tokens,
                reasoning_tokens: b.output.reasoning_tokens,
                cached_tokens: b.input.cache_read_tokens,
                cache_read_tokens: b.input.cache_read_tokens,
                cache_creation_tokens: b.input.cache_write_tokens,
                total_tokens: b.total_tokens,
            };
        } else if self.tokens.total_tokens == 0 {
            self.tokens.total_tokens = self
                .tokens
                .input_tokens
                .saturating_add(self.tokens.output_tokens)
                .saturating_add(self.tokens.reasoning_tokens);
        }
        if self.fail.status_code <= 0 {
            self.fail.status_code = if self.failed { 500 } else { 200 };
        }
    }
}

pub fn masked_source(value: &str) -> String {
    let value = label(value);
    if value == "unknown" {
        return value.into();
    }
    if let Some((local, domain)) = value.rsplit_once('@') {
        let domain = domain.trim();
        if !local.is_empty() && !domain.is_empty() && !local.contains('@') {
            return format!("{}@{domain}", masked_key(local));
        }
    }
    masked_key(value)
}
fn masked_key(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        let visible = chars.len() / 3;
        format!(
            "{}{}",
            "*".repeat(chars.len() - visible),
            chars[chars.len() - visible..].iter().collect::<String>()
        )
    } else {
        format!(
            "{}***{}",
            chars[..4].iter().collect::<String>(),
            chars[chars.len() - 4..].iter().collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_accounting_and_legacy() {
        let mut r: Record = serde_json::from_str(r#"{"tokens":{"total_tokens":999},"token_breakdown":{"total_tokens":12,"input":{"total_tokens":7,"cache_read_tokens":2,"cache_write_tokens":1},"output":{"total_tokens":5,"reasoning_tokens":3}}}"#).unwrap();
        r.normalize();
        r.normalize();
        assert_eq!(
            r.tokens,
            Tokens {
                input_tokens: 7,
                output_tokens: 5,
                reasoning_tokens: 3,
                cached_tokens: 2,
                cache_read_tokens: 2,
                cache_creation_tokens: 1,
                total_tokens: 12
            }
        );
        assert_eq!(r.fail.status_code, 200);
        let mut r: Record = serde_json::from_str(
            r#"{"failed":true,"tokens":{"input_tokens":3,"output_tokens":4}}"#,
        )
        .unwrap();
        r.normalize();
        assert_eq!(r.tokens.total_tokens, 7);
        assert_eq!(r.fail.status_code, 500);
        let mut r: Record =
            serde_json::from_str(r#"{"tokens":{"input_tokens":99},"token_breakdown":{}}"#).unwrap();
        r.normalize();
        assert_eq!(r.tokens, Tokens::default());
    }
    #[test]
    fn masking_is_unicode_safe() {
        for (input, want) in [
            ("user@example.com", "***r@example.com"),
            ("abcdefghijkl@example.com", "abcd***ijkl@example.com"),
            ("sk-abcdefghijkl", "sk-a***ijkl"),
            ("abc", "**c"),
            ("a", "*"),
            ("", "unknown"),
            ("上游秘密凭证测试九十", "上游秘密***测试九十"),
        ] {
            assert_eq!(masked_source(input), want);
        }
    }
}
