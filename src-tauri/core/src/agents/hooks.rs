//! Claude Code's hook payloads, read into what Manager shows, and the replies that answer them.
//! Pure: JSON in, JSON out. The events sidebar-term's settings register (`install.rs`):
//!
//! - `SessionStart` / `SessionEnd`: the agent reports through its hooks (`SessionInfo.hooked`).
//! - `PermissionRequest`: may it use a tool? Held open until answered ([`permission`]).
//! - `PreToolUse` for `AskUserQuestion`: a question with options, held open the same way
//!   ([`question`]). Claude Code also sends a `PermissionRequest` for it, which is let through.
//! - `UserPromptSubmit`, `PostToolUse`, `Stop`: lines of Manager's feed ([`event`]), and steps
//!   of the agent's turn for the Journal ([`step`]).
//!
//! Payloads carry `tool_name`, `tool_input`, `tool_response`, `prompt`, `cwd` and
//! `permission_suggestions` (Claude Code's hooks reference). A reply is the JSON a command hook
//! would print; an empty reply leaves Claude Code to ask in its Terminal as usual.

use crate::model::{AgentEventKind, LineTone, PendingKind, PendingLine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The tool whose `PreToolUse` carries a question with options.
pub const ASK_TOOL: &str = "AskUserQuestion";
/// Lines of detail a question shows at most.
const DETAIL_LINES: usize = 8;
/// Characters of one line of text (a prompt, a command) kept for the feed.
const LINE_MAX: usize = 120;
/// Answers a card offers at most: keys 1-9 pick them.
const OPTIONS_MAX: usize = 9;

/// A question read from a hook, before the Host gives it an id and a time.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub kind: PendingKind,
    pub text: String,
    pub detail: Vec<PendingLine>,
    pub options: Vec<String>,
    /// For each option, the reply that gives it.
    pub replies: Vec<Value>,
    /// For the feed: "Asked to edit session.rs".
    pub event: String,
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// `s` on one line, at most `max` characters, with an ellipsis when cut.
pub fn one_line(s: &str, max: usize) -> String {
    let line = s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let mut out: String = line.chars().take(max).collect();
    if line.chars().count() > max || s.lines().filter(|l| !l.trim().is_empty()).count() > 1 {
        out.push('…');
    }
    out
}

/// `path` relative to `cwd` when it is inside it, else as given.
fn shown_path(path: &str, cwd: Option<&str>) -> String {
    if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
        let prefix = if cwd.ends_with('/') { cwd.to_owned() } else { format!("{cwd}/") };
        if let Some(rest) = path.strip_prefix(&prefix) {
            return rest.to_owned();
        }
    }
    path.to_owned()
}

fn lines<'a>(text: &'a str, tone: LineTone, prefix: &str) -> impl Iterator<Item = PendingLine> + 'a {
    let prefix = prefix.to_owned();
    text.lines().map(move |l| PendingLine { text: format!("{prefix}{l}"), tone })
}

/// The first `DETAIL_LINES` lines, with a last line saying how many more there were.
fn clip(mut detail: Vec<PendingLine>) -> Vec<PendingLine> {
    if detail.len() > DETAIL_LINES {
        let more = detail.len() - (DETAIL_LINES - 1);
        detail.truncate(DETAIL_LINES - 1);
        detail.push(PendingLine { text: format!("… {more} more lines"), tone: LineTone::Plain });
    }
    detail
}

/// Lines added and removed by an edit's `old_string` / `new_string` pairs.
fn edit_counts(input: &Value) -> (usize, usize) {
    let pair = |e: &Value| {
        let old = str_of(e, "old_string").map_or(0, |s| s.lines().count());
        let new = str_of(e, "new_string").map_or(0, |s| s.lines().count());
        (new, old)
    };
    match input.get("edits").and_then(Value::as_array) {
        Some(edits) => edits.iter().map(pair).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1)),
        None => pair(input),
    }
}

fn counts_text(added: usize, removed: usize) -> String {
    match (added, removed) {
        (0, 0) => String::new(),
        (a, 0) => format!(" (+{a})"),
        (0, r) => format!(" (−{r})"),
        (a, r) => format!(" (+{a} −{r})"),
    }
}

