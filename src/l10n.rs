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

    #[test]
    fn dutch_and_turkish_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let nl = &data["nl"];
        let tr = &data["tr"];

        assert!(
            nl[recovery_key]
                .as_str()
                .unwrap()
                .contains("herstelzin van 25 woorden")
        );
        assert!(
            tr[recovery_key]
                .as_str()
                .unwrap()
                .contains("25 kelimelik kurtarma ifadenizden")
        );
        assert!(
            nl["Wallet debit (amount + fee)"]
                .as_str()
                .unwrap()
                .contains("netwerkkosten")
        );
        assert!(
            tr["Wallet debit (amount + fee)"]
                .as_str()
                .unwrap()
                .contains("ağ ücreti")
        );
        assert!(
            nl[privacy_key]
                .as_str()
                .unwrap()
                .contains("haalt de app koersen op bij rates.nexawal.com")
        );
        assert!(
            tr[privacy_key]
                .as_str()
                .unwrap()
                .contains("kurları rates.nexawal.com adresinden alır")
        );
        assert!(
            !nl[privacy_key]
                .as_str()
                .unwrap()
                .contains("neem contact op met rates.nexawal.com")
        );
        assert!(
            !tr[privacy_key]
                .as_str()
                .unwrap()
                .contains("ile iletişime geçin")
        );
    }

    #[test]
    fn azerbaijani_and_bulgarian_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";

        assert!(
            data["az"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25 sözdən ibarət bərpa ifadəniz")
        );
        assert!(
            data["bg"][recovery_key]
                .as_str()
                .unwrap()
                .contains("фразата си за възстановяване от 25 думи")
        );
        assert!(
            data["az"]["Wallet debit (amount + fee)"]
                .as_str()
                .unwrap()
                .contains("Pul kisəsindən çıxılan məbləğ")
        );
        assert!(
            data["bg"]["Wallet debit (amount + fee)"]
                .as_str()
                .unwrap()
                .contains("Общо дебитирано от портфейла")
        );
        assert!(
            data["az"][privacy_key]
                .as_str()
                .unwrap()
                .contains("tətbiq məzənnələri rates.nexawal.com saytından alır")
        );
        assert!(
            data["bg"][privacy_key]
                .as_str()
                .unwrap()
                .contains("приложението получава курсовете от rates.nexawal.com")
        );
    }

    #[test]
    fn bengali_and_catalan_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let rates_key = "Optional. When enabled, NexaWal requests an XMR rate for your selected currency from rates.nexawal.com over HTTPS. This request uses clearnet even when the wallet node uses I2P. Nexatrode can see your IP address and selected currency; wallet amounts, addresses, seeds, and transaction history are not sent. Estimates only — XMR is what you send and hold.";

        assert!(
            data["bn"][recovery_key]
                .as_str()
                .unwrap()
                .contains("২৫ শব্দের পুনরুদ্ধার বাক্যাংশ")
        );
        assert_eq!(
            data["bn"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "ওয়ালেট থেকে মোট কর্তন (পরিমাণ + ফি)"
        );
        assert!(
            data["bn"][privacy_key]
                .as_str()
                .unwrap()
                .contains("অ্যাপ rates.nexawal.com থেকে বিনিময় হার সংগ্রহ করে")
        );
        assert!(
            !data["bn"][privacy_key]
                .as_str()
                .unwrap()
                .contains("যোগাযোগ করুন")
        );
        assert!(
            data["bn"][rates_key]
                .as_str()
                .unwrap()
                .contains("পুনরুদ্ধার বাক্যাংশ")
        );
        assert!(data["bn"]["Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time."].as_str().unwrap().contains("ফি-র পরে"));

        assert!(
            data["ca"][recovery_key]
                .as_str()
                .unwrap()
                .contains("frase de recuperació de 25 paraules")
        );
        assert_eq!(
            data["ca"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Total descomptat de la cartera (import + comissió)"
        );
        assert!(
            data["ca"][privacy_key]
                .as_str()
                .unwrap()
                .contains("l’aplicació obté els tipus de canvi de rates.nexawal.com")
        );
        assert!(
            !data["ca"][privacy_key]
                .as_str()
                .unwrap()
                .contains("poseu-vos en contacte")
        );
        assert!(
            data["ca"][rates_key]
                .as_str()
                .unwrap()
                .contains("Internet pública (Clearnet)")
        );
        assert!(data["ca"]["Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time."].as_str().unwrap().contains("després de la tarifa"));
    }

    #[test]
    fn czech_and_danish_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";

        assert!(
            data["cs"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25slovnou frázi pro obnovení")
        );
        assert_eq!(
            data["cs"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Celkem odečteno z peněženky (částka + poplatek)"
        );
        assert!(
            data["cs"][privacy_key]
                .as_str()
                .unwrap()
                .contains("aplikace načítá kurzy z rates.nexawal.com")
        );
        assert!(
            !data["cs"][privacy_key]
                .as_str()
                .unwrap()
                .contains("kontaktujte rates.nexawal.com")
        );
        assert!(
            data["cs"][send_max_key]
                .as_str()
                .unwrap()
                .contains("po odečtení poplatku")
        );

        assert!(
            data["da"][recovery_key]
                .as_str()
                .unwrap()
                .contains("gendannelsesfrase på 25 ord")
        );
        assert_eq!(
            data["da"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Samlet trukket fra tegnebogen (beløb + gebyr)"
        );
        assert!(
            data["da"][privacy_key]
                .as_str()
                .unwrap()
                .contains("henter appen kurser fra rates.nexawal.com")
        );
        assert!(
            !data["da"][privacy_key]
                .as_str()
                .unwrap()
                .contains("kontakt rates.nexawal.com")
        );
        assert!(
            data["da"][send_max_key]
                .as_str()
                .unwrap()
                .contains("efter fradrag af gebyret")
        );
    }

    #[test]
    fn greek_and_esperanto_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";

        assert!(
            data["el"][recovery_key]
                .as_str()
                .unwrap()
                .contains("φράση ανάκτησης 25 λέξεων")
        );
        assert_eq!(
            data["el"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Συνολικό ποσό που αφαιρείται από το πορτοφόλι (ποσό + χρέωση)"
        );
        assert!(
            data["el"][privacy_key]
                .as_str()
                .unwrap()
                .contains("η εφαρμογή λαμβάνει ισοτιμίες από rates.nexawal.com")
        );
        assert!(
            !data["el"][privacy_key]
                .as_str()
                .unwrap()
                .contains("επικοινωνήστε με το rates.nexawal.com")
        );
        assert!(
            data["el"][send_max_key]
                .as_str()
                .unwrap()
                .contains("μετά την αφαίρεση της χρέωσης")
        );

        assert!(
            data["eo"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25-vorta reakira frazo")
        );
        assert_eq!(
            data["eo"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Sumo entute deprenita de la monujo (sumo + kotizo)"
        );
        assert!(
            data["eo"][privacy_key]
                .as_str()
                .unwrap()
                .contains("la aplikaĵo ricevas kurzojn de rates.nexawal.com")
        );
        assert!(
            !data["eo"][privacy_key]
                .as_str()
                .unwrap()
                .contains("kontaktu rates.nexawal.com")
        );
        assert!(
            data["eo"][send_max_key]
                .as_str()
                .unwrap()
                .contains("post depreno de la kotizo")
        );
    }

    #[test]
    fn estonian_and_basque_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";

        assert!(
            data["et"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25-sõnalise taastefraasi")
        );
        assert_eq!(
            data["et"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Rahakotist maha arvestatud kogusumma (summa + tasu)"
        );
        assert!(
            data["et"][privacy_key]
                .as_str()
                .unwrap()
                .contains("hangib rakendus kursid aadressilt rates.nexawal.com")
        );
        assert!(
            !data["et"][privacy_key]
                .as_str()
                .unwrap()
                .contains("võtke hindade saamiseks ühendust")
        );
        assert!(
            data["et"][send_max_key]
                .as_str()
                .unwrap()
                .contains("pärast tasu mahaarvamist")
        );

        assert!(
            data["eu"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25 hitzeko berreskuratze-esaldi")
        );
        assert_eq!(
            data["eu"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Zorroari guztira kendutako zenbatekoa (zenbatekoa + komisioa)"
        );
        assert!(
            data["eu"][privacy_key]
                .as_str()
                .unwrap()
                .contains("aplikazioak rates.nexawal.com-etik eskuratzen ditu truke-tasak")
        );
        assert!(
            !data["eu"][privacy_key]
                .as_str()
                .unwrap()
                .contains("jarri harremanetan rates.nexawal.com")
        );
        assert!(
            data["eu"][send_max_key]
                .as_str()
                .unwrap()
                .contains("komisioa kendu ondoren")
        );
    }

    #[test]
    fn persian_and_finnish_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";

        assert!(
            data["fi"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25 sanan palautuslausekkeestasi")
        );
        assert_eq!(
            data["fi"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Lompakosta veloitettava määrä (määrä + verkkomaksu)"
        );
        assert_eq!(data["fi"]["Fee"].as_str().unwrap(), "Verkkomaksu");
        assert!(
            data["fi"][privacy_key]
                .as_str()
                .unwrap()
                .contains("sovellus noutaa valuuttakurssit palvelusta rates.nexawal.com")
        );
        assert!(
            !data["fi"][privacy_key]
                .as_str()
                .unwrap()
                .contains("ota yhteyttä rates.nexawal.com")
        );
        assert!(
            data["fi"][send_max_key]
                .as_str()
                .unwrap()
                .contains("verkkomaksun jälkeen käytettävissä oleva enimmäismäärä")
        );
        assert_eq!(
            data["fi"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );

        assert!(
            data["fa"][recovery_key]
                .as_str()
                .unwrap()
                .contains("عبارت بازیابی ۲۵ کلمه‌ای")
        );
        assert!(
            data["fa"]["Wallet debit (amount + fee)"]
                .as_str()
                .unwrap()
                .contains("کارمزد")
        );
        assert_eq!(data["fa"]["Fee"].as_str().unwrap(), "کارمزد شبکه");
        assert!(
            data["fa"][privacy_key]
                .as_str()
                .unwrap()
                .contains("برنامه نرخ‌ها را از rates.nexawal.com دریافت می‌کند")
        );
        assert!(
            !data["fa"][privacy_key]
                .as_str()
                .unwrap()
                .contains("تماس بگیرید")
        );
        assert!(
            data["fa"][send_max_key]
                .as_str()
                .unwrap()
                .contains("پس از کسر کارمزد شبکه")
        );
        assert_eq!(
            data["fa"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );
    }

    #[test]
    fn filipino_and_irish_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let rates_key = "Optional. When enabled, NexaWal requests an XMR rate for your selected currency from rates.nexawal.com over HTTPS. This request uses clearnet even when the wallet node uses I2P. Nexatrode can see your IP address and selected currency; wallet amounts, addresses, seeds, and transaction history are not sent. Estimates only — XMR is what you send and hold.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";

        assert!(
            data["fil"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25-salitang parirala sa pagbawi")
        );
        assert!(
            !data["fil"][recovery_key]
                .as_str()
                .unwrap()
                .contains("seed phrase")
        );
        assert_eq!(data["fil"]["Fee"].as_str().unwrap(), "Bayad sa network");
        assert_eq!(
            data["fil"]["Send Max"].as_str().unwrap(),
            "Ipadala ang pinakamalaking halaga"
        );
        assert!(
            data["fil"][privacy_key]
                .as_str()
                .unwrap()
                .contains("kinukuha ng app ang mga exchange rate mula sa rates.nexawal.com")
        );
        assert!(
            !data["fil"][privacy_key]
                .as_str()
                .unwrap()
                .contains("makipag-ugnayan sa rates.nexawal.com")
        );
        assert!(
            data["fil"][rates_key]
                .as_str()
                .unwrap()
                .contains("parirala sa pagbawi")
        );
        assert!(
            data["fil"][send_max_key]
                .as_str()
                .unwrap()
                .contains("matapos ibawas ang bayad sa network")
        );
        assert_eq!(
            data["fil"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );

        assert!(
            data["ga"][recovery_key]
                .as_str()
                .unwrap()
                .contains("athshlánaithe 25 focal")
        );
        assert!(
            !data["ga"][recovery_key]
                .as_str()
                .unwrap()
                .contains("frása síl")
        );
        assert_eq!(data["ga"]["Fee"].as_str().unwrap(), "Táille líonra");
        assert_eq!(
            data["ga"]["Send Max"].as_str().unwrap(),
            "Seol an t-uasmhéid"
        );
        assert!(
            data["ga"][privacy_key]
                .as_str()
                .unwrap()
                .contains("faigheann an aip rátaí malairte ó rates.nexawal.com")
        );
        assert!(
            !data["ga"][privacy_key]
                .as_str()
                .unwrap()
                .contains("déan teagmháil le rates.nexawal.com")
        );
        assert!(
            data["ga"][rates_key]
                .as_str()
                .unwrap()
                .contains("frása athshlánaithe")
        );
        assert!(
            data["ga"][send_max_key]
                .as_str()
                .unwrap()
                .contains("tar éis an táille líonra a asbhaint")
        );
        assert_eq!(
            data["ga"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );
    }

    #[test]
    fn galician_and_guarani_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let rates_key = "Optional. When enabled, NexaWal requests an XMR rate for your selected currency from rates.nexawal.com over HTTPS. This request uses clearnet even when the wallet node uses I2P. Nexatrode can see your IP address and selected currency; wallet amounts, addresses, seeds, and transaction history are not sent. Estimates only — XMR is what you send and hold.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";

        assert!(
            data["gl"][recovery_key]
                .as_str()
                .unwrap()
                .contains("frase de recuperación de 25 palabras")
        );
        assert_eq!(
            data["gl"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Total debitado da carteira (cantidade + comisión)"
        );
        assert_eq!(data["gl"]["Fee"].as_str().unwrap(), "Comisión");
        assert!(
            data["gl"][privacy_key]
                .as_str()
                .unwrap()
                .contains("enderezo IP e as consultas de sincronización da carteira")
        );
        assert!(
            data["gl"][privacy_key]
                .as_str()
                .unwrap()
                .contains("a aplicación obtén as taxas de rates.nexawal.com")
        );
        assert!(
            !data["gl"][privacy_key]
                .as_str()
                .unwrap()
                .contains("póñase en contacto con rates.nexawal.com")
        );
        assert!(
            data["gl"][rates_key]
                .as_str()
                .unwrap()
                .contains("frases de recuperación")
        );
        assert!(
            data["gl"][send_max_key]
                .as_str()
                .unwrap()
                .contains("despois de descontar a comisión da rede")
        );
        assert_eq!(
            data["gl"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );

        assert!(
            data["gn"][recovery_key]
                .as_str()
                .unwrap()
                .contains("frase de recuperación 25 ñe'ẽ")
        );
        assert_eq!(
            data["gn"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Viru oñeguenohẽva ñongatuha-gui (cantidad + comisión)"
        );
        assert!(
            data["gn"][privacy_key]
                .as_str()
                .unwrap()
                .contains("ne ñongatuha sincronización rehegua jerure")
        );
        assert!(
            data["gn"][privacy_key]
                .as_str()
                .unwrap()
                .contains("aplicación ogueru tasa de cambio rates.nexawal.com-gui")
        );
        assert!(
            !data["gn"][privacy_key]
                .as_str()
                .unwrap()
                .contains("eñe’ẽ rates.nexawal.com ndive")
        );
        assert!(
            data["gn"][rates_key]
                .as_str()
                .unwrap()
                .contains("frase de recuperación")
        );
        assert!(
            data["gn"][send_max_key]
                .as_str()
                .unwrap()
                .contains("ojeipe'a rire comisión de red")
        );
        assert_eq!(
            data["gn"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );
    }

    #[test]
    fn hausa_and_hebrew_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let send_max_mode_key = "Send Max mode: Confirm will sweep all unlocked after fee.";

        assert!(
            data["ha"][recovery_key]
                .as_str()
                .unwrap()
                .contains("jumlar dawowa mai kalmomi 25")
        );
        assert_eq!(
            data["ha"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Jimillar kuɗin da aka cire daga walat (adadi + kuɗin hanyar sadarwa)"
        );
        assert!(
            data["ha"][privacy_key]
                .as_str()
                .unwrap()
                .contains("adireshin IP ɗinka da buƙatun wallet ɗinka na daidaitawa")
        );
        assert!(
            data["ha"][privacy_key]
                .as_str()
                .unwrap()
                .contains("manhajar tana neman ƙimar kuɗi daga rates.nexawal.com")
        );
        assert!(
            !data["ha"][privacy_key]
                .as_str()
                .unwrap()
                .contains("tuntuɓi rates.nexawal.com")
        );
        assert!(
            data["ha"][send_max_key]
                .as_str()
                .unwrap()
                .contains("bayan cire kuɗin hanyar sadarwa")
        );
        assert!(
            data["ha"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("ba a kulle ba")
        );
        assert_eq!(
            data["ha"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );

        assert!(
            data["he"][recovery_key]
                .as_str()
                .unwrap()
                .contains("ביטוי השחזור בן 25 המילים")
        );
        assert_eq!(
            data["he"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "חיוב כולל בארנק (סכום + עמלת רשת)"
        );
        assert!(
            data["he"][privacy_key]
                .as_str()
                .unwrap()
                .contains("כתובת ה-IP שלך ואת שאילתות הארנק לצורך סנכרון")
        );
        assert!(
            data["he"][privacy_key]
                .as_str()
                .unwrap()
                .contains("האפליקציה תבקש שערי חליפין מ-rates.nexawal.com")
        );
        assert!(
            !data["he"][privacy_key]
                .as_str()
                .unwrap()
                .contains("צור קשר עם rates.nexawal.com")
        );
        assert!(
            data["he"][send_max_key]
                .as_str()
                .unwrap()
                .contains("הסכום המרבי שניתן להוציא לאחר עמלת הרשת")
        );
        assert!(
            data["he"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("היתרה הלא-נעולה")
        );
        assert_eq!(
            data["he"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );
    }

    #[test]
    fn hindi_and_croatian_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let send_max_mode_key = "Send Max mode: Confirm will sweep all unlocked after fee.";

        assert!(
            data["hi"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25-शब्दीय पुनर्प्राप्ति वाक्यांश")
        );
        assert_eq!(
            data["hi"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "वॉलेट से कुल कटौती (राशि + नेटवर्क शुल्क)"
        );
        assert!(
            data["hi"][privacy_key]
                .as_str()
                .unwrap()
                .contains("ऐप rates.nexawal.com से विनिमय दर माँगता है")
        );
        assert!(
            !data["hi"][privacy_key]
                .as_str()
                .unwrap()
                .contains("contact rates.nexawal.com")
        );
        assert!(
            data["hi"][send_max_key]
                .as_str()
                .unwrap()
                .contains("नेटवर्क शुल्क घटाने के बाद")
        );
        assert!(
            data["hi"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("नेटवर्क शुल्क घटाने के बाद")
        );
        assert_eq!(
            data["hi"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );

        assert!(
            data["hr"][recovery_key]
                .as_str()
                .unwrap()
                .contains("frazu za oporavak od 25 riječi")
        );
        assert_eq!(
            data["hr"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Ukupno terećenje novčanika (iznos + mrežna naknada)"
        );
        assert!(
            data["hr"][privacy_key]
                .as_str()
                .unwrap()
                .contains("aplikacija dohvaća tečajeve s rates.nexawal.com")
        );
        assert!(
            !data["hr"][privacy_key]
                .as_str()
                .unwrap()
                .contains("kontaktirajte rates.nexawal.com")
        );
        assert!(
            data["hr"][send_max_key]
                .as_str()
                .unwrap()
                .contains("nakon odbitka mrežne naknade")
        );
        assert!(
            data["hr"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("nakon odbitka mrežne naknade")
        );
        assert_eq!(
            data["hr"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );
    }

    #[test]
    fn hungarian_and_armenian_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let send_max_mode_key = "Send Max mode: Confirm will sweep all unlocked after fee.";

        assert!(
            data["hu"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25 szavas helyreállítási kifejezés")
        );
        assert_eq!(
            data["hu"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "A pénztárcából levont teljes összeg (összeg + hálózati díj)"
        );
        assert!(
            data["hu"][privacy_key]
                .as_str()
                .unwrap()
                .contains("az alkalmazás a rates.nexawal.com oldalról kéri le az árfolyamokat")
        );
        assert!(
            !data["hu"][privacy_key]
                .as_str()
                .unwrap()
                .contains("forduljon a rates.nexawal.com")
        );
        assert!(
            data["hu"][send_max_key]
                .as_str()
                .unwrap()
                .contains("hálózati díj levonása után")
        );
        assert!(
            data["hu"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("hálózati díj levonása után")
        );
        assert_eq!(
            data["hu"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );

        assert!(
            data["hy"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25 բառից բաղկացած վերականգնման արտահայտության")
        );
        assert_eq!(
            data["hy"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Դրամապանակից ընդհանուր գանձվող գումարը (գումար + ցանցի վճար)"
        );
        assert!(
            data["hy"][privacy_key]
                .as_str()
                .unwrap()
                .contains("հավելվածը փոխարժեքները հարցնում է rates.nexawal.com-ից")
        );
        assert!(
            !data["hy"][privacy_key]
                .as_str()
                .unwrap()
                .contains("կապվեք rates.nexawal.com")
        );
        assert!(
            data["hy"][send_max_key]
                .as_str()
                .unwrap()
                .contains("ցանցի վճարը հանելուց հետո")
        );
        assert!(
            data["hy"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("ցանցի վճարը հանելուց հետո")
        );
        assert_eq!(
            data["hy"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );
    }

    #[test]
    fn indonesian_and_korean_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let send_max_mode_key = "Send Max mode: Confirm will sweep all unlocked after fee.";

        assert!(
            data["id"][recovery_key]
                .as_str()
                .unwrap()
                .contains("frasa pemulihan 25 kata")
        );
        assert_eq!(
            data["id"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Total yang dipotong dari dompet (jumlah + biaya jaringan)"
        );
        assert!(
            data["id"][privacy_key]
                .as_str()
                .unwrap()
                .contains("aplikasi mengambil kurs dari rates.nexawal.com")
        );
        assert!(
            !data["id"][privacy_key]
                .as_str()
                .unwrap()
                .contains("hubungi rates.nexawal.com")
        );
        assert!(
            data["id"][send_max_key]
                .as_str()
                .unwrap()
                .contains("setelah biaya jaringan dipotong")
        );
        assert!(
            data["id"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("setelah biaya jaringan dipotong")
        );
        assert_eq!(
            data["id"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );

        assert!(
            data["ko"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25단어 복구 문구")
        );
        assert_eq!(
            data["ko"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "지갑에서 차감되는 총액 (금액 + 네트워크 수수료)"
        );
        assert!(
            data["ko"][privacy_key]
                .as_str()
                .unwrap()
                .contains("앱이 rates.nexawal.com에서 환율을 가져옵니다")
        );
        assert!(
            !data["ko"][privacy_key]
                .as_str()
                .unwrap()
                .contains("rates.nexawal.com에 문의")
        );
        assert!(
            data["ko"][send_max_key]
                .as_str()
                .unwrap()
                .contains("네트워크 수수료를 뺀 최대 사용 가능 금액")
        );
        assert!(
            data["ko"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("네트워크 수수료를 제외한")
        );
        assert_eq!(
            data["ko"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );
    }

    #[test]
    fn kyrgyz_and_lithuanian_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let send_max_mode_key = "Send Max mode: Confirm will sweep all unlocked after fee.";

        assert!(
            data["ky"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25 сөздөн турган калыбына келтирүү фраза")
        );
        assert_eq!(
            data["ky"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Капчыктан жалпы алынуучу сумма (сумма + тармактык комиссия)"
        );
        assert!(
            data["ky"][privacy_key]
                .as_str()
                .unwrap()
                .contains("колдонмо курстарды rates.nexawal.com сайтынан алат")
        );
        assert!(
            !data["ky"][privacy_key]
                .as_str()
                .unwrap()
                .contains("rates.nexawal.com менен байланышыңыз")
        );
        assert!(
            data["ky"][send_max_key]
                .as_str()
                .unwrap()
                .contains("тармактык комиссия алынгандан кийин")
        );
        assert!(
            data["ky"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("тармактык комиссия алынгандан кийин")
        );
        assert_eq!(
            data["ky"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );

        assert!(
            data["lt"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25 žodžių atkūrimo frazę")
        );
        assert_eq!(
            data["lt"]["Wallet debit (amount + fee)"].as_str().unwrap(),
            "Iš piniginės nurašoma suma (suma + tinklo mokestis)"
        );
        assert!(
            data["lt"][privacy_key]
                .as_str()
                .unwrap()
                .contains("programėlė kursus gauna iš rates.nexawal.com")
        );
        assert!(
            !data["lt"][privacy_key]
                .as_str()
                .unwrap()
                .contains("susisiekite su rates.nexawal.com")
        );
        assert!(
            data["lt"][send_max_key]
                .as_str()
                .unwrap()
                .contains("atskaičius tinklo mokestį")
        );
        assert!(
            data["lt"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("atskaičius tinklo mokestį")
        );
        assert_eq!(
            data["lt"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );
    }

    #[test]
    fn latvian_and_malay_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let send_max_mode_key = "Send Max mode: Confirm will sweep all unlocked after fee.";
        let max_preview_key =
            "Max preview ready. Confirm Send Max will sweep all unlocked after fee.";

        assert!(
            data["lv"][recovery_key]
                .as_str()
                .unwrap()
                .contains("25 vārdu atkopšanas frāzi")
        );
        assert!(
            data["lv"]["Wallet debit (amount + fee)"]
                .as_str()
                .unwrap()
                .contains("summa + tīkla maksa")
        );
        assert!(
            data["lv"][privacy_key]
                .as_str()
                .unwrap()
                .contains("lietotne iegūst kursus no rates.nexawal.com")
        );
        assert!(
            !data["lv"][privacy_key]
                .as_str()
                .unwrap()
                .contains("sazinieties ar rates.nexawal.com")
        );
        assert!(
            data["lv"][send_max_key]
                .as_str()
                .unwrap()
                .contains("pēc tīkla maksas atskaitīšanas")
        );
        assert!(
            data["lv"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("pēc tīkla maksas atskaitīšanas")
        );
        assert!(
            data["lv"][max_preview_key]
                .as_str()
                .unwrap()
                .contains("pēc tīkla maksas atskaitīšanas")
        );
        assert_eq!(
            data["lv"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );

        assert!(
            data["ms"][recovery_key]
                .as_str()
                .unwrap()
                .contains("frasa pemulihan 25 perkataan")
        );
        assert!(
            data["ms"]["Wallet debit (amount + fee)"]
                .as_str()
                .unwrap()
                .contains("amaun + yuran rangkaian")
        );
        assert!(
            data["ms"][privacy_key]
                .as_str()
                .unwrap()
                .contains("aplikasi mendapatkan kadar daripada rates.nexawal.com")
        );
        assert!(
            !data["ms"][privacy_key]
                .as_str()
                .unwrap()
                .contains("hubungi rates.nexawal.com")
        );
        assert!(
            data["ms"][send_max_key]
                .as_str()
                .unwrap()
                .contains("selepas yuran rangkaian ditolak")
        );
        assert!(
            data["ms"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("selepas yuran rangkaian ditolak")
        );
        assert!(
            data["ms"][max_preview_key]
                .as_str()
                .unwrap()
                .contains("selepas yuran rangkaian ditolak")
        );
        assert_eq!(
            data["ms"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );

        for locale in ["lv", "ms"] {
            for key in [
                "Date range",
                "Filter",
                "Loading transactions…",
                "Mainnet",
                "Sync failed",
                "Unlocking…",
            ] {
                assert_ne!(
                    data[locale][key].as_str().unwrap(),
                    key,
                    "{key} is untranslated in {locale}"
                );
            }
        }
    }

    #[test]
    fn burmese_and_norwegian_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let send_max_mode_key = "Send Max mode: Confirm will sweep all unlocked after fee.";
        let max_preview_key =
            "Max preview ready. Confirm Send Max will sweep all unlocked after fee.";

        assert!(
            data["my"][recovery_key]
                .as_str()
                .unwrap()
                .contains("စကားလုံး ၂၅ လုံးပါ ပြန်လည်ရယူရေး စကားစု")
        );
        assert!(
            data["my"]["Wallet debit (amount + fee)"]
                .as_str()
                .unwrap()
                .contains("ပမာဏ + ကွန်ရက်အခကြေးငွေ")
        );
        assert!(
            data["my"][privacy_key]
                .as_str()
                .unwrap()
                .contains("အက်ပ်သည် rates.nexawal.com မှ ဈေးနှုန်းများကို ရယူသည်")
        );
        assert!(
            !data["my"][privacy_key]
                .as_str()
                .unwrap()
                .contains("rates.nexawal.com သို့ ဆက်သွယ်ပါ")
        );
        assert!(
            data["my"][send_max_key]
                .as_str()
                .unwrap()
                .contains("ကွန်ရက်အခကြေးငွေ နုတ်ပြီးနောက်")
        );
        assert!(
            data["my"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("ကွန်ရက်အခကြေးငွေ နုတ်ပြီးနောက်")
        );
        assert!(
            data["my"][max_preview_key]
                .as_str()
                .unwrap()
                .contains("ကွန်ရက်အခကြေးငွေ နုတ်ပြီးနောက်")
        );
        assert_eq!(
            data["my"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );

        assert!(
            data["nb"][recovery_key]
                .as_str()
                .unwrap()
                .contains("gjenopprettingsfrasen på 25 ord")
        );
        assert!(
            data["nb"]["Wallet debit (amount + fee)"]
                .as_str()
                .unwrap()
                .contains("beløp + nettverksgebyr")
        );
        assert!(
            data["nb"][privacy_key]
                .as_str()
                .unwrap()
                .contains("henter appen kurs fra rates.nexawal.com")
        );
        assert!(
            !data["nb"][privacy_key]
                .as_str()
                .unwrap()
                .contains("kontakt rates.nexawal.com")
        );
        assert!(
            data["nb"][send_max_key]
                .as_str()
                .unwrap()
                .contains("etter at nettverksgebyret er trukket fra")
        );
        assert!(
            data["nb"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("etter at nettverksgebyret er trukket fra")
        );
        assert!(
            data["nb"][max_preview_key]
                .as_str()
                .unwrap()
                .contains("etter at nettverksgebyret er trukket fra")
        );
        assert_eq!(
            data["nb"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );

        for locale in ["my", "nb"] {
            for key in [
                "Date range",
                "Filter",
                "Loading transactions…",
                "Mainnet",
                "Sync failed",
                "Unlocking…",
            ] {
                assert_ne!(
                    data[locale][key].as_str().unwrap(),
                    key,
                    "{key} is untranslated in {locale}"
                );
            }
        }
    }

    #[test]
    fn romanian_and_slovak_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let send_max_mode_key = "Send Max mode: Confirm will sweep all unlocked after fee.";
        let max_preview_key =
            "Max preview ready. Confirm Send Max will sweep all unlocked after fee.";
        let replace_key = "This will replace the existing wallet on this device.\n\nIf you continue, the currently stored mnemonic and scan state will be removed. Make sure you have your mnemonic backed up before proceeding.";
        let desktop_seed_box_key =
            "Click the seed box, paste with ⌘V, set restore height, then Open & sync.";
        let locked_wallet_key =
            "Locked. Open the existing wallet from {}, or restore a different seed.";
        let open_existing_wallet_key =
            "Open the existing wallet from {}, or restore a different seed.";
        let unlock_retry_key = "Use Open existing wallet to try again, or restore from seed.";

        let romanian_recovery = data["ro"][recovery_key].as_str().unwrap();
        assert!(romanian_recovery.contains("fraza de recuperare de 25 de cuvinte"));
        let romanian_replace = data["ro"][replace_key].as_str().unwrap();
        assert!(romanian_replace.contains("fraza de recuperare"));
        assert!(!romanian_replace.contains("mnemonic"));
        assert!(
            data["ro"]["Wallet debit (amount + fee)"]
                .as_str()
                .unwrap()
                .contains("sumă + comision")
        );
        let romanian_privacy = data["ro"][privacy_key].as_str().unwrap();
        assert!(
            romanian_privacy.contains("adresa ta IP și cererile de sincronizare ale portofelului")
        );
        assert!(romanian_privacy.contains("aplicația preia cursurile de la rates.nexawal.com"));
        assert!(!romanian_privacy.contains("contactați rates.nexawal.com"));
        let romanian_send_max = data["ro"][send_max_key].as_str().unwrap();
        assert!(romanian_send_max.contains("după deducerea comisionului de rețea"));
        assert_eq!(romanian_send_max.matches("%@").count(), 3);
        assert!(
            data["ro"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("după deducerea comisionului de rețea")
        );
        assert!(
            data["ro"][max_preview_key]
                .as_str()
                .unwrap()
                .contains("după deducerea comisionului de rețea")
        );

        let slovak_recovery = data["sk"][recovery_key].as_str().unwrap();
        assert!(slovak_recovery.contains("25-slovnú frázu na obnovenie"));
        let slovak_replace = data["sk"][replace_key].as_str().unwrap();
        assert!(slovak_replace.contains("frázu na obnovenie"));
        assert!(!slovak_replace.contains("mnemonic"));
        assert!(
            data["sk"]["Wallet debit (amount + fee)"]
                .as_str()
                .unwrap()
                .contains("suma + poplatok")
        );
        let slovak_privacy = data["sk"][privacy_key].as_str().unwrap();
        assert!(slovak_privacy.contains("vašu IP adresu a požiadavky na synchronizáciu peňaženky"));
        assert!(slovak_privacy.contains("aplikácia načítava kurzy zo servera rates.nexawal.com"));
        assert!(!slovak_privacy.contains("kontaktujte rates.nexawal.com"));
        let slovak_send_max = data["sk"][send_max_key].as_str().unwrap();
        assert!(slovak_send_max.contains("po odpočítaní sieťového poplatku"));
        assert_eq!(slovak_send_max.matches("%@").count(), 3);
        assert!(
            data["sk"][send_max_mode_key]
                .as_str()
                .unwrap()
                .contains("po odpočítaní sieťového poplatku")
        );
        assert!(
            data["sk"][max_preview_key]
                .as_str()
                .unwrap()
                .contains("po odpočítaní sieťového poplatku")
        );
        assert_eq!(
            data["ro"]["%llu confirmations"].as_str().unwrap(),
            "Număr de confirmări: %llu"
        );
        assert_eq!(
            data["sk"]["%llu confirmations"].as_str().unwrap(),
            "Počet potvrdení: %llu"
        );
        assert_eq!(data["ro"]["Prev"].as_str().unwrap(), "Anterior");
        assert_eq!(data["sk"]["Prev"].as_str().unwrap(), "Predch.");
        assert_eq!(
            data["ro"]["{0} confirmations"].as_str().unwrap(),
            "Număr de confirmări: {0}"
        );
        assert_eq!(
            data["sk"]["{0} confirmations"].as_str().unwrap(),
            "Počet potvrdení: {0}"
        );

        for key in [
            desktop_seed_box_key,
            locked_wallet_key,
            open_existing_wallet_key,
            unlock_retry_key,
        ] {
            let romanian = data["ro"][key].as_str().unwrap();
            assert!(
                romanian.contains("recuperare"),
                "Romanian recovery phrase wording missing from {key}"
            );
            assert!(
                !romanian.contains("seed"),
                "Technical seed term remains in Romanian: {key}"
            );
            let slovak = data["sk"][key].as_str().unwrap();
            assert!(
                slovak.contains("obnoven"),
                "Slovak recovery phrase wording missing from {key}"
            );
            assert!(
                !slovak.contains("seed"),
                "Technical seed term remains in Slovak: {key}"
            );
        }

        for locale in ["ro", "sk"] {
            for key in [
                "Date range",
                "Filter",
                "Loading transactions…",
                "Mainnet",
                "Sync failed",
                "Unlocking…",
            ] {
                assert_ne!(
                    data[locale][key].as_str().unwrap(),
                    key,
                    "{key} is untranslated in {locale}"
                );
            }
        }
    }

    #[test]
    fn slovenian_and_albanian_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let send_max_mode_key = "Send Max mode: Confirm will sweep all unlocked after fee.";
        let max_preview_key =
            "Max preview ready. Confirm Send Max will sweep all unlocked after fee.";
        let debit_key = "Wallet debit (amount + fee)";
        let recovery_keys = [
            recovery_key,
            "This will replace the existing wallet on this device.\n\nIf you continue, the currently stored mnemonic and scan state will be removed. Make sure you have your mnemonic backed up before proceeding.",
            "Generate new seed",
            "I wrote down my recovery seed",
            "Mnemonic (paste):",
            "Mnemonic cannot be empty.",
            "Stored mnemonic could not be found.",
            "Rescan failed: mnemonic is unavailable.",
            "Click the seed box, paste with ⌘V, set restore height, then Open & sync.",
            "Locked. Open the existing wallet from {}, or restore a different seed.",
            "Open the existing wallet from {}, or restore a different seed.",
            "Use Open existing wallet to try again, or restore from seed.",
        ];

        for (locale, recovery_term, fee_term) in [
            ("sl", "obnovit", "omrežnine"),
            ("sq", "rikuperim", "rrjetit"),
        ] {
            assert!(
                data[locale][recovery_key]
                    .as_str()
                    .unwrap()
                    .contains(recovery_term)
            );
            assert!(
                data[locale][debit_key]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("odpis")
                    || locale == "sq"
            );
            assert!(
                data[locale][debit_key]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("debiti")
                    || locale == "sl"
            );
            assert!(
                data[locale][privacy_key]
                    .as_str()
                    .unwrap()
                    .contains("rates.nexawal.com")
            );
            assert!(
                !data[locale][privacy_key]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("contact")
            );
            for key in recovery_keys {
                let value = data[locale][key].as_str().unwrap().to_lowercase();
                assert!(
                    !value.contains("seed"),
                    "technical seed wording remains in {locale}: {key}"
                );
                assert!(
                    !value.contains("mnemonic"),
                    "technical mnemonic wording remains in {locale}: {key}"
                );
            }
            for key in [send_max_key, send_max_mode_key, max_preview_key] {
                assert!(
                    data[locale][key].as_str().unwrap().contains(fee_term),
                    "fee deduction is unclear in {locale}: {key}"
                );
            }
            assert_eq!(
                data[locale][send_max_key]
                    .as_str()
                    .unwrap()
                    .matches("%@")
                    .count(),
                3
            );
            assert_eq!(
                data[locale]["%llu confirmations"]
                    .as_str()
                    .unwrap()
                    .matches("%llu")
                    .count(),
                1
            );
            for key in [
                "Date range",
                "Filter",
                "Loading transactions…",
                "Mainnet",
                "Sync failed",
                "Unlocking…",
            ] {
                assert_ne!(
                    data[locale][key].as_str().unwrap(),
                    key,
                    "{key} is untranslated in {locale}"
                );
            }
        }
    }

    #[test]
    fn swedish_and_thai_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let send_max_mode_key = "Send Max mode: Confirm will sweep all unlocked after fee.";
        let max_preview_key =
            "Max preview ready. Confirm Send Max will sweep all unlocked after fee.";
        let recovery_keys = [
            recovery_key,
            "This will replace the existing wallet on this device.\n\nIf you continue, the currently stored mnemonic and scan state will be removed. Make sure you have your mnemonic backed up before proceeding.",
            "Generate new seed",
            "I wrote down my recovery seed",
            "Mnemonic (paste):",
            "Mnemonic cannot be empty.",
            "Stored mnemonic could not be found.",
            "Rescan failed: mnemonic is unavailable.",
            "Click the seed box, paste with ⌘V, set restore height, then Open & sync.",
            "Locked. Open the existing wallet from {}, or restore a different seed.",
            "Open the existing wallet from {}, or restore a different seed.",
            "Use Open existing wallet to try again, or restore from seed.",
        ];

        for (locale, recovery_term, fee_term, confirmations, destination) in [
            (
                "sv",
                "återställningsfras",
                "nätverksavgiften",
                "{0} bekräftelser",
                "Mottagare",
            ),
            (
                "th",
                "วลีกู้คืน",
                "ค่าธรรมเนียมเครือข่าย",
                "จำนวนการยืนยัน: {0}",
                "ปลายทาง",
            ),
        ] {
            assert!(
                data[locale][recovery_key]
                    .as_str()
                    .unwrap()
                    .contains(recovery_term)
            );
            let privacy = data[locale][privacy_key].as_str().unwrap();
            assert!(privacy.contains("rates.nexawal.com"));
            assert!(!privacy.to_lowercase().contains("contact"));
            for key in recovery_keys {
                let value = data[locale][key].as_str().unwrap().to_lowercase();
                assert!(
                    !value.contains("seed"),
                    "technical seed wording remains in {locale}: {key}"
                );
                assert!(
                    !value.contains("mnemonic"),
                    "technical mnemonic wording remains in {locale}: {key}"
                );
                assert!(
                    !value.contains("frö"),
                    "technical seed wording remains in Swedish: {key}"
                );
                assert!(
                    !value.contains("ซีด") && !value.contains("ช่วยจำ"),
                    "technical seed wording remains in Thai: {key}"
                );
            }
            for key in [send_max_key, send_max_mode_key, max_preview_key] {
                assert!(
                    data[locale][key].as_str().unwrap().contains(fee_term),
                    "fee deduction is unclear in {locale}: {key}"
                );
            }
            assert_eq!(
                data[locale][send_max_key]
                    .as_str()
                    .unwrap()
                    .matches("%@")
                    .count(),
                3
            );
            assert!(
                data[locale]["Wallet debit (amount + fee)"]
                    .as_str()
                    .unwrap()
                    .contains("debiterat")
                    || locale == "th"
            );
            assert_eq!(
                data[locale]["%llu confirmations"]
                    .as_str()
                    .unwrap()
                    .matches("%llu")
                    .count(),
                1
            );
            assert_eq!(
                data[locale]["{0} confirmations"].as_str().unwrap(),
                confirmations
            );
            assert_eq!(data[locale]["Destination"].as_str().unwrap(), destination);
            for key in [
                "Date range",
                "Filter",
                "Loading transactions…",
                "Mainnet",
                "Sync failed",
                "Unlocking…",
            ] {
                assert_ne!(
                    data[locale][key].as_str().unwrap(),
                    key,
                    "{key} is untranslated in {locale}"
                );
            }
        }
    }

    #[test]
    fn ukrainian_and_urdu_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let send_max_mode_key = "Send Max mode: Confirm will sweep all unlocked after fee.";
        let max_preview_key =
            "Max preview ready. Confirm Send Max will sweep all unlocked after fee.";
        let recovery_keys = [
            recovery_key,
            "This will replace the existing wallet on this device.\n\nIf you continue, the currently stored mnemonic and scan state will be removed. Make sure you have your mnemonic backed up before proceeding.",
            "Generate new seed",
            "I wrote down my recovery seed",
            "Mnemonic (paste):",
            "Mnemonic cannot be empty.",
            "Stored mnemonic could not be found.",
            "Rescan failed: mnemonic is unavailable.",
            "Paste 25-word mnemonic…",
            "Stored metadata does not contain a mnemonic",
            "Your recovery seed:",
            "Seed word",
        ];

        for (locale, recovery_term, fee_term, confirmations, destination) in [
            (
                "uk",
                "фразу відновлення",
                "мережевої комісії",
                "Підтверджень: {0}",
                "Призначення",
            ),
            ("ur", "بحالی", "نیٹ ورک فیس", "تصدیقات: {0}", "منزل"),
        ] {
            assert!(
                data[locale][recovery_key]
                    .as_str()
                    .unwrap()
                    .contains(recovery_term)
            );
            for key in recovery_keys {
                let value = data[locale][key].as_str().unwrap().to_lowercase();
                assert!(
                    !value.contains("seed"),
                    "technical seed wording remains in {locale}: {key}"
                );
                assert!(
                    !value.contains("mnemonic"),
                    "technical mnemonic wording remains in {locale}: {key}"
                );
                if locale == "uk" {
                    assert!(
                        !value.contains("мнемон"),
                        "technical mnemonic transliteration remains in Ukrainian: {key}"
                    );
                } else {
                    assert!(
                        !value.contains("سیڈ") && !value.contains("منیمونک"),
                        "technical seed/mnemonic transliteration remains in Urdu: {key}"
                    );
                }
            }

            let privacy = data[locale][privacy_key].as_str().unwrap();
            assert!(privacy.contains("rates.nexawal.com"));
            assert!(privacy.contains("IP"));
            assert!(!privacy.to_lowercase().contains("contact"));
            assert!(!privacy.contains("رابطہ"));

            for key in [send_max_key, send_max_mode_key, max_preview_key] {
                assert!(
                    data[locale][key].as_str().unwrap().contains(fee_term),
                    "fee deduction is unclear in {locale}: {key}"
                );
            }
            assert_eq!(
                data[locale][send_max_key]
                    .as_str()
                    .unwrap()
                    .matches("%@")
                    .count(),
                3
            );
            assert!(
                data[locale]["Wallet debit (amount + fee)"]
                    .as_str()
                    .unwrap()
                    .contains("списання")
                    || locale == "ur"
            );
            assert_eq!(
                data[locale]["%llu confirmations"]
                    .as_str()
                    .unwrap()
                    .matches("%llu")
                    .count(),
                1
            );
            assert_eq!(
                data[locale]["{0} confirmations"].as_str().unwrap(),
                confirmations
            );
            assert_eq!(data[locale]["Destination"].as_str().unwrap(), destination);

            for key in [
                "Date range",
                "Filter",
                "Loading transactions…",
                "Mainnet",
                "Sync failed",
                "Unlocking…",
            ] {
                assert_ne!(
                    data[locale][key].as_str().unwrap(),
                    key,
                    "{key} is untranslated in {locale}"
                );
            }
        }
    }

    #[test]
    fn arabic_and_polish_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let custody_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let recovery_keys = [
            "Generate new seed",
            "I wrote down my recovery seed",
            "Mnemonic (paste):",
            "Mnemonic cannot be empty.",
            "Rescan failed: mnemonic is unavailable.",
            "Stored mnemonic could not be found.",
            "Paste 25-word mnemonic…",
            "Stored metadata does not contain a mnemonic",
            "Seed copied. Store it offline, then clear the clipboard.",
            "Terms accepted. Paste your seed, then Open & sync.",
            "Check that you wrote the seed down and confirm the three words before opening.",
        ];

        for (locale, recovery_term, fee_term, fee_help_term, debit_term, forbidden, destination) in [
            (
                "ar",
                "عبارة الاسترداد",
                "رسوم الشبكة",
                "رسوم الشبكة",
                "المخصوم",
                ["بذرة", "العبارة التذكيرية"].as_slice(),
                "الوجهة",
            ),
            (
                "pl",
                "frazę odzyskiwania",
                "opłaty sieciowej",
                "opłatą sieciową",
                "obciążenie",
                [
                    "seed",
                    "mnemonic",
                    "ziarno",
                    "self-custodial",
                    "plik początkowy",
                ]
                .as_slice(),
                "Adres odbiorcy",
            ),
        ] {
            let custody = data[locale][custody_key].as_str().unwrap();
            assert!(
                custody
                    .to_lowercase()
                    .contains(&recovery_term.to_lowercase()),
                "recovery phrase missing in {locale}"
            );
            for term in forbidden {
                assert!(
                    !custody.to_lowercase().contains(&term.to_lowercase()),
                    "technical wording in {locale}: {term}"
                );
            }

            for key in recovery_keys {
                let value = data[locale][key].as_str().unwrap().to_lowercase();
                for term in forbidden {
                    assert!(
                        !value.contains(&term.to_lowercase()),
                        "technical wording in {locale}: {key}"
                    );
                }
            }

            let send_max = data[locale][send_max_key].as_str().unwrap();
            assert!(
                send_max.to_lowercase().contains(&fee_term.to_lowercase()),
                "Send Max fee deduction missing in {locale}"
            );
            assert_eq!(
                send_max.matches("%@").count(),
                3,
                "Send Max arguments changed in {locale}"
            );
            assert!(
                data[locale]["Send Max mode: Confirm will sweep all unlocked after fee."]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains(&fee_term.to_lowercase())
            );
            assert!(data[locale]["Max preview ready. Confirm Send Max will sweep all unlocked after fee."].as_str().unwrap().to_lowercase().contains(&fee_term.to_lowercase()));
            assert!(
                data[locale]["Wallet debit (amount + fee)"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains(&debit_term.to_lowercase())
            );
            assert!(
                data[locale]["Amount is the total wallet debit, including the network fee."]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains(&fee_help_term.to_lowercase())
            );
            assert_eq!(
                data[locale]["%llu confirmations"]
                    .as_str()
                    .unwrap()
                    .matches("%llu")
                    .count(),
                1
            );
            assert_eq!(
                data[locale]["{0} confirmations"]
                    .as_str()
                    .unwrap()
                    .matches("{0}")
                    .count(),
                1
            );
            if locale == "ar" {
                assert!(
                    data[locale]["%llu confirmations"]
                        .as_str()
                        .unwrap()
                        .contains("عدد التأكيدات")
                );
                assert!(
                    data[locale]["{0} confirmations"]
                        .as_str()
                        .unwrap()
                        .contains("عدد التأكيدات")
                );
            } else {
                assert_eq!(
                    data[locale]["{0} confirmations"].as_str().unwrap(),
                    "Liczba potwierdzeń: {0}"
                );
            }
            assert_eq!(data[locale]["Destination"].as_str().unwrap(), destination);

            let privacy = data[locale][privacy_key].as_str().unwrap();
            assert!(privacy.contains("rates.nexawal.com"));
            assert!(privacy.to_lowercase().contains("ip"));
            assert!(!privacy.to_lowercase().contains("contact"));

            for key in [
                "Date range",
                "Filter",
                "Loading transactions…",
                "Mainnet",
                "Sync failed",
                "Unlocking…",
            ] {
                assert_ne!(
                    data[locale][key].as_str().unwrap(),
                    key,
                    "{key} is untranslated in {locale}"
                );
            }
        }
    }

    #[test]
    fn brazilian_portuguese_and_russian_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let custody_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let rate_key = "Optional. When enabled, NexaWal requests an XMR rate for your selected currency from rates.nexawal.com over HTTPS. This request uses clearnet even when the wallet node uses I2P. Nexatrode can see your IP address and selected currency; wallet amounts, addresses, seeds, and transaction history are not sent. Estimates only — XMR is what you send and hold.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let recovery_keys = [
            "Generate new seed",
            "I wrote down my recovery seed",
            "Mnemonic (paste):",
            "Mnemonic cannot be empty.",
            "Rescan failed: mnemonic is unavailable.",
            "Stored mnemonic could not be found.",
            "This will replace the existing wallet on this device.\n\nIf you continue, the currently stored mnemonic and scan state will be removed. Make sure you have your mnemonic backed up before proceeding.",
            "Paste 25-word mnemonic…",
            "Stored metadata does not contain a mnemonic",
            "Seed copied. Store it offline, then clear the clipboard.",
            "Terms accepted. Paste your seed, then Open & sync.",
            "Your recovery seed:",
            "Seed word",
            "Check that you wrote the seed down and confirm the three words before opening.",
            "Paste clipboard into seed",
            "Seed field is empty.",
            "Seed field is empty. Paste or type the 25-word phrase first.",
            "Seed phrase",
            "Stored metadata does not contain a mnemonic",
            "Click the seed box, paste with ⌘V, set restore height, then Open & sync.",
            "Write this seed down on paper. Open is locked until you confirm three words.",
            "Seed field has {0} words. Set restore height, then Open & sync.",
            "Your recovery seed is stored in {0}. Open it with the same wallet screens, settings, history, and local scan cache.",
            "Paste clipboard into seed",
        ];

        for (locale, recovery_term, fee_term, fee_help_term, debit_term, forbidden) in [
            (
                "pt-BR",
                "frase de recuperação",
                "taxa de rede",
                "taxa de rede",
                "Débito total",
                ["seed", "mnemônico", "mnemônica", "semente"].as_slice(),
            ),
            (
                "ru",
                "фраз",
                "сетевой комиссии",
                "сетевую комиссию",
                "Полное списание",
                ["seed", "сид-фраз", "мнемон"].as_slice(),
            ),
        ] {
            let custody = data[locale][custody_key].as_str().unwrap();
            assert!(
                custody
                    .to_lowercase()
                    .contains(&recovery_term.to_lowercase()),
                "recovery wording missing in {locale}"
            );
            for term in forbidden {
                assert!(
                    !custody.to_lowercase().contains(&term.to_lowercase()),
                    "technical custody term in {locale}: {term}"
                );
            }
            for key in recovery_keys {
                let value = data[locale][key].as_str().unwrap().to_lowercase();
                assert!(
                    value.contains(&recovery_term.to_lowercase()),
                    "recovery phrase wording missing in {locale}: {key}"
                );
                for term in forbidden {
                    assert!(
                        !value.contains(&term.to_lowercase()),
                        "technical recovery term in {locale}: {key}"
                    );
                }
            }

            let send_max = data[locale][send_max_key].as_str().unwrap();
            assert!(
                send_max.to_lowercase().contains(&fee_term.to_lowercase()),
                "Send Max fee deduction missing in {locale}"
            );
            assert_eq!(
                send_max.matches("%@").count(),
                3,
                "Send Max arguments changed in {locale}"
            );
            for key in [
                "Authenticate to sweep the wallet balance",
                "Send Max mode: Confirm will sweep all unlocked after fee.",
                "Max preview ready. Confirm Send Max will sweep all unlocked after fee.",
            ] {
                assert!(
                    data[locale][key]
                        .as_str()
                        .unwrap()
                        .to_lowercase()
                        .contains(&fee_term.to_lowercase()),
                    "fee disclosure missing in {locale}: {key}"
                );
            }
            assert!(
                data[locale]["Wallet debit (amount + fee)"]
                    .as_str()
                    .unwrap()
                    .contains(debit_term)
            );
            assert!(
                data[locale]["Amount is the total wallet debit, including the network fee."]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains(&fee_help_term.to_lowercase())
            );

            for (key, placeholder) in [("%llu confirmations", "%llu"), ("{0} confirmations", "{0}")]
            {
                assert_eq!(
                    data[locale][key]
                        .as_str()
                        .unwrap()
                        .matches(placeholder)
                        .count(),
                    1,
                    "placeholder changed in {locale}: {key}"
                );
            }
            let hybrid_policy = data[locale]["Both (scan clearnet, broadcast I2P)"]
                .as_str()
                .unwrap();
            if locale == "pt-BR" {
                assert!(
                    hybrid_policy.to_lowercase().contains("sincronizar")
                        && hybrid_policy.to_lowercase().contains("transmitir")
                );
                assert!(!hybrid_policy.to_lowercase().contains("varrer"));
                assert!(
                    data[locale]["Scan additional accounts or subaddresses"]
                        .as_str()
                        .unwrap()
                        .to_lowercase()
                        .contains("escanear")
                );
            } else {
                assert!(
                    hybrid_policy.to_lowercase().contains("синхронизация")
                        && hybrid_policy.to_lowercase().contains("транзакций")
                );
                assert!(
                    data[locale]["Rescan from Height"]
                        .as_str()
                        .unwrap()
                        .to_lowercase()
                        .contains("синхронизировать")
                );
            }
            let clipboard_paste = data[locale]["Paste clipboard into seed"].as_str().unwrap();
            assert!(
                clipboard_paste
                    .to_lowercase()
                    .contains(&recovery_term.to_lowercase())
            );
            let selected_subaddress_send =
                data[locale]["Swept max spendable from selected subaddress via %@."]
                    .as_str()
                    .unwrap();
            assert!(
                selected_subaddress_send
                    .to_lowercase()
                    .contains(if locale == "pt-BR" {
                        "enviado"
                    } else {
                        "отправлен"
                    })
            );
            assert_eq!(selected_subaddress_send.matches("%@").count(), 1);

            let privacy = data[locale][privacy_key].as_str().unwrap();
            assert!(privacy.contains("rates.nexawal.com") && privacy.to_lowercase().contains("ip"));
            assert!(!privacy.to_lowercase().contains("contact"));
            let rates = data[locale][rate_key].as_str().unwrap();
            assert!(rates.contains("rates.nexawal.com") && rates.contains("I2P"));
            for key in [
                "Date range",
                "Filter",
                "Loading transactions…",
                "Mainnet",
                "Sync failed",
                "Unlocking…",
            ] {
                assert_ne!(
                    data[locale][key].as_str().unwrap(),
                    key,
                    "{key} is untranslated in {locale}"
                );
            }
        }
    }

    #[test]
    fn yoruba_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let recovery_key = "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.";
        let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
        let rate_key = "Optional. When enabled, NexaWal requests an XMR rate for your selected currency from rates.nexawal.com over HTTPS. This request uses clearnet even when the wallet node uses I2P. Nexatrode can see your IP address and selected currency; wallet amounts, addresses, seeds, and transaction history are not sent. Estimates only — XMR is what you send and hold.";
        let send_max_key = "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.";
        let send_max_mode_key = "Send Max mode: Confirm will sweep all unlocked after fee.";
        let max_preview_key =
            "Max preview ready. Confirm Send Max will sweep all unlocked after fee.";
        let recovery_keys = [
            recovery_key,
            "This will replace the existing wallet on this device.\n\nIf you continue, the currently stored mnemonic and scan state will be removed. Make sure you have your mnemonic backed up before proceeding.",
            "Generate new seed",
            "I wrote down my recovery seed",
            "Write these words down in order and store them somewhere safe offline. You'll need them to restore the wallet if this phone is lost. NexaWal also keeps an encrypted copy on device for unlock — treat the backup like cash.",
            "Mnemonic (paste):",
            "Mnemonic cannot be empty.",
            "Stored mnemonic could not be found.",
            "Rescan failed: mnemonic is unavailable.",
            "Paste 25-word mnemonic…",
            "Stored metadata does not contain a mnemonic",
            "Terms accepted. Paste your seed, then Open & sync.",
            "Your recovery seed:",
            "Seed word",
        ];

        assert!(
            data["yo"][recovery_key]
                .as_str()
                .unwrap()
                .contains("ọ̀rọ̀ ìpadàbọ̀")
        );
        for key in recovery_keys {
            let value = data["yo"][key].as_str().unwrap().to_lowercase();
            assert!(
                !value.contains("seed"),
                "technical seed wording remains in Yoruba: {key}"
            );
            assert!(
                !value.contains("mnemonic"),
                "technical mnemonic wording remains in Yoruba: {key}"
            );
            assert!(
                !value.contains("irugbin") && !value.contains("ìrùgbìn"),
                "literal seed wording remains in Yoruba: {key}"
            );
        }

        let privacy = data["yo"][privacy_key].as_str().unwrap();
        assert!(privacy.contains("rates.nexawal.com"));
        assert!(privacy.contains("IP"));
        assert!(!privacy.to_lowercase().contains("contact"));
        assert!(!privacy.contains("kan si rates.nexawal.com"));
        let rates = data["yo"][rate_key].as_str().unwrap();
        assert!(rates.contains("I2P") && rates.contains("rates.nexawal.com"));
        assert!(rates.contains("a kì í fi"));

        for key in [send_max_key, send_max_mode_key, max_preview_key] {
            assert!(
                data["yo"][key].as_str().unwrap().contains("ọya nẹ́tíwọ́ọ̀kì"),
                "network-fee disclosure missing in {key}"
            );
        }
        assert_eq!(
            data["yo"][send_max_key]
                .as_str()
                .unwrap()
                .matches("%@")
                .count(),
            3
        );
        assert!(
            data["yo"]["Wallet debit (amount + fee)"]
                .as_str()
                .unwrap()
                .contains("iye + ọya nẹ́tíwọ́ọ̀kì")
        );
        assert_eq!(
            data["yo"]["%llu confirmations"]
                .as_str()
                .unwrap()
                .matches("%llu")
                .count(),
            1
        );
        assert_eq!(
            data["yo"]["{0} confirmations"].as_str().unwrap(),
            "Ìmúdájú: {0}"
        );
        assert_eq!(
            data["yo"]["Destination"].as_str().unwrap(),
            "Àdírẹ́sì olùgbà"
        );

        for key in [
            "Date range",
            "Filter",
            "Loading transactions…",
            "Mainnet",
            "Sync failed",
            "Unlocking…",
        ] {
            assert_ne!(
                data["yo"][key].as_str().unwrap(),
                key,
                "{key} is untranslated in Yoruba"
            );
        }
    }

    #[test]
    fn vietnamese_and_chinese_wallet_critical_copy_is_clear() {
        let raw_json = include_str!("../assets/l10n.json");
        let data: Value = serde_json::from_str(raw_json).expect("valid localization JSON");
        let locales = [
            (
                "vi",
                "cụm từ khôi phục",
                "phí mạng",
                "Số lần xác nhận",
                "đồng bộ",
                "số tiền + phí",
                "Xác nhận",
            ),
            (
                "zh-Hans",
                "恢复短语",
                "网络手续费",
                "确认次数",
                "同步",
                "金额 + 手续费",
                "确认数",
            ),
            (
                "zh-Hant",
                "復原詞組",
                "網路手續費",
                "確認次數",
                "同步",
                "金額 + 手續費",
                "確認次數",
            ),
        ];
        let recovery_keys = [
            "nexawal by Nexatrode LLC is a self-custodial interface for managing digital assets. You hold exclusive responsibility for your private keys and 25-word seed phrase.",
            "This will replace the existing wallet on this device.\n\nIf you continue, the currently stored mnemonic and scan state will be removed. Make sure you have your mnemonic backed up before proceeding.",
            "Generate new seed",
            "I wrote down my recovery seed",
            "Couldn't generate a seed: %@",
            "Confirm you wrote it down: enter the requested words below.",
            "Mnemonic (paste):",
            "Mnemonic cannot be empty.",
            "Rescan failed: mnemonic is unavailable.",
            "Stored mnemonic could not be found.",
            "Paste 25-word mnemonic…",
            "Your recovery seed:",
            "Write these words down in order and store them somewhere safe offline. You'll need them to restore the wallet if this phone is lost. NexaWal also keeps an encrypted copy on device for unlock — treat the backup like cash.",
            "Terms accepted. Paste your seed, then Open & sync.",
            "Seed word",
            "Copy seed",
            "Create seed",
            "Paste clipboard into seed",
            "Seed copied. Store it offline, then clear the clipboard.",
            "Seed field is empty.",
            "Seed field is empty. Paste or type the 25-word phrase first.",
            "Seed phrase",
            "Stored metadata does not contain a mnemonic",
            "Click the seed box, paste with ⌘V, set restore height, then Open & sync.",
            "Check that you wrote the seed down and confirm the three words before opening.",
            "Write this seed down on paper. Open is locked until you confirm three words.",
            "Could not generate seed: {0}",
            "Seed field has {0} words. Set restore height, then Open & sync.",
            "Your recovery seed is stored in {0}. Open it with the same wallet screens, settings, history, and local scan cache.",
            "Open the existing wallet from {}, or restore a different seed.",
            "Use Open existing wallet to try again, or restore from seed.",
        ];
        let send_max_keys = [
            "Send maximum spendable after fee to %@.\nPreview amount: %@\nEstimated fee: %@\nFinal amount is recalculated at send time.",
            "Send Max mode: Confirm will sweep all unlocked after fee.",
            "Max preview ready. Confirm Send Max will sweep all unlocked after fee.",
        ];

        for (
            locale,
            recovery_term,
            fee_term,
            confirmation_label,
            sync_term,
            debit_term,
            confirmation_title,
        ) in locales
        {
            let technical_recovery_term = if locale == "vi" {
                "mnemonic"
            } else if locale == "zh-Hans" {
                "助记词"
            } else {
                "助記詞"
            };
            let literal_seed_term = if locale == "vi" {
                "hạt giống"
            } else if locale == "zh-Hans" {
                "种子"
            } else {
                "種子"
            };
            for key in recovery_keys {
                let value = data[locale][key].as_str().unwrap_or_default();
                assert!(
                    value.to_lowercase().contains(&recovery_term.to_lowercase()),
                    "recovery phrase missing in {locale}: {key}"
                );
                assert!(
                    !value
                        .to_lowercase()
                        .contains(&technical_recovery_term.to_lowercase()),
                    "technical recovery wording in {locale}: {key}"
                );
                assert!(
                    !value.contains(literal_seed_term),
                    "literal seed wording in {locale}: {key}"
                );
            }
            for key in send_max_keys {
                let value = data[locale][key].as_str().unwrap();
                assert!(
                    value.to_lowercase().contains(&fee_term.to_lowercase()),
                    "fee deduction missing in {locale}: {key}"
                );
                if key.contains("%@") {
                    assert_eq!(
                        value.matches("%@").count(),
                        3,
                        "send preview placeholders changed in {locale}"
                    );
                }
            }
            assert!(
                data[locale]["Wallet debit (amount + fee)"]
                    .as_str()
                    .unwrap()
                    .contains(debit_term)
            );
            assert!(
                data[locale]["%llu confirmations"]
                    .as_str()
                    .unwrap()
                    .contains("%llu")
            );
            let confirmations = data[locale]["{0} confirmations"].as_str().unwrap();
            assert!(confirmations.contains("{0}") && confirmations.contains(confirmation_label));
            assert!(
                data[locale]["Seed field has {0} words. Set restore height, then Open & sync."]
                    .as_str()
                    .unwrap()
                    .contains("{0}")
            );
            let copy_amount_term = if locale == "vi" {
                "sao chép"
            } else if locale == "zh-Hans" {
                "复制"
            } else {
                "複製"
            };
            assert!(
                data[locale]["Copy amount"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains(&copy_amount_term.to_lowercase())
            );

            let hybrid = data[locale]["Both (scan clearnet, broadcast I2P)"]
                .as_str()
                .unwrap();
            assert!(
                hybrid.contains("Clearnet")
                    && hybrid.contains("I2P")
                    && hybrid.to_lowercase().contains(&sync_term.to_lowercase())
            );
            assert!(
                data[locale]["Confirmations"]
                    .as_str()
                    .unwrap()
                    .contains(confirmation_title)
            );
            assert!(
                data[locale]["Amount is the total wallet debit, including the network fee."]
                    .as_str()
                    .unwrap()
                    .contains(fee_term)
            );
            assert_ne!(data[locale]["Mainnet"].as_str().unwrap(), "Mainnet");

            let privacy_key = "MIT-licensed, unaudited software. You are responsible for your seed and funds. The default remote node can see your IP and wallet sync queries — run your own node for stronger privacy. Optional fiat estimates, if enabled, contact rates.nexawal.com for rates.";
            let rates_key = "Optional. When enabled, NexaWal requests an XMR rate for your selected currency from rates.nexawal.com over HTTPS. This request uses clearnet even when the wallet node uses I2P. Nexatrode can see your IP address and selected currency; wallet amounts, addresses, seeds, and transaction history are not sent. Estimates only — XMR is what you send and hold.";
            let privacy = data[locale][privacy_key].as_str().unwrap();
            let rates = data[locale][rates_key].as_str().unwrap();
            assert!(privacy.contains("rates.nexawal.com") && privacy.contains("IP"));
            assert!(privacy.contains(recovery_term));
            assert!(
                rates.contains("rates.nexawal.com")
                    && rates.contains("I2P")
                    && rates.contains(recovery_term)
            );
            assert!(!rates.contains(technical_recovery_term));
            assert!(!rates.contains(literal_seed_term));
        }
    }
}
