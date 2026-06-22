use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    #[serde(default)]
    pub log_name: String,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub insert_id: String,
    #[serde(default)]
    pub text_payload: Option<String>,
    #[serde(default)]
    pub json_payload: Option<Value>,
    #[serde(default)]
    pub proto_payload: Option<Value>,
    pub resource: Option<Resource>,
    #[serde(default)]
    pub labels: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub trace: Option<String>,
    #[serde(default)]
    pub span_id: Option<String>,
    #[serde(default)]
    pub http_request: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Resource {
    #[serde(rename = "type")]
    pub resource_type: String,
    pub labels: std::collections::HashMap<String, String>,
}

impl LogEntry {
    pub fn display_time(&self) -> String {
        self.timestamp
            .as_deref()
            .and_then(|t| {
                chrono::DateTime::parse_from_rfc3339(t)
                    .ok()
                    .map(|dt| dt.format("%H:%M:%S%.3f").to_string())
            })
            .unwrap_or_else(|| "--:--:--".into())
    }

    pub fn severity_num(&self) -> usize {
        match self.severity.to_uppercase().as_str() {
            "DEFAULT" => 0,
            "DEBUG" => 1,
            "INFO" => 2,
            "NOTICE" => 3,
            "WARNING" => 4,
            "ERROR" => 5,
            "CRITICAL" => 6,
            "ALERT" => 7,
            "EMERGENCY" => 8,
            _ => 0,
        }
    }

    pub fn display_summary(&self) -> String {
        let msg = self
            .text_payload
            .as_deref()
            .or_else(|| {
                self.json_payload
                    .as_ref()
                    .and_then(|v| v.get("message").and_then(|m| m.as_str()))
            })
            .or_else(|| {
                self.proto_payload
                    .as_ref()
                    .and_then(|v| v.get("message").and_then(|m| m.as_str()))
            })
            .unwrap_or("(no payload)")
            .to_string();

        let resource_type = self
            .resource
            .as_ref()
            .map(|r| r.resource_type.as_str())
            .unwrap_or("unknown");

        format!("{resource_type}  {msg}")
    }

    pub fn summary_line(&self) -> String {
        let time = self.display_time();
        let sev = format!("{:9}", self.severity);
        let summary = self.display_summary();
        let max_len = 120usize.saturating_sub(time.len() + sev.len() + 2);
        let summary = truncate_for_display(&summary, max_len);
        format!("{time} {sev} {summary}")
    }

    /// Returns the full text of the currently displayed payload.
    /// Priority matches the detail renderer: textPayload, jsonPayload, protoPayload.
    pub fn payload_text(&self) -> Option<String> {
        if let Some(text) = &self.text_payload {
            return Some(text.clone());
        }
        if let Some(json) = &self.json_payload {
            return serde_json::to_string(json).ok();
        }
        if let Some(proto) = &self.proto_payload {
            return serde_json::to_string(proto).ok();
        }
        None
    }
}

