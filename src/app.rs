use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};
use ratatui::Frame;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::gcp;
use crate::model::LogEntry;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Facets,
    List,
    Detail,
}

#[derive(Debug, Clone)]
pub enum Message {
    Key {
        code: KeyCode,
        modifiers: KeyModifiers,
    },
    Tick,
    Resize,
    GcpReady(gcp::Client),
    AuthError(String),
    Error(String),
    ProjectsLoaded(Vec<String>),
    EntriesFetched(Vec<LogEntry>),
    TailEntries(Vec<LogEntry>),
}

#[derive(Debug, Default)]
pub struct CommandPalette {
    pub active: bool,
    pub input: String,
    pub matches: Vec<String>,
    pub selected_match: Option<usize>,
}

#[derive(Debug, Default)]
pub struct FilterState {
    pub severities: [bool; 9],
    pub free_text: String,
    pub raw_query: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimeRange {
    FiveM,
    FifteenM,
    OneH,
    SixH,
    TwentyFourH,
}

impl Default for TimeRange {
    fn default() -> Self {
        Self::OneH
    }
}

const SEVERITY_NAMES: [&str; 9] = [
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

fn is_field_query(text: &str) -> bool {
    // Raw operators already indicate a query
    if text.contains('=') || text.contains(':') || text.contains('>') || text.contains('<') {
        return true;
    }
    // Known GCP field path prefixes
    let prefixes = [
        "resource.",
        "jsonPayload.",
        "protoPayload.",
        "labels.",
        "logName",
        "severity",
        "timestamp",
        "trace",
        "spanId",
        "httpRequest.",
        "operation.",
        "sourceLocation.",
    ];
    let lower = text.to_lowercase();
    prefixes
        .iter()
        .any(|p| lower.starts_with(p) || lower.contains(p))
}
enum FilterSuggestion {
    Field {
        label: &'static str,
        query: &'static str,
    },
    Operator {
        label: &'static str,
        snippet: &'static str,
    },
}

const SUGGESTIONS: &[FilterSuggestion] = &[
    FilterSuggestion::Field {
        label: "resource.labels.container_name",
        query: "resource.labels.container_name=",
    },
    FilterSuggestion::Field {
        label: "resource.labels.pod_name",
        query: "resource.labels.pod_name=",
    },
    FilterSuggestion::Field {
        label: "jsonPayload.message",
        query: "jsonPayload.message=",
    },
    FilterSuggestion::Field {
        label: "jsonPayload.level",
        query: "jsonPayload.level=",
    },
    FilterSuggestion::Field {
        label: "protoPayload.serviceName",
        query: "protoPayload.serviceName=",
    },
    FilterSuggestion::Field {
        label: "logName",
        query: "logName=\"projects/*/logs/\"",
    },
    FilterSuggestion::Field {
        label: "resource.type",
        query: "resource.type=\"\"",
    },
    FilterSuggestion::Operator {
        label: "= (equals)",
        snippet: "=",
    },
    FilterSuggestion::Operator {
        label: "!= (not equals)",
        snippet: "!=",
    },
    FilterSuggestion::Operator {
        label: ": (contains)",
        snippet: ":",
    },
    FilterSuggestion::Operator {
        label: "> (greater than)",
        snippet: ">",
    },
];

pub struct App {
    pub running: bool,
    pub focus: Focus,
    pub project: Option<String>,
    pub gcp: Option<gcp::Client>,
    pub command_tx: mpsc::UnboundedSender<Message>,
    pub projects: Vec<String>,
    pub entries: Vec<LogEntry>,
    pub selected: usize,
    pub detail_scroll: usize,
    pub tail_on: bool,
    pub status: String,
    pub error: Option<String>,

    pub show_help: bool,
    pub show_project_picker: bool,
    pub picker_state: ListState,
    pub palette: CommandPalette,
    pub filter: FilterState,
    pub time_range: TimeRange,
    pub config: Config,
}

impl App {
    pub fn new(command_tx: mpsc::UnboundedSender<Message>) -> Self {
        let mut severities = [true; 9];
        severities[0] = false;
        severities[1] = false;

        let mut picker_state = ListState::default();
        picker_state.select(Some(0));

        Self {
            running: true,
            focus: Focus::List,
            project: None,
            gcp: None,
            command_tx,
            projects: Vec::new(),
            entries: Vec::new(),
            selected: 0,
            detail_scroll: 0,
            tail_on: false,
            status: String::new(),
            error: None,
            show_help: false,
            show_project_picker: false,
            picker_state,
            palette: CommandPalette::default(),
            filter: FilterState {
                severities,
                free_text: String::new(),
                raw_query: None,
            },
            time_range: TimeRange::default(),
            config: Config::load().unwrap_or_default(),
        }
    }

    pub fn init_project(&mut self, project: String) {
        self.project = Some(project);
        self.maybe_fetch_entries();
    }

    pub fn update(&mut self, msg: Message) {
        match msg {
            Message::Key { code, modifiers } => self.handle_key(code, modifiers),
            Message::Tick => {}
            Message::Resize => {}
            Message::GcpReady(client) => {
                self.gcp = Some(client);
                self.maybe_fetch_entries();
            }
            Message::AuthError(e) => {
                self.error = Some(e);
                if self.projects.is_empty() {
                    self.projects
                        .push("(run `gcloud auth application-default login`)".into());
                    self.show_project_picker = true;
                    self.picker_state.select(Some(0));
                }
            }
            Message::Error(e) => {
                self.error = Some(e);
            }
            Message::ProjectsLoaded(list) => {
                self.projects = list;
                if self.project.is_none() && !self.projects.is_empty() {
                    self.show_project_picker = true;
                    self.picker_state.select(Some(0));
                }
            }
            Message::EntriesFetched(entries) => {
                self.entries = entries;
                self.selected = self.selected.min(self.entries.len().saturating_sub(1));
                self.detail_scroll = 0;
                self.error = None;
            }
            Message::TailEntries(new_entries) => {
                // Prepend new entries (newest first), dedupe by insertId
                let mut added = 0usize;
                for entry in new_entries {
                    if !self.entries.iter().any(|e| e.insert_id == entry.insert_id) {
                        self.entries.insert(0, entry);
                        added += 1;
                    }
                }
                // Cap at 10k entries
                const MAX: usize = 10000;
                if self.entries.len() > MAX {
                    let remove = self.entries.len() - MAX;
                    self.entries.truncate(MAX);
                    if self.selected >= MAX - remove {
                        self.selected = self.selected.saturating_sub(remove);
                    }
                }
                // Keep selected on same logical entry if possible
                if self.selected > 0 {
                    self.selected += added;
                    self.selected = self.selected.min(self.entries.len().saturating_sub(1));
                }
                self.detail_scroll = 0;
            }
        }
    }

    fn maybe_fetch_entries(&mut self) {
        let gcp = match &self.gcp {
            Some(g) => g.clone(),
            None => return,
        };
        let project = match &self.project {
            Some(p) => p.clone(),
            None => return,
        };
        self.request_entry_fetch(gcp, project);
    }

    fn request_entry_fetch(&self, gcp: gcp::Client, project: String) {
        let filter_str = self.build_filter_string();
        let tx = self.command_tx.clone();

        tokio::spawn(async move {
            match gcp.list_entries(&project, filter_str.as_deref(), 200).await {
                Ok(entries) => {
                    let _ = tx.send(Message::EntriesFetched(entries));
                }
                Err(e) => {
                    let _ = tx.send(Message::Error(e.display_message()));
                }
            }
        });
    }

    fn validate_filter_query(text: &str) -> Option<String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Some("Filter query is empty".into());
        }
        if trimmed.ends_with('=')
            || trimmed.ends_with(':')
            || trimmed.ends_with('>')
            || trimmed.ends_with('<')
        {
            return Some(format!(
                "Incomplete query: '{}' is missing a value after the operator",
                trimmed
            ));
        }
        if trimmed.len() == 1 && "=:<><".contains(trimmed) {
            return Some(format!(
                "Invalid query: '{}' is not a valid filter",
                trimmed
            ));
        }
        None
    }
    pub fn build_filter_string(&self) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();