/// A `PermissionRequest`: the question, its detail, and the answers on offer ("Yes", what
/// Claude Code suggests remembering, "No"). `None` for `AskUserQuestion`, which its
/// `PreToolUse` asks instead.
pub fn permission(payload: &Value) -> Option<Ask> {
    let tool = str_of(payload, "tool_name").unwrap_or("a tool");
    if tool == ASK_TOOL {
        return None;
    }
    let input = payload.get("tool_input").cloned().unwrap_or(Value::Null);
    let cwd = str_of(payload, "cwd");
    let file = str_of(&input, "file_path")
        .or_else(|| str_of(&input, "notebook_path"))
        .map(|p| shown_path(p, cwd));
    let (text, detail, event) = match (tool, &file) {
        ("Edit" | "MultiEdit", Some(file)) => {
            let edits: Vec<Value> = match input.get("edits").and_then(Value::as_array) {
                Some(edits) => edits.clone(),
                None => vec![input.clone()],
            };
            let mut detail = Vec::new();
            for e in &edits {
                detail.extend(lines(str_of(e, "old_string").unwrap_or(""), LineTone::Remove, "− "));
                detail.extend(lines(str_of(e, "new_string").unwrap_or(""), LineTone::Add, "+ "));
            }
            (format!("Make this edit to {file}?"), detail, format!("Asked to edit {file}"))
        }
        ("Write", Some(file)) => {
            let detail = lines(str_of(&input, "content").unwrap_or(""), LineTone::Add, "+ ").collect();
            (format!("Write {file}?"), detail, format!("Asked to write {file}"))
        }
        ("NotebookEdit", Some(file)) => {
            let detail = lines(str_of(&input, "new_source").unwrap_or(""), LineTone::Add, "+ ").collect();
            (format!("Edit the notebook {file}?"), detail, format!("Asked to edit {file}"))
        }
        ("Bash", _) => {
            let command = str_of(&input, "command").unwrap_or("");
            let text = match str_of(&input, "description").filter(|d| !d.is_empty()) {
                Some(d) => format!("Run this command? {}", one_line(d, LINE_MAX)),
                None => "Run this command?".to_owned(),
            };
            let detail = lines(command, LineTone::Plain, "").collect();
            (text, detail, format!("Asked to run {}", one_line(command, LINE_MAX)))
        }
        ("WebFetch", _) => {
            let url = str_of(&input, "url").unwrap_or("");
            (format!("Fetch {url}?"), Vec::new(), format!("Asked to fetch {url}"))
        }
        _ => {
            let shown = one_line(&input.to_string(), LINE_MAX);
            let detail = if input.is_null() { Vec::new() } else { vec![PendingLine { text: shown, tone: LineTone::Plain }] };
            (format!("Use {tool}?"), detail, format!("Asked to use {tool}"))
        }
    };

    let allow = |extra: Option<&Value>| {
        let mut decision = json!({ "behavior": "allow" });
        if let Some(s) = extra {
            decision["updatedPermissions"] = json!([s]);
        }
        json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": decision } })
    };
    let mut options = vec!["Yes".to_owned()];
    let mut replies = vec![allow(None)];
    let suggestions = payload.get("permission_suggestions").and_then(Value::as_array);
    for s in suggestions.into_iter().flatten().take(OPTIONS_MAX - 2) {
        options.push(suggestion_label(s));
        replies.push(allow(Some(s)));
    }
    options.push("No, tell Claude what to do".to_owned());
    replies.push(json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": {
        "behavior": "deny",
        "message": "The user said no. Stop and wait for them to say what to do instead.",
        "interrupt": true,
    } } }));
    Some(Ask { kind: PendingKind::Permission, text, detail: clip(detail), options, replies, event })
}

/// How Manager words one of Claude Code's `permission_suggestions`.
fn suggestion_label(s: &Value) -> String {
    match (str_of(s, "type"), str_of(s, "mode")) {
        (Some("setMode"), Some("acceptEdits")) => "Allow all edits this session".to_owned(),
        (Some("setMode"), Some("bypassPermissions")) => "Allow everything this session".to_owned(),
        (Some("addRules"), _) => {
            let rule = s.get("rules").and_then(Value::as_array).and_then(|r| r.first());
            let tool = rule.and_then(|r| str_of(r, "toolName")).unwrap_or("this");
            match rule.and_then(|r| str_of(r, "ruleContent")).filter(|c| !c.is_empty()) {
                Some(content) => format!("Yes, don't ask again for {tool}({})", one_line(content, 40)),
                None => format!("Yes, don't ask again for {tool}"),
            }
        }
        (Some("addDirectories"), _) => "Yes, and allow this directory".to_owned(),
        _ => "Yes, and remember".to_owned(),
    }
}

