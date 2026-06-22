# log-ex-tui

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

`log-ex-tui` is a `k9s`-style terminal UI for browsing Google Cloud Logging entries.

It supports:

- Application Default Credentials from `GOOGLE_APPLICATION_CREDENTIALS`
- Local ADC files from `~/.config/gcloud/application_default_credentials.json`
- Local ADC files from `~/Library/Application Support/gcloud/application_default_credentials.json`
- `CLOUDSDK_CONFIG/application_default_credentials.json`
- GCE/GKE metadata server tokens
- Project picking through Cloud Resource Manager
- Log entry listing, filtering, detail view, and polling-based tail mode

## Build

```bash
cargo build
cargo build --release
```

Release binary:

```bash
./target/release/log-ex-tui
```

## Quick Start

1. Authenticate with Application Default Credentials:

```bash
gcloud auth application-default login
```

2. Start the app:

```bash
./target/release/log-ex-tui
```

3. Or skip the project picker:

```bash
./target/release/log-ex-tui --project=my-project-id
```

4. Enable debug logging when needed:

```bash
./target/release/log-ex-tui --debug
```

5. Tune tail polling if needed:

```bash
./target/release/log-ex-tui --tail-interval-seconds=60 --tail-page-size=50
```

Debug logs are written to `~/.cache/log-ex-tui/debug.log`.

## Usage

### Navigation

| Key           | Action                                        |
| ------------- | --------------------------------------------- |
| `j` / `↓`     | Move down                                     |
| `k` / `↑`     | Move up                                       |
| `g`           | Go to top                                     |
| `G`           | Go to bottom                                  |
| `h` / `Esc`   | Back to list view                             |
| `l` / `Enter` | Forward / select detail                       |
| `Tab`         | Cycle focus forward (Facets → List → Detail)  |
| `Shift+Tab`   | Cycle focus backward (Facets → Detail → List) |
| `P`           | Project picker                                |
| `?`           | Toggle help overlay                           |
| `q`           | Quit (when not in a modal)                    |
| `Ctrl+C`      | Quit                                          |
| `Ctrl+R`      | Hard refresh entries                          |

### Filter & View

| Key        | Action                                                  |
| ---------- | ------------------------------------------------------- |
| `/`        | Open command palette with `/` prefix (free-text filter) |
| `:`        | Open command palette                                    |
| `↑` / `↓`  | Select palette suggestion                               |
| `0`–`8`    | Toggle severity levels (0=DEFAULT, 8=EMERGENCY)         |
| `T`        | Cycle time range (5m → 15m → 1h → 6h → 24h)             |
| `t`        | Toggle tail mode on/off                                 |
| `)`        | Clear all severity toggles                              |
| `F1`–`F10` | Load saved filter slot (`f1`–`f10`)                     |

### Command Palette

Type `:` to open the command palette, then enter one of the following:

| Command               | Action                                                           |
| --------------------- | ---------------------------------------------------------------- |
| `↑` / `↓`             | Navigate palette suggestions                                     |
| `:help` / `:?`        | Show help overlay                                                |
| `:project` / `:p`     | Open project picker                                              |
| `:tail` / `:t`        | Toggle tail mode                                                 |
| `:clear` / `:c`       | Reset filter to default severities and clear text/query          |
| `:severity <levels>`  | Set severity levels explicitly (e.g., `:severity error warning`) |
| `:time <range>`       | Set time range (`5m`, `15m`, `1h`, `6h`, `24h`)                  |
| `:save <slot> [name]` | Save current filter to slot (e.g., `:save f1 production-errors`) |
| `:load <slot>`        | Load a saved filter (e.g., `:load f1`)                           |
| `:about` / `:ab`      | Show current status (project, tail, entries, filter)             |
| `/ <text>`            | Free-text search (e.g., `/error`)                                |

### Severity Levels

The numeric keys `0`–`8` map to these GCP severity levels:

| Key | Severity  |
| --- | --------- |
| `0` | DEFAULT   |
| `1` | DEBUG     |
| `2` | INFO      |
| `3` | NOTICE    |
| `4` | WARNING   |
| `5` | ERROR     |
| `6` | CRITICAL  |
| `7` | ALERT     |
| `8` | EMERGENCY |

### Time Ranges

The `T` key cycles through these preset time ranges:

