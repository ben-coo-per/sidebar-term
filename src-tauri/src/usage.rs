//! Usage: how much of each coding agent's usage limits is spent, for the sidebar Panel's Usage
//! section. Read only while the webview watches, and only for the agents chosen in Settings.
//!
//! Claude Code: `GET https://api.anthropic.com/api/oauth/usage`, the undocumented endpoint behind
//! Claude Code's `/usage`, authorised with the OAuth token Claude Code keeps in the login Keychain
//! (`Claude Code-credentials`; `~/.claude/.credentials.json` where it has no Keychain). The token
//! is only ever read: refreshing it here would rotate Claude Code's refresh token and sign it out.
//! An expired token keeps the last numbers, marked stale, until Claude Code renews it. Polled
//! every minute.
//!
//! Codex: the `rate_limits` Codex records in its session logs (`~/.codex/sessions/YYYY/MM/DD/
//! rollout-*.jsonl`) from its API's response headers, on every turn. Free to read, so checked
//! every tick, but only as fresh as the last Codex turn on this Mac.
//!
//! The network and the Keychain go through `/usr/bin/curl` and `/usr/bin/security`, as Activity
//! uses `/bin/ps`: no HTTP or Keychain crate. The token reaches curl on stdin, never in argv,
//! which `ps` shows to every user.

use crate::model::{AgentKind, AgentUsage, UsageSnapshot, UsageWindow, EVENT_USAGE};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

/// Time between reads while watched. Codex is re-read each tick if its log changed.
const TICK: Duration = Duration::from_secs(5);
/// Time between Claude Code usage requests.
const CLAUDE_EVERY: Duration = Duration::from_secs(60);
/// Wait after the usage endpoint rate-limits us.
const CLAUDE_BACKOFF: Duration = Duration::from_secs(300);
const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
/// Claude Code's windows, in display order: response key, label.
const CLAUDE_WINDOWS: [(&str, &str); 4] = [
    ("five_hour", "5h"),
    ("seven_day", "Week"),
    ("seven_day_opus", "Opus wk"),
    ("seven_day_sonnet", "Sonnet wk"),
];
/// Codex logs are looked for in this many of the newest day directories.
const CODEX_RECENT_DAYS: usize = 14;
/// At most this many of the newest Codex logs are searched for a usage record.
const CODEX_SCAN: usize = 10;

#[derive(Default)]
struct Watch {
    on: bool,
    agents: Vec<AgentKind>,
    /// Set by `Usage::watch`: read now, and send even if nothing changed.
    changed: bool,
}

/// Handle to the reading thread (Tauri state). Starts idle.
pub struct Usage {
    watch: Arc<(Mutex<Watch>, Condvar)>,
}

impl Usage {
    /// Start (or change) or stop watching `agents`. Starting reads at once and sends a snapshot.
    pub fn watch(&self, on: bool, agents: Vec<AgentKind>) {
        let (lock, cvar) = &*self.watch;
        *lock.lock().unwrap_or_else(PoisonError::into_inner) = Watch {
            on,
            agents,
            changed: true,
        };
        cvar.notify_all();
    }
}

/// Start the reading thread, idle until `Usage::watch(true, ..)`. While watched, emit a
/// `UsageSnapshot` whenever it changes, checking every `TICK`.
///
/// The thread never exits: a panic skips the tick.
pub fn spawn(app: AppHandle) -> Usage {
    let watch = Arc::new((Mutex::new(Watch::default()), Condvar::new()));
    let shared = Arc::clone(&watch);
    let started = thread::Builder::new().name("usage".into()).spawn(move || {
        let (lock, cvar) = &*shared;
        let mut reader = Reader::default();
        let mut sent: Option<UsageSnapshot> = None;
        loop {
            let (agents, force) = {
                let mut w = cvar
                    .wait_while(lock.lock().unwrap_or_else(PoisonError::into_inner), |w| {
                        !w.on
                    })
                    .unwrap_or_else(PoisonError::into_inner);
                (w.agents.clone(), std::mem::take(&mut w.changed))
            };
            match catch_unwind(AssertUnwindSafe(|| reader.read(&agents, Instant::now()))) {
                Ok(snapshot) if force || sent.as_ref() != Some(&snapshot) => {
                    if let Err(e) = app.emit(EVENT_USAGE, &snapshot) {
                        eprintln!("usage: emit failed: {e}");
                    }
                    sent = Some(snapshot);
                }
                Ok(_) => {}
                Err(_) => {
                    eprintln!("usage: reading panicked; skipping this tick");
                    reader = Reader::default();
                }
            }
            // Sleep a tick, waking early to stop or to change agents.
            drop(
                cvar.wait_timeout_while(
                    lock.lock().unwrap_or_else(PoisonError::into_inner),
                    TICK,
                    |w| w.on && !w.changed,
                )
                .unwrap_or_else(PoisonError::into_inner),
            );
        }
    });
    if let Err(e) = started {
        eprintln!("usage: could not start thread: {e}");
    }
    Usage { watch }
}