/// A `PreToolUse` for `AskUserQuestion` with one question to pick one option of: the question
/// and its options, each answered through `updatedInput`. `None` for anything else (several
/// questions, several picks, another tool): the agent asks in its Terminal.
pub fn question(payload: &Value) -> Option<Ask> {
    if str_of(payload, "tool_name") != Some(ASK_TOOL) {
        return None;
    }
    let input = payload.get("tool_input")?;
    let questions = input.get("questions")?.as_array()?;
    let [q] = questions.as_slice() else { return None };
    if q.get("multiSelect").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let text = str_of(q, "question")?.to_owned();
    let labels: Vec<String> = q
        .get("options")?
        .as_array()?
        .iter()
        .filter_map(|o| str_of(o, "label").map(str::to_owned))
        .take(OPTIONS_MAX)
        .collect();
    if labels.is_empty() {
        return None;
    }
    let replies = labels
        .iter()
        .map(|label| {
            let mut updated = input.clone();
            updated["answers"] = json!({ text.clone(): label });
            json!({ "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "updatedInput": updated,
            } })
        })
        .collect();
    Some(Ask {
        kind: PendingKind::Question,
        event: format!("Asked: {}", one_line(&text, LINE_MAX)),
        text,
        detail: Vec::new(),
        options: labels,
        replies,
    })
}

/// A line of the feed for `UserPromptSubmit`, `PostToolUse` and `Stop`; `None` for anything else.
pub fn event(name: &str, payload: &Value) -> Option<(AgentEventKind, String)> {
    let cwd = str_of(payload, "cwd");
    match name {
        "UserPromptSubmit" => {
            let prompt = str_of(payload, "prompt").unwrap_or("");
            Some((AgentEventKind::Started, format!("Started “{}”", one_line(prompt, LINE_MAX))))
        }
        "Stop" => Some((AgentEventKind::Idle, "Idle at prompt".to_owned())),
        "PostToolUse" => Some(tool_used(payload, cwd)),
        _ => None,
    }
}

/// Whether a `PostToolUse` says its tool failed or was interrupted.
fn failed(payload: &Value) -> bool {
    payload.get("tool_response").is_some_and(|r| {
        r.get("is_error").and_then(Value::as_bool).unwrap_or(false)
            || r.get("interrupted").and_then(Value::as_bool).unwrap_or(false)
    })
}

/// What a hook says of the agent's turn, for the Journal (`journal.rs`): counts, not words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// The user gave it a prompt of `len` characters: a turn begins.
    Prompt { len: u32 },
    /// It used a tool.
    Tool(ToolUse),
    /// It asked the user something, and waits.
    Asked,
    /// It has its answer.
    Answered,
    /// It stopped, back at its prompt, or ended: the turn is over.
    Stop,
}

/// The kinds of tool a turn counts its uses by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolKind {
    /// It changed a file.
    Edit,
    /// It read a file or searched the checkout.
    Read,
    /// It fetched a page or searched the web.
    Web,
    /// It ran a command.
    Run,
    /// It ran a subagent.
    Agent,
    /// A tool of an MCP server.
    Mcp,
    Other,
}

/// One use of a tool, as a turn counts it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolUse {
    pub kind: ToolKind,
    pub failed: bool,
    /// Lines an edit added and removed.
    pub added: u32,
    pub removed: u32,
    /// The file an edit changed.
    pub file: Option<String>,
    /// The program a command ran ([`program`]).
    pub program: Option<String>,
}

/// Longest program name a turn counts; anything longer is not a program someone typed.
const PROGRAM_MAX: usize = 24;