| Range | Description     |
| ----- | --------------- |
| `5m`  | Last 5 minutes  |
| `15m` | Last 15 minutes |
| `1h`  | Last hour       |
| `6h`  | Last 6 hours    |
| `24h` | Last 24 hours   |

### Filter Syntax

- **Field queries**: Type `resource.labels.container_name="my-container"` or `jsonPayload.message="error"` to filter by specific fields. The app detects field paths automatically.
- **Free-text search**: Plain text without field paths is wrapped in `textPayload:"..."`.
- **Operators**: Use `=`, `!=`, `:`, `>`, `<` for comparisons.
- **Raw query** (`: <query>`): Enter a raw GCP Logging query string directly for advanced filtering.
- The filter bar shows which mode is active.

### Saved Filters

- Press `F1`–`F10` to instantly recall saved filter configurations.
- Use `:save f1 my-filter-name` to save the current filter state (severities, time range, text/query) to slot `f1`.
- Use `:load f1` or simply press `F1` to restore it.
- Saved filters persist across restarts in the config file.

### Tail Mode

- Toggle with `t` or `:tail`.
- Polls for new entries every 30 seconds by default (configurable via CLI).
- New entries are deduplicated by `insertId` to prevent duplicates.
- The in-memory entry list is capped at 10,000 rows.
- Tail mode is best-effort; small gaps are possible during reconnects.

## CLI Flags

| Flag                             | Short | Default | Description                                             |
| -------------------------------- | ----- | ------- | ------------------------------------------------------- |
| `--project <PROJECT>`            | `-p`  | —       | Start directly in a project (skips picker)              |
| `--debug`                        | `-d`  | —       | Enable debug logging to `~/.cache/log-ex-tui/debug.log` |
| `--tail-interval-seconds <SECS>` | —     | `30`    | Tail polling interval in seconds                        |
| `--tail-page-size <SIZE>`        | —     | `50`    | Tail polling page size                                  |
| `--help`                         | `-h`  | —       | Print help                                              |
| `--version`                      | `-V`  | —       | Print version                                           |

## Authentication

Credential lookup order:

1. `GOOGLE_APPLICATION_CREDENTIALS`
2. `CLOUDSDK_CONFIG/application_default_credentials.json`
3. `~/.config/gcloud/application_default_credentials.json`
4. `~/Library/Application Support/gcloud/application_default_credentials.json`
5. `dirs::config_dir()/gcloud/application_default_credentials.json`
6. GCE/GKE metadata server

Notes:

- Project listing requires permission to call Cloud Resource Manager.
- If project listing fails, you can still start directly with `--project=<id>`.
- Service account credentials are supported, but never commit real keys into the repository.

## Current Behavior

- Tail mode starts disabled by default.
- Tail mode polls every 30 seconds by default.
- New entries are deduplicated by `insertId`.
- The in-memory entry list is capped at 10,000 rows.
- Saved filters persist time range, severities, free-text search, and raw query state.

### Detail View

When viewing a log entry’s details, the following keys are available:

| Key          | Action                                                            |
| ------------ | ----------------------------------------------------------------- |
| `j` / `↓`    | Scroll down                                                       |
| `k` / `↑`    | Scroll up                                                         |
| `g`          | Jump to top of detail                                             |
| `G`          | Jump to bottom of detail                                          |
| `Ctrl+d`     | Scroll down half a page                                           |
| `Ctrl+u`     | Scroll up half a page                                             |
| `c`          | Copy the visible payload (`textPayload`, `jsonPayload`, or `protoPayload`) to the system clipboard |
| `Esc` / `h`  | Back to list view                                                 |

The copied payload is placed on the system clipboard as-is: `textPayload` is copied as plain text, while `jsonPayload` and `protoPayload` are copied as compact JSON. A status message confirms the copy for a few seconds.

## Limitations

- Tailing is REST polling, not streaming RPC.

## Security

Do not commit service-account keys, ADC files, or other GCP credentials into this repository or any public remote. The application loads credentials at runtime from standard ADC paths, environment variables, or the GCE metadata server. If you discover a security-sensitive issue, please report it privately via [GitHub Security Advisories](https://github.com/mhmmdFsl/log-ex-tui/security/advisories) rather than opening a public issue.

## Contributing

Contributions are welcome via GitHub issues and pull requests. Please run `cargo fmt` and `cargo clippy` before submitting, and keep changes focused.

## Support

For questions, bug reports, and feature requests, please use [GitHub Issues](https://github.com/mhmmdFsl/log-ex-tui/issues).

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.
