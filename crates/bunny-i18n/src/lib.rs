//! Shared UI translations (see `packages/i18n/messages.json`).

use std::collections::HashMap;
use std::sync::OnceLock;

const MESSAGES_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/i18n/messages.json"
));

static CATALOG: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();

fn catalog() -> &'static HashMap<String, HashMap<String, String>> {
    CATALOG.get_or_init(|| {
        serde_json::from_str(MESSAGES_JSON).expect("packages/i18n/messages.json must be valid JSON")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    En,
    Fr,
}

impl Locale {
    pub fn as_str(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Fr => "fr",
        }
    }

    pub fn from_db(s: &str) -> Self {
        parse_locale(s).unwrap_or(Locale::En)
    }
}

pub fn parse_locale(s: &str) -> Option<Locale> {
    match s.trim().to_lowercase().as_str() {
        "en" | "english" => Some(Locale::En),
        "fr" | "french" | "français" | "francais" => Some(Locale::Fr),
        _ => None,
    }
}

pub fn is_valid_locale_code(s: &str) -> bool {
    parse_locale(s).is_some()
}

/// Translate `key` for `locale`, substituting `{name}` placeholders from `args`.
pub fn t(locale: Locale, key: &str, args: &[(&str, &str)]) -> String {
    let entry = catalog()
        .get(key)
        .unwrap_or_else(|| panic!("missing i18n key: {key}"));
    let template = entry
        .get(locale.as_str())
        .or_else(|| entry.get("en"))
        .unwrap_or_else(|| panic!("missing locale {} for key {key}", locale.as_str()));
    let mut out = template.clone();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// All message keys in the catalog (for tests).
pub fn all_keys() -> Vec<String> {
    let mut keys: Vec<_> = catalog().keys().cloned().collect();
    keys.sort();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_fr_parity() {
        for key in all_keys() {
            let entry = catalog().get(&key).unwrap();
            assert!(entry.contains_key("en"), "key {key} missing en");
            assert!(entry.contains_key("fr"), "key {key} missing fr");
        }
    }

    #[test]
    fn substitution() {
        let s = t(
            Locale::En,
            "configure.config_created",
            &[("path", "/tmp/config.yaml")],
        );
        assert!(s.contains("/tmp/config.yaml"));
    }
}
