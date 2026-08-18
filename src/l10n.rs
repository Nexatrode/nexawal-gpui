use std::collections::HashMap;
use std::sync::OnceLock;

use gpui::SharedString;
use serde_json::Value;

static CATALOG: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();

pub fn t(key: &str) -> SharedString {
    if let Some(translated) = translation_for(key) {
        return translated;
    }

    SharedString::from(key)
}

fn translation_for(key: &str) -> Option<SharedString> {
    let catalog = load_catalog();
    for locale in locale_candidates() {
        if let Some(strings) = catalog.get(&locale) {
            if let Some(translation) = strings.get(key) {
                if !translation.is_empty() {
                    return Some(SharedString::from(translation.clone()));
                }
            }
        }
    }
    None
}

fn load_catalog() -> &'static HashMap<String, HashMap<String, String>> {
    CATALOG.get_or_init(|| {
        let mut catalog = HashMap::new();
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value =
            serde_json::from_str(raw_json).unwrap_or(Value::Object(Default::default()));

        if let Some(locales) = data.as_object() {
            for (locale, entries) in locales {
                if let Some(entries) = entries.as_object() {
                    let mut bucket = HashMap::new();
                    for (key, value) in entries {
                        if let Some(text) = value.as_str() {
                            if !text.is_empty() {
                                bucket.insert(key.to_string(), text.to_string());
                            }
                        }
                    }
                    if !bucket.is_empty() {
                        catalog.insert(locale.to_lowercase(), bucket);
                    }
                }
            }
        }

        catalog
    })
}

fn locale_candidates() -> Vec<String> {
    let raw = std::env::var("LANGUAGE")
        .ok()
        .or_else(|| std::env::var("LC_ALL").ok())
        .or_else(|| std::env::var("LC_MESSAGES").ok())
        .or_else(|| std::env::var("LANG").ok())
        .unwrap_or_else(|| "en".to_string());
    let first_locale = raw
        .split(':')
        .next()
        .unwrap_or("en")
        .split('.')
        .next()
        .unwrap_or("en")
        .split('@')
        .next()
        .unwrap_or("en")
        .trim()
        .replace('_', "-")
        .replace('.', "")
        .trim()
        .to_string();

    let mut values = Vec::new();
    let mut candidate = if first_locale.is_empty() {
        "en".to_string()
    } else {
        first_locale
    };

    loop {
        if !candidate.is_empty()
            && !values
                .iter()
                .any(|value| value == &candidate.to_lowercase())
        {
            values.push(candidate.to_lowercase());
        }

        if let Some(pos) = candidate.rfind('-') {
            candidate.truncate(pos);
        } else {
            break;
        }
    }

    if !values.iter().any(|value| value == "en") {
        values.push("en".to_string());
    }
    values
}
