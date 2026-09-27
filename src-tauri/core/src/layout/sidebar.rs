//! The sidebar as a phone shows it: the layout joined with each Session's facts, and a Title per
//! Tab. Pure. The Title follows docs/architecture.md "Naming" as far as a Host can: a rename,
//! else the agent's name, else the Foreground process when it is not the shell, else the cwd's
//! basename (`~` for home). The OSC title a running program sets is the Terminal's to read, so a
//! Host without one skips that rule (the core will scan output for it under #28); Agent status
//! and "finished" are a client's too, so they are left unset.

use super::model::Model;
use crate::model::{AgentKind, SessionId, SessionInfo, SidebarGroup, SidebarSnapshot, SidebarTab};
use std::collections::HashMap;

/// The automatic Title's name for an agent (`AGENT_NAMES` in `src/lib/agentStatus.ts`).
pub fn agent_name(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "Claude Code",
        AgentKind::Codex => "Codex",
        AgentKind::Gemini => "Gemini",
    }
}

fn basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/";
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

/// The Title a Host can derive for a Tab, given its Session's facts (if any) and the home dir.
pub fn title(
    custom_title: Option<&str>,
    last_cwd: Option<&str>,
    info: Option<&SessionInfo>,
    home: Option<&str>,
) -> String {
    if let Some(custom) = custom_title.filter(|t| !t.is_empty()) {
        return custom.to_owned();
    }
    if let Some(agent) = info.and_then(|i| i.agent) {
        return agent_name(agent).to_owned();
    }
    if let Some(fg) = info
        .filter(|i| !i.shell_is_foreground)
        .and_then(|i| i.foreground.as_deref())
    {
        return fg.to_owned();
    }
    let cwd = info.and_then(|i| i.cwd.as_deref()).or(last_cwd);
    match cwd {
        None => "~".to_owned(),
        Some(c) if home.is_some_and(|h| h == c) => "~".to_owned(),
        Some(c) => basename(c).to_owned(),
    }
}

pub fn build(
    model: &Model,
    facts: &HashMap<SessionId, SessionInfo>,
    home: Option<&str>,
) -> SidebarSnapshot {
    let groups = model
        .groups
        .iter()
        .map(|g| SidebarGroup {
            id: g.id.clone(),
            name: g.name.clone(),
            tabs: g
                .tab_ids
                .iter()
                .filter_map(|id| model.tabs.get(id))
                .map(|tab| {
                    let info = tab.session_id.and_then(|sid| facts.get(&sid));
                    SidebarTab {
                        id: tab.id.clone(),
                        session_id: tab.session_id,
                        title: title(tab.custom_title.as_deref(), tab.last_cwd.as_deref(), info, home),
                        agent: info.and_then(|i| i.agent),
                        status: None,
                        finished: false,
                        git: info.and_then(|i| i.git.clone()),
                        remote: info.is_some_and(|i| i.remote),
                    }
                })
                .collect(),
        })
        .collect();
    SidebarSnapshot {
        groups,
        active_tab_id: model.active_tab_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GitInfo, Group, Tab};

    fn info(id: SessionId) -> SessionInfo {
        SessionInfo {
            foreground: Some("zsh".into()),
            cwd: Some("/Users/you/Dev/jack".into()),
            ..SessionInfo::empty(id)
        }
    }

    #[test]
    fn the_title_follows_the_naming_rules_a_host_can_apply() {
        let home = Some("/Users/you");
        assert_eq!(title(Some("Build"), None, None, home), "Build", "a rename wins");
        assert_eq!(title(Some(""), Some("/Users/you/x"), None, home), "x", "an empty rename is none");
        let mut i = info(1);
        i.agent = Some(AgentKind::Claude);
        assert_eq!(title(None, None, Some(&i), home), "Claude Code");
        let mut i = info(1);
        i.shell_is_foreground = false;
        i.foreground = Some("vim".into());
        assert_eq!(title(None, None, Some(&i), home), "vim");
        assert_eq!(title(None, None, Some(&info(1)), home), "jack", "the shell's cwd basename");
        let mut i = info(1);
        i.cwd = Some("/Users/you".into());
        assert_eq!(title(None, None, Some(&i), home), "~");
        assert_eq!(title(None, Some("/tmp/work/"), None, home), "work", "no facts yet: the last cwd");
        assert_eq!(title(None, None, None, home), "~");
        assert_eq!(title(None, Some("/"), None, home), "/");
    }

    #[test]
    fn the_snapshot_lists_every_tab_in_group_order_with_its_facts() {
        let mut model = Model {
            groups: vec![
                Group { id: "g1".into(), name: "Work".into(), collapsed: false, tab_ids: vec![] },
                Group { id: "g2".into(), name: "Empty".into(), collapsed: true, tab_ids: vec![] },
            ],
            ..Model::default()
        };
        for (id, sid) in [("t1", Some(1)), ("t2", Some(2)), ("t3", None)] {
            model
                .insert_tab(
                    Tab {
                        id: id.into(),
                        group_id: "g1".into(),
                        session_id: sid,
                        custom_title: None,
                        last_cwd: Some("/Users/you/Dev/jack".into()),
                    },
                    None,
                )
                .unwrap();
        }
        model.activate("t2").unwrap();
        let git = GitInfo {
            repo_name: "jack".into(),
            common_dir: "/Users/you/Dev/jack/.git".into(),
            worktree_root: "/Users/you/Dev/jack".into(),
            worktree_name: None,
            branch: Some("main".into()),
            head_short: None,
        };
        let mut facts = HashMap::new();
        let mut one = info(1);
        one.agent = Some(AgentKind::Claude);
        one.git = Some(git.clone());
        facts.insert(1, one);
        let mut two = info(2);
        two.remote = true;
        two.shell_is_foreground = false;
        two.foreground = Some("ssh".into());
        facts.insert(2, two);

        let snap = build(&model, &facts, Some("/Users/you"));
        assert_eq!(snap.active_tab_id.as_deref(), Some("t2"));
        assert_eq!(snap.groups.len(), 2);
        assert_eq!(snap.groups[1].tabs, []);
        let tabs = &snap.groups[0].tabs;
        assert_eq!(tabs.len(), 3);
        assert_eq!(
            tabs[0],
            SidebarTab {
                id: "t1".into(),
                session_id: Some(1),
                title: "Claude Code".into(),
                agent: Some(AgentKind::Claude),
                status: None,
                finished: false,
                git: Some(git),
                remote: false,
            }
        );
        assert_eq!(tabs[1].title, "ssh");
        assert!(tabs[1].remote);
        assert_eq!(tabs[2].session_id, None, "a Tab without a Session still lists");
        assert_eq!(tabs[2].title, "jack", "from its last cwd");
    }
}
