pub struct FilterBuilder {
    severities: Vec<String>,
    log_names: Vec<String>,
    resource_type: Option<String>,
    time_from: Option<String>,
    time_to: Option<String>,
    free_text: Option<String>,
    raw: Option<String>,
}

impl FilterBuilder {
    pub fn new() -> Self {
        Self {
            severities: Vec::new(),
            log_names: Vec::new(),
            resource_type: None,
            time_from: None,
            time_to: None,
            free_text: None,
            raw: None,
        }
    }

    pub fn raw(&mut self, filter: &str) -> &mut Self {
        self.raw = Some(filter.to_string());
        self
    }

    pub fn severity(&mut self, sevs: &[bool]) -> &mut Self {
        let names = [
            "DEFAULT",
            "DEBUG",
            "INFO",
            "NOTICE",
            "WARNING",
            "ERROR",
            "CRITICAL",
            "ALERT",
            "EMERGENCY",
        ];
        self.severities = sevs
            .iter()
            .enumerate()
            .filter(|(_, &on)| on)
            .map(|(i, _)| names[i].to_string())
            .collect();
        self
    }

    pub fn build(&self) -> String {
        if let Some(raw) = &self.raw {
            return raw.clone();
        }

        let mut parts: Vec<String> = Vec::new();

        if !self.severities.is_empty() {
            parts.push(format!(
                "severity=({})",
                self.severities.join(" OR severity=")
            ));
        }

        for log_name in &self.log_names {
            parts.push(format!("logName=\"{log_name}\""));
        }

        if let Some(rt) = &self.resource_type {
            parts.push(format!("resource.type=\"{rt}\""));
        }

        if let Some(text) = &self.free_text {
            if !text.is_empty() {
                let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
                parts.push(format!("textPayload:\"{escaped}\""));
            }
        }

        parts.join(" AND ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_filter() {
        let mut b = FilterBuilder::new();
        b.raw("severity>=ERROR");
        assert_eq!(b.build(), "severity>=ERROR");
    }

    #[test]
    fn test_severity_single() {
        let mut b = FilterBuilder::new();
        b.severity(&[false, false, true, false, false, false, false, false, false]);
        assert_eq!(b.build(), "severity=(INFO)");
    }

    #[test]
    fn test_severity_multiple() {
        let mut b = FilterBuilder::new();
        b.severity(&[false, false, false, false, true, true, false, false, false]);
        assert_eq!(b.build(), "severity=(WARNING OR severity=ERROR)");
    }

    #[test]
    fn test_combined() {
        let mut b = FilterBuilder::new();
        b.severity(&[false, false, true, false, false, false, false, false, false])
            .raw("resource.type=\"k8s_container\"");
        // raw overrides everything
        assert_eq!(b.build(), "resource.type=\"k8s_container\"");
    }
}
