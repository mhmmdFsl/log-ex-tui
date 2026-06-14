use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub filters: HashMap<String, SavedFilter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_searches: Vec<String>,
}

fn default_severities() -> [bool; 9] {
    [false, false, true, true, true, true, true, true, true]
}

fn default_time_range() -> String {
    "1h".to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SavedFilter {
    pub name: String,
    #[serde(default, alias = "filter", skip_serializing_if = "Option::is_none")]
    pub legacy_filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_query: Option<String>,
    #[serde(default = "default_severities")]
    pub severities: [bool; 9],
    #[serde(default = "default_time_range")]
    pub time_range: String,
}

impl SavedFilter {
    pub fn describe(&self) -> String {
        if let Some(raw) = self.raw_query.as_deref() {
            return raw.to_string();
        }

        if let Some(legacy) = self.legacy_filter.as_deref() {
            return legacy.to_string();
        }

        self.free_text.clone().unwrap_or_default()
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read_to_string(&path)?;
        let config: Self = serde_json::from_str(&data)?;
        Ok(config)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, data)?;
        Ok(())
    }

    fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("log-ex-tui/config.json")
    }
}

#[cfg(test)]
mod tests {
    use super::SavedFilter;

    #[test]
    fn legacy_filter_field_deserializes() {
        let saved: SavedFilter =
            serde_json::from_str(r#"{"name":"legacy","filter":"severity=\"ERROR\""}"#)
                .expect("legacy filter should parse");

        assert_eq!(saved.legacy_filter.as_deref(), Some("severity=\"ERROR\""));
        assert_eq!(saved.time_range, "1h");
        assert_eq!(
            saved.severities,
            [false, false, true, true, true, true, true, true, true]
        );
    }
}
