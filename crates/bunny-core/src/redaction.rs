use regex::Regex;
use std::sync::LazyLock;

static AWS_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"AKIA[0-9A-Z]{16}").unwrap());
static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)Bearer\s+[A-Za-z0-9\-._~+/]+=*").unwrap());
static PEM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"-----BEGIN[A-Z ]+PRIVATE KEY-----").unwrap());
static JWT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+").unwrap());

const REDACTED: &str = "[REDACTED]";

pub struct Redactor {
    known_values: Vec<String>,
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

impl Redactor {
    pub fn new() -> Self {
        Self {
            known_values: Vec::new(),
        }
    }

    pub fn with_known_secrets(mut self, values: Vec<String>) -> Self {
        self.known_values = values;
        self
    }

    pub fn redact_text(&self, input: &str) -> String {
        let mut out = input.to_string();
        for v in &self.known_values {
            if !v.is_empty() {
                out = out.replace(v, REDACTED);
            }
        }
        out = AWS_KEY.replace_all(&out, REDACTED).into_owned();
        out = BEARER.replace_all(&out, REDACTED).into_owned();
        out = PEM.replace_all(&out, REDACTED).into_owned();
        out = JWT.replace_all(&out, REDACTED).into_owned();
        out
    }

    pub fn redact_url(&self, url: &str) -> String {
        if let Some((base, _)) = url.split_once('?') {
            format!("{base}?[query-redacted]")
        } else {
            url.to_string()
        }
    }

    pub fn redact_headers(&self, headers: &[(String, String)]) -> Vec<(String, String)> {
        headers
            .iter()
            .map(|(k, v)| {
                let kl = k.to_lowercase();
                if matches!(
                    kl.as_str(),
                    "authorization" | "cookie" | "set-cookie" | "x-api-key" | "x-auth-token"
                ) {
                    (k.clone(), REDACTED.to_string())
                } else {
                    (k.clone(), self.redact_text(v))
                }
            })
            .collect()
    }

    pub fn redact_json_value(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => serde_json::Value::String(self.redact_text(s)),
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| self.redact_json_value(v)).collect())
            }
            serde_json::Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (k, v) in map {
                    let redacted = if matches!(
                        k.to_lowercase().as_str(),
                        "password" | "secret" | "token" | "authorization" | "cookie"
                    ) {
                        serde_json::Value::String(REDACTED.to_string())
                    } else {
                        self.redact_json_value(v)
                    };
                    out.insert(k.clone(), redacted);
                }
                serde_json::Value::Object(out)
            }
            other => other.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_bearer() {
        let r = Redactor::new();
        let s = r.redact_text("Authorization: Bearer abc.def.ghi");
        assert!(s.contains(REDACTED));
    }
}