/// Keeps what is worth not re-reading: Claude Code's last answer, parsed Codex logs.
#[derive(Default)]
struct Reader {
    claude: Option<AgentUsage>,
    claude_next: Option<Instant>,
    /// Codex log -> (mtime, size, its last usage record).
    codex_logs: HashMap<PathBuf, (SystemTime, u64, Option<CodexReading>)>,
}

impl Reader {
    fn read(&mut self, agents: &[AgentKind], now: Instant) -> UsageSnapshot {
        let agents = agents
            .iter()
            .filter_map(|agent| match agent {
                AgentKind::Claude => Some(self.claude(now)),
                AgentKind::Codex => Some(self.codex()),
                AgentKind::Gemini => None, // no usage source yet
            })
            .collect();
        UsageSnapshot { agents }
    }

    fn claude(&mut self, now: Instant) -> AgentUsage {
        if let (Some(last), Some(next)) = (&self.claude, self.claude_next) {
            if now < next {
                return last.clone();
            }
        }
        let (usage, wait) = read_claude(self.claude.as_ref());
        self.claude = Some(usage.clone());
        self.claude_next = Some(now + wait);
        usage
    }

    fn codex(&mut self) -> AgentUsage {
        let logs = home()
            .map(|h| recent_codex_logs(&h.join(".codex/sessions")))
            .unwrap_or_default();
        self.codex_logs
            .retain(|path, _| logs.iter().any(|l| &l.0 == path));
        if logs.is_empty() {
            return unavailable(AgentKind::Codex, "No Codex sessions on this Mac yet");
        }
        for (path, mtime, len) in logs.into_iter().take(CODEX_SCAN) {
            let reading = match self.codex_logs.get(&path) {
                Some((m, l, r)) if *m == mtime && *l == len => r.clone(),
                _ => {
                    let r = fs::read_to_string(&path)
                        .ok()
                        .and_then(|s| last_codex_reading(&s));
                    self.codex_logs.insert(path, (mtime, len, r.clone()));
                    r
                }
            };
            if let Some(r) = reading {
                return AgentUsage {
                    agent: AgentKind::Codex,
                    windows: r.windows,
                    plan: r.plan,
                    updated_at: r.updated_at,
                    error: None,
                };
            }
        }
        unavailable(
            AgentKind::Codex,
            "No usage yet: it appears after a Codex turn",
        )
    }
}

fn unavailable(agent: AgentKind, why: &str) -> AgentUsage {
    AgentUsage {
        agent,
        windows: Vec::new(),
        plan: None,
        updated_at: None,
        error: Some(why.to_owned()),
    }
}