        // Time range
        let now = chrono::Utc::now();
        let since = match self.time_range {
            TimeRange::FiveM => now - chrono::Duration::minutes(5),
            TimeRange::FifteenM => now - chrono::Duration::minutes(15),
            TimeRange::OneH => now - chrono::Duration::hours(1),
            TimeRange::SixH => now - chrono::Duration::hours(6),
            TimeRange::TwentyFourH => now - chrono::Duration::hours(24),
        };
        parts.push(format!("timestamp>=\"{}\"", since.to_rfc3339()));

        let active_sevs: Vec<&str> = self
            .filter
            .severities
            .iter()
            .enumerate()
            .filter(|(_, &on)| on)
            .map(|(i, _)| SEVERITY_NAMES[i])
            .collect();

        if !active_sevs.is_empty() {
            if active_sevs.len() == 1 {
                parts.push(format!("severity=\"{}\"", active_sevs[0]));
            } else {
                let ors: Vec<String> = active_sevs
                    .iter()
                    .map(|s| format!("severity=\"{s}\""))
                    .collect();
                parts.push(format!("({})", ors.join(" OR ")));
            }
        }

        if let Some(raw) = self.filter.raw_query.as_deref() {
            if !raw.is_empty() {
                parts.push(raw.to_string());
            }
        } else if !self.filter.free_text.is_empty() {
            let text = &self.filter.free_text;
            if is_field_query(text) {
                parts.push(text.to_string());
            } else {
                let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
                parts.push(format!("textPayload:\"{escaped}\""));
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" AND "))
        }
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        if self.show_help {
            if matches!(code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')) {
                self.show_help = false;
            }
            return;
        }

        if self.show_project_picker {
            match code {
                KeyCode::Esc | KeyCode::Char('q') => self.show_project_picker = false,
                KeyCode::Enter | KeyCode::Char('l') => {
                    if let Some(idx) = self.picker_state.selected() {
                        if idx < self.projects.len() {
                            let p = self.projects[idx].clone();
                            if !p.starts_with('(') {
                                self.project = Some(p);
                                self.show_project_picker = false;
                                self.error = None;
                                self.maybe_fetch_entries();
                            }
                        }
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let sel = self.picker_state.selected().unwrap_or(0).saturating_sub(1);
                    self.picker_state.select(Some(sel));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let sel = self
                        .picker_state
                        .selected()
                        .unwrap_or(0)
                        .saturating_add(1)
                        .min(self.projects.len().saturating_sub(1));
                    self.picker_state.select(Some(sel));
                }
                _ => {}
            }
            return;
        }

        if self.focus == Focus::Detail {
            match (code, modifiers) {
                (KeyCode::Char('j'), _) | (KeyCode::Down, _) => {
                    self.detail_scroll = self.detail_scroll.saturating_add(1);
                    return;
                }
                (KeyCode::Char('k'), _) | (KeyCode::Up, _) => {
                    self.detail_scroll = self.detail_scroll.saturating_sub(1);
                    return;
                }
                (KeyCode::Char('g'), KeyModifiers::NONE) => {
                    self.detail_scroll = 0;
                    return;
                }
                (KeyCode::Char('G'), _) => {
                    self.detail_scroll = usize::MAX;
                    return;
                }
                (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                    self.detail_scroll = self.detail_scroll.saturating_add(10);
                    return;
                }
                (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                    self.detail_scroll = self.detail_scroll.saturating_sub(10);
                    return;
                }
                _ => {}
            }
        }

        if self.palette.active {
            match code {
                KeyCode::Esc => {
                    self.palette.active = false;
                    self.palette.input.clear();
                    self.palette.matches.clear();
                    self.palette.selected_match = None;
                }
                KeyCode::Enter => {
                    if let Some(i) = self.palette.selected_match {
                        if i < self.palette.matches.len() {
                            let selected = self.palette.matches[i].clone();
                            if self.palette.input.starts_with('/') {
                                let cmd = selected
                                    .split(" = ")
                                    .nth(1)
                                    .unwrap_or(&selected)
                                    .to_string();
                                if let Some(err) = Self::validate_filter_query(&cmd) {
                                    self.status = err;
                                    self.palette.active = false;
                                    self.palette.input.clear();
                                    self.palette.matches.clear();
                                    self.palette.selected_match = None;
                                    return;
                                }
                                self.palette.input = cmd.clone();
                                self.palette.selected_match = None;
                                self.palette.matches.clear();
                                self.palette.active = false;
                                self.execute_command(&cmd);
                                return;
                            }
                            self.palette.input = selected.clone();
                            self.palette.selected_match = None;
                            self.palette.matches.clear();
                            self.palette.active = false;
                            self.execute_command(&selected);
                            return;
                        }
                    }
                    let cmd = std::mem::take(&mut self.palette.input);
                    self.palette.active = false;
                    self.palette.matches.clear();
                    self.palette.selected_match = None;
                    self.execute_command(cmd.trim());
                }
                KeyCode::Up => {
                    let len = self.palette.matches.len();
                    if len > 0 {
                        self.palette.selected_match = Some(
                            self.palette
                                .selected_match
                                .unwrap_or(0)
                                .saturating_sub(1)
                                .max(0),
                        );
                    }
                }
                KeyCode::Down => {
                    let len = self.palette.matches.len();
                    if len > 0 {
                        self.palette.selected_match =
                            Some((self.palette.selected_match.unwrap_or(0) + 1).min(len - 1));
                    }
                }
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    self.palette.input.push(c);
                    self.palette.selected_match = None;
                    self.update_palette_matches();
                }
                KeyCode::Backspace => {
                    self.palette.input.pop();
                    self.palette.selected_match = None;
                    self.update_palette_matches();
                }
                _ => {}
            }
            return;
        }

        match (code, modifiers) {
            (KeyCode::Char('q'), KeyModifiers::NONE) => self.running = false,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => self.running = false,
            (KeyCode::Char(':'), _) => {
                self.palette.active = true;
                self.palette.input.clear();
                self.update_palette_matches();
            }
            (KeyCode::Char('/'), _) => {
                self.palette.active = true;
                self.palette.input = "/".into();
                self.update_palette_matches();
            }
            (KeyCode::Char('?'), _) => self.show_help = !self.show_help,
            (KeyCode::Char('P'), _) => {
                self.show_project_picker = true;
                self.picker_state.select(Some(
                    self.projects
                        .iter()
                        .position(|p| Some(p.as_str()) == self.project.as_deref())
                        .unwrap_or(0),
                ));
            }
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => {
                let max = self.entries.len().saturating_sub(1);
                self.selected = self.selected.saturating_add(1).min(max);
            }
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => {
                self.selected = self.selected.saturating_sub(1);
            }
            (KeyCode::Char('g'), KeyModifiers::NONE) => self.selected = 0,
            (KeyCode::Char('G'), _) => {
                self.selected = self.entries.len().saturating_sub(1);
            }
            (KeyCode::Char('t'), _) => self.tail_on = !self.tail_on,
            (KeyCode::Char('T'), KeyModifiers::SHIFT) => {
                self.time_range = self.time_range.next();
                self.status = format!("time: {}", self.time_range);
                self.maybe_fetch_entries();
            }
            (KeyCode::Tab, _) => {
                self.focus = match self.focus {
                    Focus::Facets => Focus::List,
                    Focus::List => {
                        self.detail_scroll = 0;
                        Focus::Detail
                    }
                    Focus::Detail => Focus::Facets,
                };
            }
            (KeyCode::BackTab, _) => {
                self.focus = match self.focus {
                    Focus::Facets => {
                        self.detail_scroll = 0;
                        Focus::Detail
                    }
                    Focus::List => Focus::Facets,
                    Focus::Detail => Focus::List,
                };
            }
            (KeyCode::Char(c), _) if c.is_ascii_digit() => {
                let idx = c.to_digit(10).unwrap_or(0) as usize;
                if idx < 9 {
                    self.filter.severities[idx] = !self.filter.severities[idx];
                    self.maybe_fetch_entries();
                }
            }
            (KeyCode::Char(')'), _) => {
                self.filter.severities.fill(false);
                self.maybe_fetch_entries();
            }
            (KeyCode::Char('l'), _) | (KeyCode::Enter, _) => {
                if !self.entries.is_empty() {
                    self.detail_scroll = 0;
                    self.focus = Focus::Detail;
                }
            }
            (KeyCode::Esc | KeyCode::Char('h'), _) => {
                self.focus = Focus::List;
            }
            (KeyCode::Char('c'), _) => {
                self.status = "  co: copy (not yet implemented)".into();
            }
            (KeyCode::Char('e'), _) => {
                self.status = "  ex: export (not yet implemented)".into();
            }
            (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
                self.maybe_fetch_entries();
            }
            _ => {}
        }
    }

    fn execute_command(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return;
        }

        if cmd.starts_with('/') {
            let text = cmd[1..].trim().to_string();
            if !text.is_empty() {
                if let Some(err) = Self::validate_filter_query(&text) {
                    self.status = err;
                    return;
                }
                self.filter.free_text = text.clone();
                self.filter.raw_query = None;
                // Track recent search
                self.config.recent_searches.retain(|s| s != &text);
                self.config.recent_searches.insert(0, text);
                self.config.recent_searches.truncate(10);
                let _ = self.config.save();
            }
            self.maybe_fetch_entries();
            return;
        }

        match cmd {
            ":q" | ":quit" => self.running = false,
            ":?" | ":help" => self.show_help = true,
            ":p" | ":project" => {
                self.show_project_picker = true;
                self.picker_state
                    .select(Some(self.picker_state.selected().unwrap_or(0)));
            }
            ":t" | ":tail" => self.tail_on = !self.tail_on,
            ":c" | ":clear" => {
                self.filter.severities = [false, false, true, true, true, true, true, true, true];
                self.filter.free_text.clear();
                self.filter.raw_query = None;
                self.maybe_fetch_entries();
            }
            s if s.starts_with(":sev ") => {
                let levels: Vec<&str> = s[5..].split_whitespace().collect();
                self.filter.severities.fill(false);
                for lvl in levels {
                    match lvl.to_lowercase().as_str() {
                        "default" => self.filter.severities[0] = true,
                        "debug" => self.filter.severities[1] = true,
                        "info" => self.filter.severities[2] = true,
                        "notice" => self.filter.severities[3] = true,
                        "warning" | "warn" => self.filter.severities[4] = true,
                        "error" => self.filter.severities[5] = true,
                        "critical" => self.filter.severities[6] = true,
                        "alert" => self.filter.severities[7] = true,
                        "emergency" | "emerg" => self.filter.severities[8] = true,
                        _ => {}
                    }
                }
                self.maybe_fetch_entries();
            }
            s if s.starts_with(":tm ") || s.starts_with(":time ") => {
                let arg = s.splitn(2, ' ').nth(1).unwrap_or("1h");
                self.time_range = match arg {
                    "5m" => TimeRange::FiveM,
                    "15m" => TimeRange::FifteenM,
                    "1h" => TimeRange::OneH,
                    "6h" => TimeRange::SixH,
                    "24h" => TimeRange::TwentyFourH,
                    _ => TimeRange::OneH,
                };
                self.maybe_fetch_entries();
            }
            ":ab" | ":about" => {
                self.status = format!(
                    "project: {} · tail: {} · entries: {} · filer: {}",
                    self.project.as_deref().unwrap_or("none"),
                    if self.tail_on { "on" } else { "off" },
                    self.entries.len(),
                    self.build_filter_string().unwrap_or_default(),
                );
            }
            s if s.starts_with(":save ") => {
                let rest = s[6..].trim();
                let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                if parts.is_empty() {
                    self.status = "usage: :save <slot> [name]".into();
                    return;
                }
                let slot = parts[0].to_string();
                let name = parts.get(1).unwrap_or(&"saved").to_string();
                let saved = crate::config::SavedFilter {
                    name: name.clone(),
                    legacy_filter: None,
                    free_text: (!self.filter.free_text.is_empty())
                        .then(|| self.filter.free_text.clone()),
                    raw_query: self.filter.raw_query.clone(),
                    severities: self.filter.severities,
                    time_range: self.time_range.as_config_value().to_string(),
                };
                let preview = saved.describe();
                self.config.filters.insert(slot.clone(), saved);
                if let Err(e) = self.config.save() {
                    self.status = format!("save error: {e}");
                } else {
                    self.status = format!("saved {slot} ({name}): {preview}");
                }
            }
            s if s.starts_with(":load ") => {
                let slot = s[6..].trim();
                self.load_saved_filter(slot);
            }
            s if s.starts_with(":f") && s.len() >= 3 => {
                let slot = &s[1..]; // strip leading colon
                self.load_saved_filter(slot);
            }
            _ => {
                self.status = format!("unknown: {cmd}");
            }
        }
    }

    fn load_saved_filter(&mut self, slot: &str) {
        match self.config.filters.get(slot) {
            Some(saved) => {
                self.filter.severities = saved.severities;
                self.time_range = TimeRange::from_config_value(&saved.time_range);
                self.filter.free_text = saved.free_text.clone().unwrap_or_default();
                self.filter.raw_query = saved
                    .raw_query
                    .clone()
                    .or_else(|| saved.legacy_filter.clone());
                self.status = format!("loaded {slot} ({})", saved.name);
                self.maybe_fetch_entries();
            }
            None => {
                self.status = format!("no filter in slot {slot}");
            }
        }
    }

    fn update_palette_matches(&mut self) {
        let input = &self.palette.input;
        let is_filter_mode = input.starts_with('/');
        let query = if is_filter_mode {
            &input[1..]
        } else {
            input.as_str()
        };

        if is_filter_mode {
            // `/` mode: show field suggestions and recent searches
            let mut matches: Vec<String> = Vec::new();
            if query.is_empty() {
                // Show top 8 field suggestions when `/` is empty
                for sug in SUGGESTIONS.iter().take(8) {
                    match sug {
                        FilterSuggestion::Field { label, query } => {
                            matches.push(format!("{} = {}", label, query));
                        }
                        FilterSuggestion::Operator { label, snippet } => {
                            matches.push(format!("{} = {}", label, snippet));
                        }
                    }
                }
            } else {
                // Filter suggestions by label prefix match
                let lower_query = query.to_lowercase();
                for sug in SUGGESTIONS.iter() {
                    let label = match sug {
                        FilterSuggestion::Field { label, .. } => label,
                        FilterSuggestion::Operator { label, .. } => label,
                    };
                    if label.to_lowercase().starts_with(&lower_query)
                        || label.to_lowercase().contains(&lower_query)
                    {
                        let query_str = match sug {
                            FilterSuggestion::Field { query, .. } => *query,
                            FilterSuggestion::Operator { snippet, .. } => *snippet,
                        };
                        matches.push(format!("{} = {}", label, query_str));
                    }
                }
                // Add recent searches
                for recent in self
                    .config
                    .recent_searches
                    .iter()
                    .take(12 - matches.len().min(12))
                {
                    if recent.to_lowercase().contains(&lower_query) || query.is_empty() {
                        matches.push(format!("/ {}", recent));
                    }
                }
            }
            self.palette.matches = matches.into_iter().take(12).collect();
        } else {
            // `:` mode: commands + saved filters
            let mut matches: Vec<String> = Vec::new();
            if input.is_empty() || input == ":" {
                matches = vec![
                    ":project".into(),
                    ":severity".into(),
                    ":time".into(),
                    ":tail".into(),
                    ":clear".into(),
                    ":save".into(),
                    ":load".into(),
                    ":help".into(),
                    ":quit".into(),
                    ":about".into(),
                ];
            } else {
                let commands = [
                    ":project",
                    ":severity",
                    ":time",
                    ":tail",
                    ":clear",
                    ":save",
                    ":load",
                    ":help",
                    ":quit",
                    ":about",
                ];
                for cmd in commands.iter() {
                    if cmd.starts_with(input) {
                        matches.push(cmd.to_string());
                    }
                }
                // Add saved filters prefixed with :load
                for slot in self.config.filters.keys() {
                    let load_cmd = format!(":load {}", slot);
                    if load_cmd.starts_with(input) || input.len() > 1 && load_cmd.contains(input) {
                        matches.push(load_cmd);
                    }
                }
            }
            self.palette.matches = matches.into_iter().take(12).collect();
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        if area.width < 80 || area.height < 10 {
            let text = Text::from(format!(
                "Terminal too small\nNeed ≥ 80×24, got {}×{}",
                area.width, area.height
            ));
            frame.render_widget(
                Paragraph::new(text).centered().style(Style::new().red()),
                area,
            );
            return;
        }

        let [header_area, main_area, footer_area] = match Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area)[..]
        {
            [a, b, c] => [a, b, c],
            _ => return,
        };

        self.render_header(frame, header_area);
        self.render_main(frame, main_area);
        self.render_footer(frame, footer_area);

        if self.show_help {
            self.render_help_overlay(frame, area);
        }
        if self.show_project_picker {
            self.render_project_picker(frame, area);
        }
        if self.palette.active {
            self.render_command_palette(frame, area);
        }
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let project_str = self.project.as_deref().unwrap_or("no project");

        let mut parts = vec![
            Span::styled(" log-ex-tui", Style::new().bold().green()),
            Span::raw(" › ").fg(Color::DarkGray),
            Span::styled(project_str, Style::new().bold().cyan()),
            Span::raw(" › ").fg(Color::DarkGray),
        ];
        if self.tail_on {
            parts.push(Span::styled("tail:on", Style::new().fg(Color::Green)));
        } else {
            parts.push(Span::styled("tail:off", Style::new().fg(Color::DarkGray)));
        }
        let active_filter = self
            .filter
            .raw_query
            .as_deref()
            .filter(|raw| !raw.is_empty())
            .unwrap_or(&self.filter.free_text);
        if !active_filter.is_empty() {
            parts.push(Span::raw(" › ").fg(Color::DarkGray));
            parts.push(Span::styled(
                active_filter,
                Style::new().italic().fg(Color::Yellow),
            ));
        }
        parts.push(Span::raw(format!("  {} entries", self.entries.len())));

        frame.render_widget(Line::from(parts), area);
    }

    fn render_main(&self, frame: &mut Frame, area: Rect) {
        if let Some(err) = &self.error {
            let block = Paragraph::new(err.as_str())
                .style(Style::new().red().bold())
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Error ")
                        .fg(Color::Red),
                )
                .centered();
            frame.render_widget(block, area);
            return;
        }

        let [facets_area, body_area] = match Layout::horizontal([
            Constraint::Length(22),
            Constraint::Min(1),
        ])
        .split(area)[..]
        {
            [a, b] => [a, b],
            _ => return,
        };

        let [list_area, detail_area] =
            match Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(body_area)[..]
            {
                [a, b] => [a, b],
                _ => return,
            };

        self.render_facets(frame, facets_area);
        self.render_entry_list(frame, list_area);
        self.render_detail(frame, detail_area);
    }

    fn render_facets(&self, frame: &mut Frame, area: Rect) {
        let is_focused = self.focus == Focus::Facets;
        let border = if is_focused {
            Style::new().fg(Color::Cyan)
        } else {
            Style::new().fg(Color::DarkGray)
        };

        let mut items: Vec<ListItem> = Vec::new();
        items.push(ListItem::new(Line::from(Span::styled(
            " Severities",
            Style::new().bold().underlined(),
        ))));
        for (i, name) in SEVERITY_NAMES.iter().enumerate() {
            let checked = self.filter.severities[i];
            let icon = if checked { " ☑" } else { " ☐" };
            items.push(ListItem::new(Line::from(vec![
                Span::raw(format!(" {} {}", i, icon)),
                Span::styled(*name, severity_color(i)),
            ])));
        }
        items.push(ListItem::new(Line::from(Span::raw(""))));
        items.push(ListItem::new(Line::from(Span::styled(
            " Time",
            Style::new().bold().underlined(),
        ))));
        items.push(ListItem::new(Line::from(vec![
            Span::raw(" "),
            Span::styled(self.time_range.to_string(), Style::new().fg(Color::Yellow)),
        ])));
        let active_filter = self
            .filter
            .raw_query
            .as_deref()
            .filter(|raw| !raw.is_empty())
            .unwrap_or(&self.filter.free_text);
        if !active_filter.is_empty() {
            items.push(ListItem::new(Line::from(Span::raw(""))));
            items.push(ListItem::new(Line::from(Span::styled(
                " Text",
                Style::new().bold().underlined(),
            ))));
            items.push(ListItem::new(Line::from(vec![
                Span::raw(" "),
                Span::styled(active_filter, Style::new().fg(Color::Yellow).italic()),
            ])));
        }

        frame.render_widget(
            List::new(items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Filter ")
                    .border_style(border),
            ),
            area,
        );
    }

    fn render_entry_list(&self, frame: &mut Frame, area: Rect) {
        let is_focused = self.focus == Focus::List;
        let border = if is_focused {
            Style::new().fg(Color::Cyan)
        } else {
            Style::new().fg(Color::DarkGray)
        };

        if self.entries.is_empty() {
            let para =
                Paragraph::new("No entries match filter\n\nPress / to search\n: for commands")
                    .centered()
                    .style(Style::new().fg(Color::DarkGray))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(" Log Entries ")
                            .border_style(border),
                    );
            frame.render_widget(para, area);
            return;
        }

        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|e| {
                let sev_style = severity_color(e.severity_num());
                let time = e.display_time();
                let summary = e.display_summary();
                let max_len = area.width as usize;
                let max_len = max_len.saturating_sub(time.len() + 12);
                let summary = truncate_for_display(&summary, max_len);
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!(" {} ", e.severity),
                        sev_style.bg(Color::Rgb(30, 30, 30)),
                    ),
                    Span::raw(format!(" {time} {summary}")),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border)
                    .title(format!(" Log Entries ({}) ", self.entries.len())),
            )
            .highlight_style(Style::new().bg(Color::Rgb(40, 40, 60)));

        let mut state = ListState::default();
        state.select(Some(self.selected));
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn render_detail(&self, frame: &mut Frame, area: Rect) {
        let is_focused = self.focus == Focus::Detail;
        let border = if is_focused {
            Style::new().fg(Color::Cyan)
        } else {
            Style::new().fg(Color::DarkGray)
        };

        let entry = match self.entries.get(self.selected) {
            Some(e) => e,
            None => {
                let text = Text::from("Select an entry to view details");
                frame.render_widget(
                    Paragraph::new(text)
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title(" Detail ")
                                .border_style(border),
                        )
                        .centered(),
                    area,
                );
                return;
            }
        };

        let mut lines = vec![Line::from(Span::styled(
            format!(" timestamp: {}", entry.display_time()),
            Style::new().fg(Color::Cyan),
        ))];
        if let Some(ref resource) = entry.resource {
            lines.push(Line::from(Span::raw(format!(
                " resource: {}",
                resource.resource_type
            ))));
        }
        if let Some(ref trace) = entry.trace {
            lines.push(Line::from(Span::raw(format!(" trace: {trace}"))));
        }
        lines.push(Line::from(Span::raw("")));

        if let Some(ref text) = entry.text_payload {
            let truncated = truncate_for_display(text, 2000);
            lines.push(Line::from(Span::styled(
                " textPayload:",
                Style::new().bold(),
            )));
            lines.push(Line::from(Span::raw(format!("   {truncated}"))));
        }
        if let Some(ref json) = entry.json_payload {
            lines.push(Line::from(Span::styled(
                " jsonPayload:",
                Style::new().bold(),
            )));
            let formatted = serde_json::to_string_pretty(json).unwrap_or_default();
            let truncated = if formatted.chars().count() > 2000 {
                format!("{}\n… (truncated)", truncate_for_display(&formatted, 1997))
            } else {
                formatted
            };
            for line in truncated.lines().take(30) {
                lines.push(Line::from(Span::raw(format!("   {line}"))));
            }
        }
        if let Some(ref proto) = entry.proto_payload {
            lines.push(Line::from(Span::styled(
                " protoPayload:",
                Style::new().bold(),
            )));
            let formatted = serde_json::to_string_pretty(proto).unwrap_or_default();
            for line in formatted.lines().take(20) {
                lines.push(Line::from(Span::raw(format!("   {line}"))));
            }
        }

        let visible_height = area.height.saturating_sub(2) as usize; // subtract border lines
        let max_scroll = lines.len().saturating_sub(visible_height);
        let scroll = self.detail_scroll.min(max_scroll);

        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Detail ")
                        .border_style(border),
                )
                .scroll((scroll as u16, 0)),
            area,
        );
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let hints = if self.palette.active {
            " esc cancel  ·  ↵ execute  ·  ⌫ backspace"
        } else if self.show_help {
            " esc / ?  close"
        } else if self.show_project_picker {
            " j/↓ k/↑  nav  ·  ↵ select  ·  esc cancel"
        } else {
            match self.focus {
                Focus::Facets => " j/↓ k/↑ nav  ·  0-8 toggle severity  ·  ⇥ focus",
                Focus::List => {
                    " 0-8 sev  ·  j/↓ k/↑  ·  : cmd  ·  / search  ·  ↵ view  ·  t tail  ·  T time  ·  ? help  ·  ⇥ focus"
                }
                Focus::Detail => {
                    " j/↓ k/↑ scroll  ·  g/G top/bot  ·  ⌃d/⌃u half-page  ·  esc back  ·  ? help"
                }
            }
        };

        frame.render_widget(
            Paragraph::new(Line::from(Span::raw(format!(
                " {}  {}",
                hints, self.status
            ))))
            .style(
                Style::new()
                    .fg(Color::Rgb(180, 180, 180))
                    .bg(Color::Rgb(30, 30, 30)),
            ),
            area,
        );
    }

    fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let w = 66u16.min(area.width.saturating_sub(4));
        let h = 28u16.min(area.height.saturating_sub(4));
        let r = Rect {
            x: (area.width - w) / 2,
            y: (area.height - h) / 2,
            width: w,
            height: h,
        };
        frame.render_widget(Clear, r);

        let text = Text::from(vec![
            Line::from(vec![
                Span::styled(" NAVIGATION", Style::new().bold().underlined()),
                Span::raw("                           "),
                Span::styled(" COMMANDS", Style::new().bold().underlined()),
            ]),
            Line::from(Span::raw(
                " j/↓  k/↑     Move                    :project   Switch project",
            )),
            Line::from(Span::raw(
                " g/G          Top/bottom              :severity  Set severities",
            )),
            Line::from(Span::raw(" ⇥ / ⇧⇥      Cycle focus")),
            Line::from(Span::raw(
                " T            Cycle time range           :time      Set time range",
            )),
            Line::from(Span::raw(
                " h/Esc        Back                    :tail      Toggle tail",
            )),
            Line::from(Span::raw(
                " l/Enter      Forward/select          :clear     Clear filter",
            )),
            Line::from(Span::raw(
                "                                        :quit      Quit",
            )),
            Line::from(Span::raw("")),
            Line::from(Span::raw(
                " VIEW                               :help      This screen",
            )),
            Line::from(Span::raw(" /            Filter/query entries")),
            Line::from(Span::raw(
                " l/Enter      Show entry detail       j/k/⌃d/⌃u  Scroll detail",
            )),
            Line::from(Span::raw(" ↑ / ↓        Select palette suggestion")),
            Line::from(Span::raw(" 0-8          Toggle severity")),
            Line::from(Span::raw(" T            Cycle time range")),
            Line::from(Span::raw(" ⌃r           Hard refresh")),
            Line::from(Span::raw(" P            Project picker")),
            Line::from(Span::styled(
                " Press Esc/?/q to close",
                Style::new().fg(Color::DarkGray),
            )),
        ]);

        frame.render_widget(
            Paragraph::new(text)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(" Help ")
                        .style(Style::new().bg(Color::Rgb(20, 20, 30))),
                )
                .wrap(Wrap { trim: false }),
            r,
        );
    }

    fn render_project_picker(&self, frame: &mut Frame, area: Rect) {
        let w = 50u16.min(area.width.saturating_sub(8));
        let h = (self.projects.len() as u16 + 3)
            .min(20)
            .min(area.height.saturating_sub(4));
        let r = Rect {
            x: (area.width - w) / 2,
            y: (area.height - h) / 2,
            width: w,
            height: h,
        };
        frame.render_widget(Clear, r);

        let items: Vec<ListItem> = self
            .projects
            .iter()
            .map(|p| {
                let selected = Some(p.as_str()) == self.project.as_deref();
                ListItem::new(Line::from(Span::raw(format!(
                    "{}{}",
                    if selected { " ✓ " } else { "   " },
                    p
                ))))
            })
            .collect();

        let list = List::new(items)
            .highlight_symbol("▸ ")
            .highlight_style(
                Style::new()
                    .bg(Color::Rgb(40, 40, 60))
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" Select Project "),
            );

        let mut state = self.picker_state.clone();
        frame.render_stateful_widget(list, r, &mut state);
    }

    fn render_command_palette(&self, frame: &mut Frame, area: Rect) {
        let w = 50u16.min(area.width.saturating_sub(8));
        let match_len = self.palette.matches.len() as u16;
        let h = (3 + match_len).min(14).min(area.height.saturating_sub(4));
        let r = Rect {
            x: (area.width - w) / 2,
            y: 2,
            width: w,
            height: h,
        };
        frame.render_widget(Clear, r);

        let [input_area, matches_area] =
            match Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(r)[..] {
                [a, b] => [a, b],
                _ => return,
            };

        let is_filter_mode = self.palette.input.starts_with('/');
        let display = if self.palette.input.is_empty() {
            ":".into()
        } else {
            self.palette.input.clone()
        };
        let title = if is_filter_mode {
            " Filter "
        } else {
            " Command "
        };
        frame.render_widget(
            Paragraph::new(display.as_str())
                .style(Style::new().fg(Color::Yellow))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .title(title),
                ),
            input_area,
        );

        if !self.palette.matches.is_empty() && !self.palette.input.is_empty() {
            let match_items: Vec<ListItem> = self
                .palette
                .matches
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let style = if self.palette.selected_match == Some(i) {
                        Style::new()
                            .bg(Color::Rgb(40, 40, 60))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    };
                    ListItem::new(Line::from(Span::styled(format!(" {}", m), style)))
                })
                .collect();
            let mut list_state = ListState::default();
            list_state.select(self.palette.selected_match);
            frame.render_stateful_widget(
                List::new(match_items).highlight_style(Style::new().fg(Color::Cyan)),
                matches_area,
                &mut list_state,
            );
        }
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