fn truncate_for_display(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let mut chars = input.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        let visible = max_chars.saturating_sub(1);
        let shortened: String = input.chars().take(visible).collect();
        format!("{shortened}…")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_num() {
        let e = LogEntry {
            severity: "ERROR".into(),
            ..Default::default()
        };
        assert_eq!(e.severity_num(), 5);
    }

    #[test]
    fn test_display_time() {
        let e = LogEntry {
            timestamp: Some("2026-06-02T12:34:56.789Z".into()),
            ..Default::default()
        };
        assert_eq!(e.display_time(), "12:34:56.789");
    }

    #[test]
    fn test_display_summary_text() {
        let e = LogEntry {
            text_payload: Some("hello world".into()),
            resource: Some(Resource {
                resource_type: "gce_instance".into(),
                labels: std::collections::HashMap::new(),
            }),
            ..Default::default()
        };
        assert_eq!(e.display_summary(), "gce_instance  hello world");
    }

    #[test]
    fn summary_line_truncates_utf8_safely() {
        let e = LogEntry {
            severity: "INFO".into(),
            text_payload: Some("🙂".repeat(200)),
            resource: Some(Resource {
                resource_type: "gce_instance".into(),
                labels: std::collections::HashMap::new(),
            }),
            ..Default::default()
        };

        let line = e.summary_line();

        assert!(line.ends_with('…'));
        assert!(line.is_char_boundary(line.len()));
    }

    #[test]
    fn deserialize_camelcase_json_payload_with_message() {
        let json = r#"{
            "logName": "projects/foo/logs/bar",
            "insertId": "abc123",
            "severity": "ERROR",
            "jsonPayload": {"message": "hello from json", "extra": 42},
            "resource": {"type": "gce_instance", "labels": {"instance_id": "123"}}
        }"#;
        let entry: LogEntry = serde_json::from_str(json).expect("deserialize LogEntry");
        assert_eq!(entry.log_name, "projects/foo/logs/bar");
        assert_eq!(entry.insert_id, "abc123");
        assert_eq!(entry.severity, "ERROR");
        assert_eq!(
            entry.json_payload.as_ref().unwrap()["message"]
                .as_str()
                .unwrap(),
            "hello from json"
        );
        assert_eq!(entry.display_summary(), "gce_instance  hello from json");
    }

    #[test]
    fn deserialize_camelcase_json_payload_without_message() {
        let json = r#"{
            "logName": "projects/foo/logs/bar",
            "insertId": "abc123",
            "severity": "INFO",
            "jsonPayload": {"foo": "bar"},
            "resource": {"type": "gce_instance", "labels": {}}
        }"#;
        let entry: LogEntry = serde_json::from_str(json).expect("deserialize LogEntry");
        assert_eq!(entry.display_summary(), "gce_instance  (no payload)");
    }

    #[test]
    fn deserialize_camelcase_text_payload() {
        let json = r#"{
            "logName": "projects/foo/logs/bar",
            "insertId": "abc123",
            "severity": "WARNING",
            "textPayload": "hello from text",
            "resource": {"type": "gce_instance", "labels": {}}
        }"#;
        let entry: LogEntry = serde_json::from_str(json).expect("deserialize LogEntry");
        assert_eq!(entry.text_payload.as_deref().unwrap(), "hello from text");
        assert_eq!(entry.display_summary(), "gce_instance  hello from text");
    }

    #[test]
    fn payload_text_prioritizes_text_then_json_then_proto() {
        let text_entry = LogEntry {
            text_payload: Some("text only".into()),
            json_payload: Some(serde_json::json!({"msg": "json"})),
            proto_payload: Some(serde_json::json!({"msg": "proto"})),
            ..Default::default()
        };
        assert_eq!(text_entry.payload_text().unwrap(), "text only");

        let json_entry = LogEntry {
            text_payload: None,
            json_payload: Some(serde_json::json!({"msg": "json"})),
            proto_payload: Some(serde_json::json!({"msg": "proto"})),
            ..Default::default()
        };
        assert_eq!(json_entry.payload_text().unwrap(), r#"{"msg":"json"}"#);

        let proto_entry = LogEntry {
            text_payload: None,
            json_payload: None,
            proto_payload: Some(serde_json::json!({"msg": "proto"})),
            ..Default::default()
        };
        assert_eq!(proto_entry.payload_text().unwrap(), r#"{"msg":"proto"}"#);

        let empty = LogEntry::default();
        assert!(empty.payload_text().is_none());
    }
}

impl Default for LogEntry {
    fn default() -> Self {
        Self {
            log_name: String::new(),
            timestamp: None,
            severity: String::new(),
            insert_id: String::new(),
            text_payload: None,
            json_payload: None,
            proto_payload: None,
            resource: None,
            labels: std::collections::HashMap::new(),
            trace: None,
            span_id: None,
            http_request: None,
        }
    }
}
