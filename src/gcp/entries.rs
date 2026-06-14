use serde::{Deserialize, Serialize};

use crate::model::LogEntry;

use super::client::{Client, GcpError};

const LOGGING_API: &str = "https://logging.googleapis.com/v2/entries:list";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ListRequest {
    resource_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    order_by: Option<String>,
    page_size: u32,
}

#[derive(Deserialize)]
struct ListResponse {
    entries: Option<Vec<LogEntry>>,
}

impl Client {
    pub async fn list_entries(
        &self,
        project: &str,
        filter: Option<&str>,
        page_size: u32,
    ) -> Result<Vec<LogEntry>, GcpError> {
        let req = ListRequest {
            resource_names: vec![format!("projects/{project}")],
            filter: filter.map(|s| s.to_string()),
            order_by: Some("timestamp desc".into()),
            page_size,
        };

        let resp: ListResponse = self.post_json(LOGGING_API, &req).await?;
        Ok(resp.entries.unwrap_or_default())
    }
}