fn severity_color(level: usize) -> Style {
    match level {
        0 => Style::new().fg(Color::DarkGray),
        1 => Style::new().fg(Color::Magenta),
        2 => Style::new().fg(Color::Green),
        3 => Style::new().fg(Color::Cyan),
        4 => Style::new().fg(Color::Yellow),
        5 => Style::new().fg(Color::Red),
        6 => Style::new().fg(Color::Red).bold(),
        7 => Style::new().fg(Color::White).bg(Color::Red).bold(),
        8 => Style::new()
            .fg(Color::White)
            .bg(Color::Red)
            .bold()
            .add_modifier(Modifier::SLOW_BLINK),
        _ => Style::default(),
    }
}

impl TimeRange {
    fn as_config_value(self) -> &'static str {
        match self {
            Self::FiveM => "5m",
            Self::FifteenM => "15m",
            Self::OneH => "1h",
            Self::SixH => "6h",
            Self::TwentyFourH => "24h",
        }
    }

    fn from_config_value(value: &str) -> Self {
        match value {
            "5m" => Self::FiveM,
            "15m" => Self::FifteenM,
            "1h" => Self::OneH,
            "6h" => Self::SixH,
            "24h" => Self::TwentyFourH,
            _ => Self::OneH,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::FiveM => Self::FifteenM,
            Self::FifteenM => Self::OneH,
            Self::OneH => Self::SixH,
            Self::SixH => Self::TwentyFourH,
            Self::TwentyFourH => Self::FiveM,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::FiveM => Self::TwentyFourH,
            Self::FifteenM => Self::FiveM,
            Self::OneH => Self::FifteenM,
            Self::SixH => Self::OneH,
            Self::TwentyFourH => Self::SixH,
        }
    }
}

