use serde::Deserialize;
use serde_json::Value;

pub(crate) fn to_pretty_json<T: serde::Serialize>(value: T) -> Result<String, String> {
    serde_json::to_string_pretty(&value).map_err(|error| error.to_string())
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn io_to_string(error: std::io::Error) -> String {
    error.to_string()
}

pub(crate) fn deserialize_optional_usize<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match value {
        Some(serde_json::Value::Number(n)) => n
            .as_u64()
            .map(|v| Some(v as usize))
            .ok_or_else(|| serde::de::Error::custom("invalid number")),
        Some(serde_json::Value::String(s)) => s
            .parse::<usize>()
            .map(Some)
            .map_err(serde::de::Error::custom),
        None | Some(serde_json::Value::Null) => Ok(None),
        _ => Err(serde::de::Error::custom("expected string or number")),
    }
}

pub(crate) fn from_value<T: for<'de> Deserialize<'de>>(input: &Value) -> Result<T, String> {
    serde_json::from_value(input.clone()).map_err(|error| error.to_string())
}

pub(crate) fn iso8601_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

pub(crate) fn iso8601_timestamp() -> String {
    if let Ok(output) = std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
    {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    iso8601_now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[test]
    fn test_to_pretty_json() {
        let val = json!({ "name": "claw", "version": 1 });
        let pretty = to_pretty_json(&val).unwrap();
        assert!(pretty.contains("\"name\": \"claw\""));
        assert!(pretty.contains("\"version\": 1"));
    }

    #[test]
    fn test_io_to_string() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        assert_eq!(io_to_string(io_err), "file missing");
    }

    #[derive(Deserialize, PartialEq, Debug)]
    struct TargetStruct {
        #[serde(default, deserialize_with = "deserialize_optional_usize")]
        num: Option<usize>,
    }

    #[test]
    fn test_deserialize_optional_usize_variations() {
        let from_num: TargetStruct = serde_json::from_str(r#"{"num": 42}"#).unwrap();
        assert_eq!(from_num.num, Some(42));

        let from_str: TargetStruct = serde_json::from_str(r#"{"num": "100"}"#).unwrap();
        assert_eq!(from_str.num, Some(100));

        let from_null: TargetStruct = serde_json::from_str(r#"{"num": null}"#).unwrap();
        assert_eq!(from_null.num, None);

        let from_missing: TargetStruct = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(from_missing.num, None);

        let err_result: Result<TargetStruct, _> = serde_json::from_str(r#"{"num": "abc"}"#);
        assert!(err_result.is_err());
    }

    #[test]
    fn test_from_value() {
        let val = json!({"num": 50});
        let res: TargetStruct = from_value(&val).unwrap();
        assert_eq!(res.num, Some(50));
    }

    #[test]
    fn test_iso8601_now_and_timestamp() {
        let now = iso8601_now();
        assert!(now.parse::<u64>().is_ok());

        let ts = iso8601_timestamp();
        assert!(!ts.is_empty());
    }
}
