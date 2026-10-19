//! Validation for the optional, bounded public details page. No wallet dependency.
use crate::explorer::{Failure, valid_id};
use serde_json::Value;

#[derive(Debug)]
pub struct Input {
    pub index: u64,
    pub key_image: String,
    pub ring_size: u64,
    pub indices: Vec<String>,
    pub truncated: bool,
}
#[derive(Debug)]
pub struct Output {
    pub index: u64,
    pub key: String,
    pub amount: Option<u64>,
    pub view_tag: Option<u64>,
    pub global_index: Option<String>,
}
#[derive(Debug)]
pub struct Advanced {
    pub page: u64,
    pub page_count: u64,
    pub version: u64,
    pub coinbase: bool,
    pub serialized_size: Option<u64>,
    pub downloaded_size: u64,
    pub weight: Option<u64>,
    pub unlock_kind: String,
    pub unlock_value: String,
    pub input_count: u64,
    pub output_count: u64,
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
    pub extra: String,
    pub extra_truncated: bool,
    pub received_timestamp: Option<u64>,
}

pub fn unsigned(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 20 || !s.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}
fn number(v: &Value, key: &str) -> Result<u64, Failure> {
    v[key].as_u64().ok_or(Failure::Unavailable)
}
fn string(v: &Value, key: &str) -> Result<String, Failure> {
    v[key]
        .as_str()
        .map(str::to_owned)
        .ok_or(Failure::Unavailable)
}
fn flag(v: &Value, key: &str) -> Result<bool, Failure> {
    v[key].as_bool().ok_or(Failure::Unavailable)
}
fn optional_number(v: &Value, key: &str) -> Result<Option<u64>, Failure> {
    if v.get(key).is_some_and(Value::is_null) {
        Ok(None)
    } else {
        number(v, key).map(Some)
    }
}
fn optional_string(v: &Value, key: &str) -> Result<Option<String>, Failure> {
    if v.get(key).is_some_and(Value::is_null) {
        Ok(None)
    } else {
        string(v, key).map(Some)
    }
}
fn check(ok: bool) -> Result<(), Failure> {
    if ok {
        Ok(())
    } else {
        Err(Failure::Unavailable)
    }
}