impl std::fmt::Display for TimeRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_config_value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn time_range_cycles() {
        assert_eq!(TimeRange::FiveM.next(), TimeRange::FifteenM);
        assert_eq!(TimeRange::FifteenM.next(), TimeRange::OneH);
        assert_eq!(TimeRange::OneH.next(), TimeRange::SixH);
        assert_eq!(TimeRange::SixH.next(), TimeRange::TwentyFourH);
        assert_eq!(TimeRange::TwentyFourH.next(), TimeRange::FiveM);

        assert_eq!(TimeRange::FiveM.prev(), TimeRange::TwentyFourH);
        assert_eq!(TimeRange::FifteenM.prev(), TimeRange::FiveM);
        assert_eq!(TimeRange::OneH.prev(), TimeRange::FifteenM);
        assert_eq!(TimeRange::SixH.prev(), TimeRange::OneH);
        assert_eq!(TimeRange::TwentyFourH.prev(), TimeRange::SixH);
    }

    #[test]
    fn time_range_display_label() {
        assert_eq!(TimeRange::OneH.to_string(), "1h");
        assert_eq!(TimeRange::FiveM.to_string(), "5m");
        assert_eq!(TimeRange::TwentyFourH.to_string(), "24h");
    }

    #[test]
    fn build_filter_prefers_raw_query() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(tx);
        app.filter.raw_query = Some("resource.type=\"k8s_container\"".into());
        app.filter.free_text = "ignored".into();

        let filter = app.build_filter_string().unwrap_or_default();

        assert!(filter.contains("resource.type=\"k8s_container\""));
        assert!(!filter.contains("textPayload:\"ignored\""));
    }

    #[test]
    fn truncate_for_display_handles_utf8() {
        assert_eq!(truncate_for_display("hello", 10), "hello");
        assert_eq!(truncate_for_display("a🙂b🙂c", 4), "a🙂b…");
        assert_eq!(truncate_for_display("🙂🙂", 1), "…");
    }
    #[test]
    fn build_filter_preserves_field_query_quotes() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(tx);
        app.filter.free_text = "resource.labels.container_name=\"ms-notification\"".into();

        let filter = app.build_filter_string().unwrap_or_default();
        assert!(filter.contains("resource.labels.container_name=\"ms-notification\""));
        assert!(!filter.contains("\\\"")); // quotes should NOT be escaped
    }

    #[test]
    fn build_filter_escapes_text_payload_quotes() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(tx);
        app.filter.free_text = "foo\"bar".into();

        let filter = app.build_filter_string().unwrap_or_default();
        assert!(filter.contains("textPayload:\"foo\\\"bar\""));
    }
}