/// The program a command line runs first, by name: `cargo` of `RUSTFLAGS=-g /usr/bin/cargo test`.
/// `None` when its first word does not read as one (a subshell, a quoted path).
pub fn program(command: &str) -> Option<String> {
    let is_assignment = |w: &str| {
        w.split_once('=').is_some_and(|(name, _)| {
            !name.is_empty()
                && !name.starts_with(|c: char| c.is_ascii_digit())
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    };
    let word = command.split_whitespace().find(|w| !is_assignment(w))?;
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+');
    if !word.chars().all(|c| plain(c) || c == '/') {
        return None;
    }
    let name = word.rsplit('/').next()?;
    (!name.is_empty() && name.len() <= PROGRAM_MAX).then(|| name.to_owned())
}

/// The step of a turn that hook `name` reports; `None` for a hook that is none.
pub fn step(name: &str, payload: &Value) -> Option<Step> {
    match name {
        "UserPromptSubmit" => {
            let len = str_of(payload, "prompt").map_or(0, |p| p.chars().count());
            Some(Step::Prompt { len: len as u32 })
        }
        "PostToolUse" if str_of(payload, "tool_name") == Some(ASK_TOOL) => Some(Step::Answered),
        "PostToolUse" => Some(Step::Tool(tool_use(payload))),
        "Stop" | "SessionEnd" => Some(Step::Stop),
        _ => None,
    }
}

fn tool_use(payload: &Value) -> ToolUse {
    let tool = str_of(payload, "tool_name").unwrap_or("");
    let input = payload.get("tool_input").cloned().unwrap_or(Value::Null);
    let kind = match tool {
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => ToolKind::Edit,
        "Read" | "Grep" | "Glob" => ToolKind::Read,
        "WebFetch" | "WebSearch" => ToolKind::Web,
        "Bash" => ToolKind::Run,
        "Task" | "Agent" => ToolKind::Agent,
        t if t.starts_with("mcp__") => ToolKind::Mcp,
        _ => ToolKind::Other,
    };
    let (added, removed) = match tool {
        "Edit" | "MultiEdit" => edit_counts(&input),
        "Write" => (str_of(&input, "content").map_or(0, |c| c.lines().count()), 0),
        _ => (0, 0),
    };
    let file = str_of(&input, "file_path").or_else(|| str_of(&input, "notebook_path"));
    ToolUse {
        kind,
        failed: failed(payload),
        added: added as u32,
        removed: removed as u32,
        file: file.filter(|_| kind == ToolKind::Edit).map(str::to_owned),
        program: str_of(&input, "command").filter(|_| kind == ToolKind::Run).and_then(program),
    }
}

fn tool_used(payload: &Value, cwd: Option<&str>) -> (AgentEventKind, String) {
    let tool = str_of(payload, "tool_name").unwrap_or("a tool");
    let input = payload.get("tool_input").cloned().unwrap_or(Value::Null);
    let file = str_of(&input, "file_path")
        .or_else(|| str_of(&input, "notebook_path"))
        .map(|p| shown_path(p, cwd));
    let failed = failed(payload);
    let (kind, text) = match (tool, &file) {
        ("Edit" | "MultiEdit", Some(file)) => {
            let (a, r) = edit_counts(&input);
            (AgentEventKind::Edit, format!("Updated {file}{}", counts_text(a, r)))
        }
        ("Write", Some(file)) => {
            let n = str_of(&input, "content").map_or(0, |c| c.lines().count());
            (AgentEventKind::Edit, format!("Wrote {file}{}", counts_text(n, 0)))
        }
        ("NotebookEdit", Some(file)) => (AgentEventKind::Edit, format!("Updated {file}")),
        ("Read", Some(file)) => (AgentEventKind::Read, format!("Read {file}")),
        ("Grep" | "Glob", _) => {
            let pattern = str_of(&input, "pattern").unwrap_or("");
            (AgentEventKind::Read, format!("Searched for {}", one_line(pattern, LINE_MAX)))
        }
        ("WebFetch", _) => (AgentEventKind::Read, format!("Fetched {}", str_of(&input, "url").unwrap_or(""))),
        ("WebSearch", _) => {
            let query = str_of(&input, "query").unwrap_or("");
            (AgentEventKind::Read, format!("Searched the web for {}", one_line(query, LINE_MAX)))
        }
        ("Bash", _) => {
            let command = str_of(&input, "command").unwrap_or("");
            (AgentEventKind::Command, format!("Ran {}", one_line(command, LINE_MAX)))
        }
        ("Task" | "Agent", _) => {
            let what = str_of(&input, "description").unwrap_or("a subagent");
            (AgentEventKind::Command, format!("Ran a subagent: {}", one_line(what, LINE_MAX)))
        }
        (ASK_TOOL, _) => (AgentEventKind::Answered, "Got an answer to its question".to_owned()),
        ("TodoWrite", _) => (AgentEventKind::Command, "Updated its todo list".to_owned()),
        _ => (AgentEventKind::Command, format!("Used {tool}")),
    };
    if failed {
        (AgentEventKind::Failed, format!("{text} · failed"))
    } else {
        (kind, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edit_asks_with_its_diff_and_the_suggestions() {
        let payload = json!({
            "hook_event_name": "PermissionRequest",
            "cwd": "/src/app",
            "tool_name": "Edit",
            "tool_input": {
                "file_path": "/src/app/core/session.rs",
                "old_string": "if token != expected { return Err(Denied) }",
                "new_string": "verify_pairing(token, &store)?;",
            },
            "permission_suggestions": [{ "type": "setMode", "mode": "acceptEdits", "destination": "session" }],
        });
        let ask = permission(&payload).unwrap();
        assert_eq!(ask.kind, PendingKind::Permission);
        assert_eq!(ask.text, "Make this edit to core/session.rs?");
        assert_eq!(ask.event, "Asked to edit core/session.rs");
        assert_eq!(ask.detail[0].tone, LineTone::Remove);
        assert_eq!(ask.detail[1], PendingLine { text: "+ verify_pairing(token, &store)?;".into(), tone: LineTone::Add });
        assert_eq!(ask.options, ["Yes", "Allow all edits this session", "No, tell Claude what to do"]);
        assert_eq!(ask.replies.len(), 3);
        assert_eq!(ask.replies[0]["hookSpecificOutput"]["decision"]["behavior"], "allow");
        assert_eq!(ask.replies[1]["hookSpecificOutput"]["decision"]["updatedPermissions"][0]["mode"], "acceptEdits");
        assert_eq!(ask.replies[2]["hookSpecificOutput"]["decision"]["behavior"], "deny");
    }

    #[test]
    fn a_long_detail_is_clipped() {
        let content: String = (0..20).map(|i| format!("line {i}\n")).collect();
        let payload = json!({ "tool_name": "Write", "tool_input": { "file_path": "/x/a.txt", "content": content } });
        let ask = permission(&payload).unwrap();
        assert_eq!(ask.detail.len(), DETAIL_LINES);
        assert_eq!(ask.detail.last().unwrap().text, "… 13 more lines");
    }

    #[test]
    fn ask_user_question_is_asked_by_its_pre_tool_use_only() {
        let payload = json!({
            "tool_name": "AskUserQuestion",
            "tool_input": { "questions": [{
                "question": "Which fix?",
                "header": "Fix",
                "multiSelect": false,
                "options": [{ "label": "Await the write", "description": "" }, { "label": "Poll in the test", "description": "" }],
            }] },
        });
        assert_eq!(permission(&payload), None);
        let ask = question(&payload).unwrap();
        assert_eq!(ask.kind, PendingKind::Question);
        assert_eq!(ask.text, "Which fix?");
        assert_eq!(ask.options, ["Await the write", "Poll in the test"]);
        let out = &ask.replies[1]["hookSpecificOutput"];
        assert_eq!(out["permissionDecision"], "allow");
        assert_eq!(out["updatedInput"]["answers"]["Which fix?"], "Poll in the test");
        assert_eq!(out["updatedInput"]["questions"][0]["header"], "Fix");
    }

    #[test]
    fn questions_manager_cannot_answer_stay_in_the_terminal() {
        let q = |multi: bool| json!({ "question": "Q?", "multiSelect": multi, "options": [{ "label": "A" }] });
        let two = json!({ "tool_name": "AskUserQuestion", "tool_input": { "questions": [q(false), q(false)] } });
        let multi = json!({ "tool_name": "AskUserQuestion", "tool_input": { "questions": [q(true)] } });
        assert_eq!(question(&two), None);
        assert_eq!(question(&multi), None);
        assert_eq!(question(&json!({ "tool_name": "Bash" })), None);
    }

    #[test]
    fn tools_used_read_as_lines_of_the_feed() {
        let used = |tool: &str, input: Value| event("PostToolUse", &json!({ "cwd": "/r", "tool_name": tool, "tool_input": input }));
        assert_eq!(
            used("Edit", json!({ "file_path": "/r/a.rs", "old_string": "a\nb", "new_string": "a\nb\nc" })),
            Some((AgentEventKind::Edit, "Updated a.rs (+3 −2)".into()))
        );
        assert_eq!(
            used("Bash", json!({ "command": "cargo test -p core\n--more" })),
            Some((AgentEventKind::Command, "Ran cargo test -p core…".into()))
        );
        assert_eq!(used("Read", json!({ "file_path": "/elsewhere/b.rs" })), Some((AgentEventKind::Read, "Read /elsewhere/b.rs".into())));
        let failed = event("PostToolUse", &json!({ "tool_name": "Bash", "tool_input": { "command": "false" }, "tool_response": { "interrupted": true } }));
        assert_eq!(failed, Some((AgentEventKind::Failed, "Ran false · failed".into())));
        assert_eq!(
            event("UserPromptSubmit", &json!({ "prompt": "Show Codex usage windows" })),
            Some((AgentEventKind::Started, "Started “Show Codex usage windows”".into()))
        );
        assert_eq!(event("SessionStart", &json!({})), None);
    }

    #[test]
    fn hooks_read_as_steps_of_a_turn() {
        assert_eq!(step("UserPromptSubmit", &json!({ "prompt": "Fix the “é”" })), Some(Step::Prompt { len: 11 }));
        assert_eq!(step("Stop", &json!({})), Some(Step::Stop));
        assert_eq!(step("SessionEnd", &json!({})), Some(Step::Stop));
        assert_eq!(step("SessionStart", &json!({})), None);
        assert_eq!(step("PostToolUse", &json!({ "tool_name": "AskUserQuestion" })), Some(Step::Answered));
        let used = |tool: &str, input: Value| match step("PostToolUse", &json!({ "tool_name": tool, "tool_input": input })) {
            Some(Step::Tool(u)) => u,
            other => panic!("{other:?}"),
        };
        let edit = used("Edit", json!({ "file_path": "/r/a.rs", "old_string": "a\nb", "new_string": "a\nb\nc" }));
        assert_eq!((edit.kind, edit.added, edit.removed, edit.file.as_deref()), (ToolKind::Edit, 3, 2, Some("/r/a.rs")));
        let write = used("Write", json!({ "file_path": "/r/b.rs", "content": "1\n2" }));
        assert_eq!((write.kind, write.added, write.removed), (ToolKind::Edit, 2, 0));
        let read = used("Read", json!({ "file_path": "/r/a.rs" }));
        assert_eq!((read.kind, read.file), (ToolKind::Read, None), "a file read is not a file changed");
        let run = used("Bash", json!({ "command": "cargo test -p core" }));
        assert_eq!((run.kind, run.program.as_deref()), (ToolKind::Run, Some("cargo")));
        assert_eq!(used("Agent", json!({ "description": "x" })).kind, ToolKind::Agent);
        assert_eq!(used("WebSearch", json!({ "query": "x" })).kind, ToolKind::Web);
        assert_eq!(used("mcp__linear__get_issue", json!({})).kind, ToolKind::Mcp);
        assert_eq!(used("TodoWrite", json!({})).kind, ToolKind::Other);
        let failed = step("PostToolUse", &json!({ "tool_name": "Bash", "tool_input": { "command": "false" }, "tool_response": { "is_error": true } }));
        assert!(matches!(failed, Some(Step::Tool(ToolUse { failed: true, .. }))));
    }

    #[test]
    fn a_command_is_counted_by_its_program() {
        assert_eq!(program("cargo test").as_deref(), Some("cargo"));
        assert_eq!(program("  RUSTFLAGS=-g CI=1 /usr/bin/cargo test").as_deref(), Some("cargo"));
        assert_eq!(program("./scripts/demo/run.sh --fast").as_deref(), Some("run.sh"));
        assert_eq!(program("cd /r && make").as_deref(), Some("cd"), "the first one only");
        assert_eq!(program("(cd /r; make)"), None);
        assert_eq!(program("\"/my tools/x\" y"), None);
        assert_eq!(program("FOO=1"), None);
        assert_eq!(program(""), None);
        assert_eq!(program(&"x".repeat(PROGRAM_MAX + 1)), None);
    }
}