impl Advanced {
    pub fn parse(v: &Value, requested: u64, now: u64, pending: bool) -> Result<Self, Failure> {
        let page = number(v, "page")?;
        let input_count = number(v, "input_count")?;
        let output_count = number(v, "output_count")?;
        check(
            page == requested
                && (1..=4096).contains(&input_count)
                && (1..=4096).contains(&output_count),
        )?;
        let page_count = number(v, "page_count")?;
        check(
            number(v, "page_size")? == 10
                && page_count == input_count.max(output_count).div_ceil(10)
                && page < page_count,
        )?;
        let version = number(v, "version")?;
        let coinbase = flag(v, "coinbase")?;
        check((version == 1 || version == 2) && (!coinbase || input_count == 1))?;
        let rows = v["inputs"].as_array().ok_or(Failure::Unavailable)?;
        let outs = v["outputs"].as_array().ok_or(Failure::Unavailable)?;
        let start = page * 10;
        check(
            rows.len() as u64
                == (if coinbase { 0 } else { input_count })
                    .saturating_sub(start)
                    .min(10)
                && outs.len() as u64 == output_count.saturating_sub(start).min(10),
        )?;
        let mut inputs = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let key_image = string(row, "key_image")?;
            let ring_size = number(row, "ring_size")?;
            let truncated = flag(row, "indices_truncated")?;
            let indices = row["absolute_indices"]
                .as_array()
                .ok_or(Failure::Unavailable)?;
            check(
                number(row, "index")? == start + i as u64
                    && valid_id(&key_image)
                    && (1..=1_048_576).contains(&ring_size)
                    && indices.len() as u64 == ring_size.min(128)
                    && truncated == (ring_size > 128),
            )?;
            let mut values = Vec::new();
            let mut previous = None;
            for index in indices {
                let raw = index.as_str().ok_or(Failure::Unavailable)?;
                let number = unsigned(raw).ok_or(Failure::Unavailable)?;
                check(previous.is_none_or(|p| number > p))?;
                previous = Some(number);
                values.push(raw.to_string());
            }
            inputs.push(Input {
                index: start + i as u64,
                key_image,
                ring_size,
                indices: values,
                truncated,
            });
        }
        let mut outputs = Vec::new();
        for (i, row) in outs.iter().enumerate() {
            let key = string(row, "key")?;
            let amount = optional_string(row, "public_amount_piconero")?
                .map(|s| unsigned(&s).ok_or(Failure::Unavailable))
                .transpose()?;
            let view_tag = optional_number(row, "view_tag")?;
            let global_index = optional_string(row, "global_index")?;
            check(
                number(row, "index")? == start + i as u64
                    && valid_id(&key)
                    && view_tag.is_none_or(|n| n <= 255)
                    && (amount.is_none() == (version == 2 && !coinbase))
                    && global_index
                        .as_deref()
                        .is_none_or(|s| unsigned(s).is_some()),
            )?;
            outputs.push(Output {
                index: start + i as u64,
                key,
                amount,
                view_tag,
                global_index,
            });
        }
        let downloaded_size = number(v, "downloaded_size_bytes")?;
        let serialized_size = optional_number(v, "serialized_size_bytes")?;
        let weight = optional_number(v, "weight")?;
        check(
            (1..=1_048_576).contains(&downloaded_size)
                && serialized_size.is_none_or(|n| n == downloaded_size)
                && weight.is_none_or(|n| (1..=4_194_304).contains(&n) && serialized_size.is_some()),
        )?;
        let unlock_kind = string(v, "unlock_kind")?;
        let unlock_value = string(v, "unlock_value")?;
        let unlock = unsigned(&unlock_value).ok_or(Failure::Unavailable)?;
        check(matches!(
            (unlock_kind.as_str(), unlock),
            ("none", 0) | ("height", 1..=499_999_999) | ("timestamp", 500_000_000..)
        ))?;
        let extra = string(v, "extra_hex")?;
        let extra_truncated = flag(v, "extra_truncated")?;
        check(
            extra.len() <= 4096
                && extra.len() % 2 == 0
                && extra
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && (!extra_truncated || extra.len() == 4096),
        )?;
        let received_timestamp = optional_number(v, "received_timestamp")?;
        check(received_timestamp.is_none_or(|t| pending && t > 0 && t <= now.saturating_add(120)))?;
        Ok(Self {
            page,
            page_count,
            version,
            coinbase,
            serialized_size,
            downloaded_size,
            weight,
            unlock_kind,
            unlock_value,
            input_count,
            output_count,
            inputs,
            outputs,
            extra,
            extra_truncated,
            received_timestamp,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::parse_page;
    fn body() -> Value {
        serde_json::from_str(include_str!(
            "../tests/fixtures/explorer_advanced_page0.json"
        ))
        .unwrap()
    }
    fn parse(value: &Value, page: u64) -> Result<crate::explorer::Observation, Failure> {
        parse_page(
            &serde_json::to_vec(value).unwrap(),
            &"a".repeat(64),
            1000,
            Some(page),
        )
    }
    #[test]
    fn shared_fixture_has_exact_fee_conflict_and_bounded_pages() {
        let b = body();
        let value = parse(&b, 0).unwrap();
        assert_eq!(value.fee, Some(u64::MAX));
        assert!(value.double_spend_seen);
        let d = value.details.unwrap();
        assert_eq!(d.inputs.len(), 10);
        assert_eq!(d.outputs.len(), 10);
        assert_eq!(d.inputs[0].indices.len(), 16);
        assert!(d.outputs[0].amount.is_none());
        assert!(d.weight.is_none());
        assert!(parse(&b, 1).is_err());
        assert!(
            parse_page(
                &serde_json::to_vec(&b).unwrap(),
                &"a".repeat(64),
                1000,
                None
            )
            .is_err()
        );
        let mut last = body();
        last["details"]["page"] = 1.into();
        let mut input = last["details"]["inputs"][0].clone();
        input["index"] = 10.into();
        let mut output = last["details"]["outputs"][0].clone();
        output["index"] = 10.into();
        let mut output2 = output.clone();
        output2["index"] = 11.into();
        last["details"]["inputs"] = serde_json::json!([input]);
        last["details"]["outputs"] = serde_json::json!([output, output2]);
        assert_eq!(parse(&last, 1).unwrap().details.unwrap().inputs.len(), 1);
    }
    #[test]
    fn legacy_coinbase_and_truncated_previews_remain_explicit() {
        let mut b = body();
        b["details"]["version"] = 1.into();
        for output in b["details"]["outputs"].as_array_mut().unwrap() {
            output["public_amount_piconero"] = u64::MAX.to_string().into();
        }
        assert_eq!(
            parse(&b, 0).unwrap().details.unwrap().outputs[0].amount,
            Some(u64::MAX)
        );
        b["details"]["coinbase"] = true.into();
        b["details"]["input_count"] = 1.into();
        b["details"]["inputs"] = serde_json::json!([]);
        b["fee_piconero"] = "0".into();
        assert!(parse(&b, 0).unwrap().details.unwrap().inputs.is_empty());
        let mut b = body();
        b["details"]["inputs"][0]["ring_size"] = 200.into();
        b["details"]["inputs"][0]["indices_truncated"] = true.into();
        b["details"]["inputs"][0]["absolute_indices"] =
            serde_json::json!((0..128).map(|n| n.to_string()).collect::<Vec<_>>());
        b["details"]["extra_hex"] = "aa".repeat(2048).into();
        b["details"]["extra_truncated"] = true.into();
        assert!(parse(&b, 0).unwrap().details.unwrap().inputs[0].truncated);
    }
    #[test]
    fn malformed_pages_indices_and_types_fail_closed() {
        for (key, value) in [
            ("page_count", serde_json::json!(1)),
            ("page_size", serde_json::json!(25)),
            ("input_count", serde_json::json!(4097)),
            ("inputs", serde_json::json!([])),
            ("unlock_kind", serde_json::json!("other")),
            ("extra_hex", serde_json::json!("GG")),
            ("weight", serde_json::json!(-1)),
        ] {
            let mut b = body();
            b["details"][key] = value;
            assert!(parse(&b, 0).is_err(), "{key}");
        }
        let mut b = body();
        b["details"]["inputs"][0]["absolute_indices"] = serde_json::json!(vec!["1"; 16]);
        assert!(parse(&b, 0).is_err());
        b = body();
        b["double_spend_seen"] = "false".into();
        assert!(parse(&b, 0).is_err());
        assert!(parse_page(&vec![b' '; 65_537], &"a".repeat(64), 1000, Some(0)).is_err());
    }
}