/// The last good numbers, if any, with why they are stale.
fn stale(agent: AgentKind, last: Option<&AgentUsage>, why: &str) -> AgentUsage {
    match last {
        Some(l) => AgentUsage {
            error: Some(why.to_owned()),
            ..l.clone()
        },
        None => unavailable(agent, why),
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

// ---- Claude Code ----

const CLAUDE_EXPIRED: &str = "Sign-in expired: it renews when you next use Claude Code";

/// Claude Code's usage, and how long to wait before asking again.
fn read_claude(last: Option<&AgentUsage>) -> (AgentUsage, Duration) {
    let agent = AgentKind::Claude;
    let creds = match claude_credentials() {
        Ok(c) => c,
        Err(e) => return (stale(agent, last, &e), CLAUDE_EVERY),
    };
    if creds.expires_at.is_some_and(|at| at <= now_ms()) {
        return (stale(agent, last, CLAUDE_EXPIRED), CLAUDE_EVERY);
    }
    match fetch_claude_usage(&creds.token) {
        Ok(body) => match parse_claude_usage(&body) {
            Some(windows) => (
                AgentUsage {
                    agent,
                    windows,
                    plan: creds.plan,
                    updated_at: Some(now_ms()),
                    error: None,
                },
                CLAUDE_EVERY,
            ),
            None => (
                stale(agent, last, "Unexpected answer from api.anthropic.com"),
                CLAUDE_EVERY,
            ),
        },
        Err(Fetch::Status(429)) => (
            stale(agent, last, "Rate limited: trying again in 5 minutes"),
            CLAUDE_BACKOFF,
        ),
        Err(Fetch::Status(401 | 403)) => (stale(agent, last, CLAUDE_EXPIRED), CLAUDE_EVERY),
        Err(Fetch::Status(code)) => (
            stale(agent, last, &format!("Usage request failed (HTTP {code})")),
            CLAUDE_EVERY,
        ),
        Err(Fetch::Unreachable) => (
            stale(agent, last, "Couldn't reach api.anthropic.com"),
            CLAUDE_EVERY,
        ),
    }
}

#[derive(Debug, PartialEq)]
struct ClaudeCredentials {
    token: String,
    /// Epoch ms.
    expires_at: Option<u64>,
    plan: Option<String>,
}

fn claude_credentials() -> Result<ClaudeCredentials, String> {
    let raw = keychain_password(CLAUDE_KEYCHAIN_SERVICE)
        .or_else(|| fs::read_to_string(home()?.join(".claude/.credentials.json")).ok())
        .ok_or("Not signed in to Claude Code")?;
    parse_claude_credentials(&raw)
}

/// `{"claudeAiOauth":{"accessToken":..,"expiresAt":<ms>,"subscriptionType":"max",..}}`
fn parse_claude_credentials(raw: &str) -> Result<ClaudeCredentials, String> {
    const BAD: &str = "Couldn't read Claude Code's sign-in";
    let v: Value = serde_json::from_str(raw).map_err(|_| BAD)?;
    let oauth = v
        .get("claudeAiOauth")
        .ok_or("Claude Code isn't signed in to a subscription")?;
    let token = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        // It goes into a curl config line: refuse anything that could break out of the quotes.
        .filter(|t| !t.is_empty() && !t.contains(['"', '\\', '\n', '\r']))
        .ok_or(BAD)?;
    Ok(ClaudeCredentials {
        token: token.to_owned(),
        expires_at: oauth.get("expiresAt").and_then(Value::as_u64),
        plan: oauth
            .get("subscriptionType")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// A generic password from the login Keychain, `None` if missing or refused.
fn keychain_password(service: &str) -> Option<String> {
    let out = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service, "-w"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let pw = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !pw.is_empty()).then_some(pw)
}

#[derive(Debug, PartialEq)]
enum Fetch {
    Unreachable,
    Status(u32),
}

fn fetch_claude_usage(token: &str) -> Result<Value, Fetch> {
    let mut child = Command::new("/usr/bin/curl")
        .args([
            "--silent",
            "--max-time",
            "15",
            "--config",
            "-",
            "--write-out",
            "\n%{http_code}",
            CLAUDE_USAGE_URL,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Fetch::Unreachable)?;
    let config = format!(
        "header = \"Authorization: Bearer {token}\"\n\
         header = \"anthropic-beta: oauth-2025-04-20\"\n\
         header = \"User-Agent: sidebar-term\"\n"
    );
    let wrote = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(config.as_bytes()).is_ok());
    let out = child.wait_with_output().map_err(|_| Fetch::Unreachable)?;
    if !wrote {
        return Err(Fetch::Unreachable);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').ok_or(Fetch::Unreachable)?;
    match code.trim().parse::<u32>() {
        Ok(200) => serde_json::from_str(body).map_err(|_| Fetch::Status(200)),
        Ok(0) | Err(_) => Err(Fetch::Unreachable),
        Ok(code) => Err(Fetch::Status(code)),
    }
}

/// `{"five_hour":{"utilization":48.0,"resets_at":"2026-..."},"seven_day":{..},"seven_day_opus":null}`
/// -> the windows present, in `CLAUDE_WINDOWS` order. `None` when there are none.
fn parse_claude_usage(v: &Value) -> Option<Vec<UsageWindow>> {
    let windows: Vec<_> = CLAUDE_WINDOWS
        .iter()
        .filter_map(|&(key, label)| {
            let w = v.get(key)?;
            Some(UsageWindow {
                label: label.to_owned(),
                used_percent: w.get("utilization")?.as_f64()? as f32,
                resets_at: w
                    .get("resets_at")
                    .and_then(Value::as_str)
                    .and_then(parse_rfc3339_ms),
            })
        })
        .collect();
    (!windows.is_empty()).then_some(windows)
}

// ---- Codex ----

#[derive(Clone, Debug, PartialEq)]
struct CodexReading {
    windows: Vec<UsageWindow>,
    plan: Option<String>,
    updated_at: Option<u64>,
}

/// Codex logs in the newest `CODEX_RECENT_DAYS` day directories of `sessions`
/// (`YYYY/MM/DD/*.jsonl`), most recently written first, with their mtime and size.
fn recent_codex_logs(sessions: &Path) -> Vec<(PathBuf, SystemTime, u64)> {
    fn newest_first(dir: &Path) -> Vec<PathBuf> {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort_unstable_by(|a, b| b.cmp(a));
        entries
    }
    let days = newest_first(sessions)
        .into_iter()
        .flat_map(|year| newest_first(&year))
        .flat_map(|month| newest_first(&month))
        .filter(|day| day.is_dir())
        .take(CODEX_RECENT_DAYS);
    let mut logs: Vec<_> = days
        .flat_map(|day| newest_first(&day))
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .filter_map(|p| {
            let meta = fs::metadata(&p).ok()?;
            Some((p, meta.modified().ok()?, meta.len()))
        })
        .collect();
    logs.sort_by(|a, b| b.1.cmp(&a.1));
    logs
}

/// The last usage record in a Codex log: a line like
/// `{"timestamp":"..","payload":{"type":"token_count","rate_limits":{"primary":{..},"secondary":null,"plan_type":"plus"}}}`.
fn last_codex_reading(log: &str) -> Option<CodexReading> {
    log.lines()
        .rev()
        .filter(|l| l.contains("\"rate_limits\":{"))
        .find_map(codex_reading)
}

fn codex_reading(line: &str) -> Option<CodexReading> {
    let v: Value = serde_json::from_str(line).ok()?;
    let limits = v.get("payload")?.get("rate_limits")?;
    let at = v
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_rfc3339_ms);
    let mut windows: Vec<(u64, UsageWindow)> = ["primary", "secondary"]
        .iter()
        .filter_map(|k| codex_window(limits.get(k)?, at))
        .collect();
    if windows.is_empty() {
        return None;
    }
    windows.sort_by_key(|(minutes, _)| *minutes); // shortest window first, as for Claude Code
    Some(CodexReading {
        windows: windows.into_iter().map(|(_, w)| w).collect(),
        plan: limits
            .get("plan_type")
            .and_then(Value::as_str)
            .map(str::to_owned),
        updated_at: at,
    })
}

/// `{"used_percent":3.0,"window_minutes":10080,"resets_at":<epoch s>}`; older Codex versions say
/// `resets_in_seconds` from the record's `timestamp` (`at`) instead. With its length in minutes.
fn codex_window(w: &Value, at: Option<u64>) -> Option<(u64, UsageWindow)> {
    let used = w.get("used_percent")?.as_f64()?;
    let minutes = w.get("window_minutes").and_then(Value::as_u64).unwrap_or(0);
    let resets_at = match w.get("resets_at").and_then(Value::as_u64) {
        Some(secs) => Some(secs * 1000),
        None => w
            .get("resets_in_seconds")
            .and_then(Value::as_u64)
            .zip(at)
            .map(|(secs, at)| at + secs * 1000),
    };
    Some((
        minutes,
        UsageWindow {
            label: window_label(minutes),
            used_percent: used as f32,
            resets_at,
        },
    ))
}

fn window_label(minutes: u64) -> String {
    match minutes {
        0 => "Limit".into(),
        10_080 => "Week".into(),
        m if m % 1440 == 0 => format!("{}d", m / 1440),
        m if m % 60 == 0 => format!("{}h", m / 60),
        m => format!("{m}m"),
    }
}

// ---- Time ----

/// `2026-04-25T06:31:40.140Z`, `2025-10-01T15:59:59.943648+02:00` (fraction optional) as epoch ms.
fn parse_rfc3339_ms(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || !matches!(b[10], b'T' | b't' | b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        let part = s.get(from..to)?;
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut rest = s.get(19..)?;
    let mut ms = 0;
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let kept = &frac[..digits.min(3)];
        ms = kept.parse::<i64>().ok()? * 10i64.pow(3 - kept.len() as u32);
        rest = &frac[digits..];
    }
    let offset_minutes = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            if rest.len() != 6 || rest.as_bytes()[3] != b':' {
                return None;
            }
            let h: i64 = rest.get(1..3)?.parse().ok()?;
            let m: i64 = rest.get(4..6)?.parse().ok()?;
            sign * (h * 60 + m)
        }
    };
    let secs = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second
        - offset_minutes * 60;
    u64::try_from(secs * 1000 + ms).ok()
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::testutil::TempDir;
    use serde_json::json;

    #[test]
    fn rfc3339() {
        let cases = [
            ("1970-01-01T00:00:00Z", Some(0)),
            ("2026-04-25T06:31:40.140Z", Some(1_777_098_700_140)),
            ("2025-10-01T15:59:59.943648+02:00", Some(1_759_327_199_943)),
            ("1999-12-31T23:00:00-05:30", Some(946_701_000_000)),
            ("2026-04-25T06:31:40", None),
            ("2026-04-25T06:31:40.Z", None),
            ("2026-13-25T06:31:40Z", None),
            ("2026-04-25T06:31:40+0200", None),
            ("2026-04-25T06:3é:40Z", None),
            ("", None),
        ];
        for (s, want) in cases {
            assert_eq!(parse_rfc3339_ms(s), want, "{s:?}");
        }
    }

    #[test]
    fn claude_credentials() {
        let raw = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-abc","refreshToken":"r","expiresAt":1760000000000,"scopes":["user:inference"],"subscriptionType":"max"}}"#;
        assert_eq!(
            parse_claude_credentials(raw),
            Ok(ClaudeCredentials {
                token: "sk-ant-oat01-abc".into(),
                expires_at: Some(1_760_000_000_000),
                plan: Some("max".into()),
            })
        );
        assert!(parse_claude_credentials("{}").is_err());
        assert!(parse_claude_credentials("not json").is_err());
        assert!(parse_claude_credentials(r#"{"claudeAiOauth":{"accessToken":"a\"b"}}"#).is_err());
    }

    #[test]
    fn claude_usage() {
        let body = json!({
            "five_hour": {"utilization": 48.0, "resets_at": "2026-04-25T06:31:40.140Z"},
            "seven_day": {"utilization": 19, "resets_at": null},
            "seven_day_opus": null,
            "extra_usage": {"is_enabled": false},
        });
        let windows = parse_claude_usage(&body).expect("windows");
        assert_eq!(
            windows,
            [
                UsageWindow {
                    label: "5h".into(),
                    used_percent: 48.0,
                    resets_at: Some(1_777_098_700_140),
                },
                UsageWindow {
                    label: "Week".into(),
                    used_percent: 19.0,
                    resets_at: None,
                },
            ]
        );
        assert_eq!(parse_claude_usage(&json!({"error": "nope"})), None);
    }

    #[test]
    fn codex_records() {
        let log = r#"{"timestamp":"2026-04-25T06:31:00.000Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":1.0,"window_minutes":10080,"resets_at":1777703487},"secondary":null,"plan_type":"free"}}}
{"timestamp":"2026-04-25T06:31:40.140Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{}},"rate_limits":{"limit_id":"codex","primary":{"used_percent":64.0,"window_minutes":10080,"resets_at":1777703487},"secondary":{"used_percent":12.5,"window_minutes":300,"resets_in_seconds":60},"plan_type":"plus"}}}
{"timestamp":"2026-04-25T06:32:00.000Z","type":"event_msg","payload":{"type":"token_count","rate_limits":null}}
{"timestamp":"2026-04-25T06:32:01.000Z","type":"response_item","payload":{"type":"message"}}
"#;
        assert_eq!(
            last_codex_reading(log),
            Some(CodexReading {
                windows: vec![
                    UsageWindow {
                        label: "5h".into(),
                        used_percent: 12.5,
                        resets_at: Some(1_777_098_760_140),
                    },
                    UsageWindow {
                        label: "Week".into(),
                        used_percent: 64.0,
                        resets_at: Some(1_777_703_487_000),
                    },
                ],
                plan: Some("plus".into()),
                updated_at: Some(1_777_098_700_140),
            })
        );
        assert_eq!(last_codex_reading("{\"payload\":{}}\n"), None);
    }

    #[test]
    fn window_labels() {
        let cases = [
            (300, "5h"),
            (10_080, "Week"),
            (1440, "1d"),
            (43_200, "30d"),
            (90, "90m"),
            (0, "Limit"),
        ];
        for (minutes, want) in cases {
            assert_eq!(window_label(minutes), want);
        }
    }

    #[test]
    fn finds_the_most_recently_written_codex_log() {
        let dir = TempDir::new("codex");
        let write = |rel: &str| {
            let p = dir.path().join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, "{}\n").unwrap();
            thread::sleep(Duration::from_millis(20)); // distinct mtimes
            p
        };
        write("2026/04/24/rollout-b.jsonl");
        write("2026/04/25/rollout-c.jsonl");
        write("2026/04/25/notes.txt");
        // Started yesterday, still being written: newest by mtime.
        let resumed = write("2026/04/24/rollout-a.jsonl");
        let logs = recent_codex_logs(dir.path());
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[0].0, resumed);
        assert!(recent_codex_logs(&dir.path().join("missing")).is_empty());
    }
}
