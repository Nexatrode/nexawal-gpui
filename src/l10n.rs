use std::collections::HashMap;
use std::sync::OnceLock;

use gpui::SharedString;
use serde_json::Value;

static CATALOG: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();
// The desktop has no in-app language switch. Resolve OS/environment preferences
// once per launch rather than querying Foundation for every transaction row.
static LOCALES: OnceLock<Vec<String>> = OnceLock::new();

pub fn t(key: &str) -> SharedString {
    if let Some(translated) = translation_for(key) {
        return translated;
    }

    SharedString::from(key)
}

/// Substitute numbered UI arguments in one pass. An address, path or RPC error
/// containing another placeholder must never be substituted a second time.
pub fn interpolate(key: &str, arguments: &[String]) -> String {
    let template = t(key);
    let mut result = String::new();
    let mut rest: &str = &template;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        if let Some(end) = rest.find('}') {
            if let Ok(index) = rest[1..end].parse::<usize>() {
                if let Some(argument) = arguments.get(index) {
                    result.push_str(argument);
                    rest = &rest[end + 1..];
                    continue;
                }
            }
        }
        result.push('{');
        rest = &rest[1..];
    }
    result.push_str(rest);
    result
}

fn translation_for(key: &str) -> Option<SharedString> {
    let catalog = load_catalog();
    for locale in LOCALES.get_or_init(locale_candidates) {
        if let Some(strings) = catalog.get(locale) {
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
    let preferences = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .next();
    let raw = preferences.unwrap_or_else(system_languages);
    candidates_for(&raw)
}

#[cfg(target_os = "macos")]
fn system_languages() -> String {
    objc2_foundation::NSLocale::preferredLanguages()
        .iter()
        .map(|language| language.to_string())
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(not(target_os = "macos"))]
fn system_languages() -> String {
    "en".to_string()
}

fn candidates_for(raw: &str) -> Vec<String> {
    let mut values = Vec::new();
    for preference in raw.split(':') {
        let raw = preference.split(['.', '@']).next().unwrap_or("");
        let mut candidate = raw.trim().replace('_', "-").to_lowercase();
        if candidate == "zh" || candidate.starts_with("zh-cn") || candidate.starts_with("zh-sg") {
            candidate = "zh-hans".into();
        } else if candidate.starts_with("zh-tw")
            || candidate.starts_with("zh-hk")
            || candidate.starts_with("zh-mo")
        {
            candidate = "zh-hant".into();
        } else {
            for (alias, language) in [("tl", "fil"), ("iw", "he"), ("in", "id"), ("no", "nb")] {
                if candidate == alias || candidate.starts_with(&format!("{alias}-")) {
                    candidate.replace_range(..alias.len(), language);
                    break;
                }
            }
        }
        loop {
            if !candidate.is_empty() && !values.contains(&candidate) {
                values.push(candidate.clone());
            }
            if let Some(pos) = candidate.rfind('-') {
                candidate.truncate(pos);
            } else {
                break;
            }
        }
    }

    if !values.iter().any(|value| value == "en") {
        values.push("en".to_string());
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserted_arguments_are_not_interpreted_again() {
        assert_eq!(
            interpolate("{1}: {0}", &["{1}".into(), "Error".into()]),
            "Error: {1}"
        );
        assert_eq!(interpolate("{2}", &[]), "{2}");
    }

    #[test]
    fn regional_and_legacy_languages_resolve() {
        assert_eq!(candidates_for("zh_CN.UTF-8")[0], "zh-hans");
        assert_eq!(candidates_for("zh_TW")[0], "zh-hant");
        assert!(candidates_for("tl_PH").contains(&"fil".into()));
        assert!(candidates_for("iw_IL").contains(&"he".into()));
        assert!(candidates_for("no_NO").contains(&"nb".into()));
        assert_eq!(
            candidates_for("fr_CA:de_DE"),
            ["fr-ca", "fr", "de-de", "de", "en"]
        );
    }

    #[test]
    fn every_locale_has_nonblank_entries_for_english_keys() {
        let catalog = load_catalog();
        let english = &catalog["en"];
        for (locale, strings) in catalog {
            for key in english.keys() {
                assert!(
                    strings.get(key).is_some_and(|text| !text.trim().is_empty()),
                    "missing {locale}: {key}"
                );
            }
        }
    }
}
