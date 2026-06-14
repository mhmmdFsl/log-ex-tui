use serde::Deserialize;

use super::client::{Client, GcpError};

const RM_API: &str = "https://cloudresourcemanager.googleapis.com/v1/projects";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectList {
    projects: Option<Vec<Project>>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    project_id: String,
    name: Option<String>,
    lifecycle_state: Option<String>,
}

impl Client {
    pub async fn list_projects(&self) -> Result<Vec<String>, GcpError> {
        let mut all_projects = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut url = format!("{RM_API}?filter=lifecycleState:ACTIVE&pageSize=200");
            if let Some(ref token) = page_token {
                url.push_str(&format!("&pageToken={}", token));
            }

            let resp: ProjectList = self.get_json(&url).await?;
            if let Some(projects) = resp.projects {
                for p in projects {
                    all_projects.push(p.project_id);
                }
            }

            match resp.next_page_token {
                Some(token) if !token.is_empty() => page_token = Some(token),
                _ => break,
            }
        }

        all_projects.sort();
        Ok(all_projects)
    }
}
