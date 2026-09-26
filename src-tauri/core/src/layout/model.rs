//! The layout as pure state: Groups, Tabs, their order and the active Tab, with every
//! transition a plain method that touches no pty and no file. `layout/mod.rs` wraps this in a
//! lock and adds the side effects (Session spawn and kill, the `layout` event, `layout.json`).
//! The rules are those of docs/architecture.md "v1 product defaults" (Naming, Interaction).

use crate::model::{Group, LayoutSnapshot, SessionId, Tab};
use std::collections::BTreeMap;

/// A fresh install's one Group.
pub const DEFAULT_GROUP_NAME: &str = "Tabs";
/// What `group_new` names a Group when the caller gives no name.
pub const NEW_GROUP_NAME: &str = "New Group";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Model {
    pub groups: Vec<Group>,
    pub tabs: BTreeMap<String, Tab>,
    pub active_tab_id: Option<String>,
}

/// Where a new Tab goes and what it starts in, decided from the active Tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub group_id: String,
    /// The Tab it goes right after, or `None` for the end of the Group.
    pub after: Option<String>,
    /// The cwd to start in when the caller gives none.
    pub cwd: Option<String>,
}

/// A Tab-, Group- or id-shaped identifier: `tab_<32 hex>`, `group_<32 hex>`.
pub fn new_id(prefix: &str) -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{prefix}_{hex}")
}

impl Model {
    /// A fresh install: one Group named "Tabs", no Tabs (the owner adds the first).
    pub fn fresh() -> Self {
        Model {
            groups: vec![Group {
                id: new_id("group"),
                name: DEFAULT_GROUP_NAME.to_owned(),
                collapsed: false,
                tab_ids: Vec::new(),
            }],
            tabs: BTreeMap::new(),
            active_tab_id: None,
        }
    }

    pub fn snapshot(&self, revision: u64) -> LayoutSnapshot {
        LayoutSnapshot {
            revision,
            groups: self.groups.clone(),
            tabs: self.tabs.clone(),
            active_tab_id: self.active_tab_id.clone(),
        }
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.active_tab_id.as_ref().and_then(|id| self.tabs.get(id))
    }

    pub fn group(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    fn group_mut(&mut self, id: &str) -> Result<&mut Group, String> {
        self.groups
            .iter_mut()
            .find(|g| g.id == id)
            .ok_or_else(|| format!("no Group {id}"))
    }

    fn tab_mut(&mut self, id: &str) -> Result<&mut Tab, String> {
        self.tabs.get_mut(id).ok_or_else(|| format!("no Tab {id}"))
    }

    /// Tab ids top to bottom, collapsed Groups included.
    pub fn ordered_tab_ids(&self) -> Vec<String> {
        self.groups.iter().flat_map(|g| g.tab_ids.iter().cloned()).collect()
    }

    /// The Tab that takes over when `tab_id` goes: the next one down, else the one above.
    pub fn neighbour_of(&self, tab_id: &str) -> Option<String> {
        let order = self.ordered_tab_ids();
        let Some(idx) = order.iter().position(|id| id == tab_id) else {
            return order.first().cloned();
        };
        order
            .get(idx + 1)
            .or_else(|| idx.checked_sub(1).and_then(|i| order.get(i)))
            .cloned()
    }

    /// The Tab whose Session is `session_id`, if any.
    pub fn tab_of_session(&self, session_id: SessionId) -> Option<&Tab> {
        self.tabs.values().find(|t| t.session_id == Some(session_id))
    }

    // --- Tabs -----------------------------------------------------------------------------

    /// Where a new Tab goes: `group_id` if given, else the active Tab's Group, else the first
    /// Group; right after the active Tab when it is in that Group, else at the end; in
    /// `cwd`, else the active Tab's last cwd.
    pub fn placement(&self, group_id: Option<&str>, cwd: Option<String>) -> Result<Placement, String> {
        let active = self.active_tab();
        let group_id = match group_id {
            Some(id) => self.group(id).map(|g| g.id.clone()).ok_or_else(|| format!("no Group {id}"))?,
            None => active
                .map(|t| t.group_id.clone())
                .or_else(|| self.groups.first().map(|g| g.id.clone()))
                .ok_or("no Group to add a Tab to")?,
        };
        let after = active.filter(|t| t.group_id == group_id).map(|t| t.id.clone());
        Ok(Placement {
            group_id,
            after,
            cwd: cwd.or_else(|| active.and_then(|t| t.last_cwd.clone())),
        })
    }

    /// Add `tab` to its Group after `after` (else at the end) and make it active.
    pub fn insert_tab(&mut self, tab: Tab, after: Option<&str>) -> Result<(), String> {
        if self.tabs.contains_key(&tab.id) {
            return Err(format!("Tab {} exists", tab.id));
        }
        let group = self.group_mut(&tab.group_id)?;
        let at = after
            .and_then(|a| group.tab_ids.iter().position(|id| id == a))
            .map_or(group.tab_ids.len(), |i| i + 1);
        group.tab_ids.insert(at, tab.id.clone());
        self.active_tab_id = Some(tab.id.clone());
        self.tabs.insert(tab.id.clone(), tab);
        Ok(())
    }

    /// Take a Tab out. If it was active, its neighbour becomes active.
    pub fn remove_tab(&mut self, tab_id: &str) -> Option<Tab> {
        let tab = self.tabs.remove(tab_id)?;
        let next = (self.active_tab_id.as_deref() == Some(tab_id)).then(|| self.neighbour_of(tab_id));
        if let Some(group) = self.groups.iter_mut().find(|g| g.id == tab.group_id) {
            group.tab_ids.retain(|id| id != tab_id);
        }
        if let Some(next) = next {
            self.active_tab_id = next;
        }
        Some(tab)
    }

    /// A rename; an empty title clears it, back to the automatic Title.
    pub fn rename_tab(&mut self, tab_id: &str, title: &str) -> Result<(), String> {
        let trimmed = title.trim();
        self.tab_mut(tab_id)?.custom_title = (!trimmed.is_empty()).then(|| trimmed.to_owned());
        Ok(())
    }

    /// True when the cwd changed.
    pub fn set_last_cwd(&mut self, tab_id: &str, cwd: &str) -> bool {
        match self.tabs.get_mut(tab_id) {
            Some(tab) if tab.last_cwd.as_deref() != Some(cwd) => {
                tab.last_cwd = Some(cwd.to_owned());
                true
            }
            _ => false,
        }
    }

    pub fn set_session(&mut self, tab_id: &str, session_id: Option<SessionId>) -> Result<(), String> {
        self.tab_mut(tab_id)?.session_id = session_id;
        Ok(())
    }

    /// True when the active Tab changed.
    pub fn activate(&mut self, tab_id: &str) -> Result<bool, String> {
        if !self.tabs.contains_key(tab_id) {
            return Err(format!("no Tab {tab_id}"));
        }
        if self.active_tab_id.as_deref() == Some(tab_id) {
            return Ok(false);
        }
        self.active_tab_id = Some(tab_id.to_owned());
        Ok(true)
    }

    /// Move a Tab within or across Groups to `index` (default: the end of the Group).
    pub fn move_tab(&mut self, tab_id: &str, group_id: &str, index: Option<usize>) -> Result<(), String> {
        let source_id = self.tab_mut(tab_id)?.group_id.clone();
        self.group_mut(group_id)?;
        let same = source_id == group_id;
        let mut insert_at = index.unwrap_or(usize::MAX);
        if let Ok(source) = self.group_mut(&source_id) {
            let removed_at = source.tab_ids.iter().position(|id| id == tab_id);
            source.tab_ids.retain(|id| id != tab_id);
            if same && removed_at.is_some_and(|r| r < insert_at) {
                insert_at = insert_at.saturating_sub(1);
            }
        }
        let target = self.group_mut(group_id)?;
        let at = insert_at.min(target.tab_ids.len());
        target.tab_ids.insert(at, tab_id.to_owned());
        self.tab_mut(tab_id)?.group_id = group_id.to_owned();
        Ok(())
    }

    // --- Groups ---------------------------------------------------------------------------

    /// A new Group at the end; `None` names it "New Group".
    pub fn new_group(&mut self, name: Option<&str>) -> String {
        let id = new_id("group");
        let name = name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(NEW_GROUP_NAME);
        self.groups.push(Group {
            id: id.clone(),
            name: name.to_owned(),
            collapsed: false,
            tab_ids: Vec::new(),
        });
        id
    }

    /// Groups have no automatic name to fall back to: an empty name is ignored.
    pub fn rename_group(&mut self, group_id: &str, name: &str) -> Result<(), String> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            self.group_mut(group_id)?;
            return Ok(());
        }
        self.group_mut(group_id)?.name = trimmed.to_owned();
        Ok(())
    }

    /// True when it changed.
    pub fn set_collapsed(&mut self, group_id: &str, collapsed: bool) -> Result<bool, String> {
        let group = self.group_mut(group_id)?;
        if group.collapsed == collapsed {
            return Ok(false);
        }
        group.collapsed = collapsed;
        Ok(true)
    }

    /// Delete a Group and take its Tabs out (the owner kills their Sessions). Never the last
    /// Group. If the active Tab was in it, the first Tab overall becomes active.
    pub fn delete_group(&mut self, group_id: &str) -> Result<Vec<Tab>, String> {
        if self.groups.len() <= 1 {
            return Err("the last Group cannot be deleted".into());
        }
        let idx = self
            .groups
            .iter()
            .position(|g| g.id == group_id)
            .ok_or_else(|| format!("no Group {group_id}"))?;
        let group = self.groups.remove(idx);
        let tabs: Vec<Tab> = group.tab_ids.iter().filter_map(|id| self.tabs.remove(id)).collect();
        if self
            .active_tab_id
            .as_ref()
            .is_some_and(|a| group.tab_ids.contains(a))
        {
            self.active_tab_id = self.ordered_tab_ids().into_iter().next();
        }
        Ok(tabs)
    }

    pub fn move_group(&mut self, group_id: &str, index: usize) -> Result<(), String> {
        let idx = self
            .groups
            .iter()
            .position(|g| g.id == group_id)
            .ok_or_else(|| format!("no Group {group_id}"))?;
        let group = self.groups.remove(idx);
        let at = index.min(self.groups.len());
        self.groups.insert(at, group);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: &str, group: &str) -> Tab {
        Tab {
            id: id.into(),
            group_id: group.into(),
            session_id: None,
            custom_title: None,
            last_cwd: None,
        }
    }

    /// Two Groups, `a` with t1 t2 t3 (t2 active), `b` with t4.
    fn model() -> Model {
        let mut m = Model {
            groups: vec![
                Group { id: "a".into(), name: "Work".into(), collapsed: false, tab_ids: vec![] },
                Group { id: "b".into(), name: "Play".into(), collapsed: false, tab_ids: vec![] },
            ],
            ..Model::default()
        };
        for (id, g) in [("t1", "a"), ("t2", "a"), ("t3", "a"), ("t4", "b")] {
            m.insert_tab(tab(id, g), None).unwrap();
        }
        m.activate("t2").unwrap();
        m
    }

    fn ids<'a>(m: &'a Model, group: &str) -> Vec<&'a str> {
        m.group(group).unwrap().tab_ids.iter().map(String::as_str).collect()
    }

    #[test]
    fn ids_are_prefixed_and_unique() {
        let a = new_id("tab");
        let b = new_id("tab");
        assert!(a.starts_with("tab_") && a.len() == 4 + 32);
        assert_ne!(a, b);
    }

    #[test]
    fn a_fresh_model_has_one_group_named_tabs() {
        let m = Model::fresh();
        assert_eq!(m.groups.len(), 1);
        assert_eq!(m.groups[0].name, "Tabs");
        assert!(m.tabs.is_empty());
        assert_eq!(m.active_tab_id, None);
    }

    #[test]
    fn a_new_tab_joins_the_active_tabs_group_right_after_it_at_its_cwd() {
        let mut m = model();
        m.set_last_cwd("t2", "/work");
        let p = m.placement(None, None).unwrap();
        assert_eq!(p, Placement { group_id: "a".into(), after: Some("t2".into()), cwd: Some("/work".into()) });
        m.insert_tab(tab("t5", &p.group_id), p.after.as_deref()).unwrap();
        assert_eq!(ids(&m, "a"), ["t1", "t2", "t5", "t3"]);
        assert_eq!(m.active_tab_id.as_deref(), Some("t5"));

        // A cwd the caller names wins.
        assert_eq!(m.placement(None, Some("/x".into())).unwrap().cwd.as_deref(), Some("/x"));
    }

    #[test]
    fn a_new_tab_in_another_group_goes_to_its_end() {
        let mut m = model();
        let p = m.placement(Some("b"), None).unwrap();
        assert_eq!(p.after, None);
        m.insert_tab(tab("t5", "b"), None).unwrap();
        assert_eq!(ids(&m, "b"), ["t4", "t5"]);
        assert!(m.placement(Some("nope"), None).is_err());
    }

    #[test]
    fn with_no_active_tab_the_first_group_takes_the_new_tab() {
        let mut m = Model::fresh();
        let p = m.placement(None, None).unwrap();
        assert_eq!(p.group_id, m.groups[0].id);
        assert_eq!(p.after, None);
        assert!(Model::default().placement(None, None).is_err(), "no Group at all");
        m.insert_tab(tab("t", &p.group_id), None).unwrap();
        assert!(m.insert_tab(tab("t", &p.group_id), None).is_err(), "ids are unique");
    }

    #[test]
    fn removing_the_active_tab_activates_the_next_one_down_else_the_one_above() {
        let mut m = model();
        m.remove_tab("t2").unwrap();
        assert_eq!(ids(&m, "a"), ["t1", "t3"]);
        assert_eq!(m.active_tab_id.as_deref(), Some("t3"));
        m.remove_tab("t3");
        assert_eq!(m.active_tab_id.as_deref(), Some("t4"), "next down crosses Groups");
        m.remove_tab("t4");
        assert_eq!(m.active_tab_id.as_deref(), Some("t1"), "nothing below: the one above");
        m.remove_tab("t1");
        assert_eq!(m.active_tab_id, None);
        assert!(m.remove_tab("t1").is_none(), "gone already");
    }

    #[test]
    fn removing_a_background_tab_keeps_the_active_one() {
        let mut m = model();
        m.remove_tab("t4");
        assert_eq!(m.active_tab_id.as_deref(), Some("t2"));
        assert!(ids(&m, "b").is_empty());
    }

    #[test]
    fn renaming_trims_and_an_empty_title_restores_the_automatic_one() {
        let mut m = model();
        m.rename_tab("t1", "  Build  ").unwrap();
        assert_eq!(m.tabs["t1"].custom_title.as_deref(), Some("Build"));
        m.rename_tab("t1", "   ").unwrap();
        assert_eq!(m.tabs["t1"].custom_title, None);
        assert!(m.rename_tab("nope", "x").is_err());
    }

    #[test]
    fn last_cwd_and_session_follow_the_tab() {
        let mut m = model();
        assert!(m.set_last_cwd("t1", "/a"));
        assert!(!m.set_last_cwd("t1", "/a"), "unchanged");
        assert!(!m.set_last_cwd("nope", "/a"));
        m.set_session("t1", Some(7)).unwrap();
        assert_eq!(m.tab_of_session(7).map(|t| t.id.as_str()), Some("t1"));
        assert!(m.tab_of_session(8).is_none());
        assert!(m.set_session("nope", None).is_err());
    }

    #[test]
    fn activating_reports_whether_anything_changed() {
        let mut m = model();
        assert!(!m.activate("t2").unwrap());
        assert!(m.activate("t4").unwrap());
        assert_eq!(m.active_tab_id.as_deref(), Some("t4"));
        assert!(m.activate("nope").is_err());
    }

    #[test]
    fn moving_within_a_group_accounts_for_the_gap_it_leaves() {
        let mut m = model();
        m.move_tab("t1", "a", Some(2)).unwrap();
        assert_eq!(ids(&m, "a"), ["t2", "t1", "t3"], "index counted before removal");
        m.move_tab("t3", "a", Some(0)).unwrap();
        assert_eq!(ids(&m, "a"), ["t3", "t2", "t1"]);
        m.move_tab("t3", "a", None).unwrap();
        assert_eq!(ids(&m, "a"), ["t2", "t1", "t3"], "no index: the end");
        m.move_tab("t2", "a", Some(99)).unwrap();
        assert_eq!(ids(&m, "a"), ["t1", "t3", "t2"], "clamped");
    }

    #[test]
    fn moving_across_groups_updates_the_tabs_group() {
        let mut m = model();
        m.move_tab("t2", "b", Some(0)).unwrap();
        assert_eq!(ids(&m, "a"), ["t1", "t3"]);
        assert_eq!(ids(&m, "b"), ["t2", "t4"]);
        assert_eq!(m.tabs["t2"].group_id, "b");
        assert_eq!(m.active_tab_id.as_deref(), Some("t2"), "moving keeps it active");
        assert!(m.move_tab("nope", "b", None).is_err());
        assert!(m.move_tab("t1", "nope", None).is_err());
        assert_eq!(ids(&m, "a"), ["t1", "t3"], "a failed move changes nothing");
    }

    #[test]
    fn groups_are_made_renamed_collapsed_and_reordered() {
        let mut m = model();
        let c = m.new_group(None);
        assert_eq!(m.groups[2].name, "New Group");
        let d = m.new_group(Some("  Ops "));
        assert_eq!(m.groups[3].name, "Ops");
        m.rename_group(&c, " Chores ").unwrap();
        assert_eq!(m.group(&c).unwrap().name, "Chores");
        m.rename_group(&c, "  ").unwrap();
        assert_eq!(m.group(&c).unwrap().name, "Chores", "empty names are ignored");
        assert!(m.rename_group("nope", "x").is_err());

        assert!(m.set_collapsed("a", true).unwrap());
        assert!(!m.set_collapsed("a", true).unwrap());
        assert!(m.group("a").unwrap().collapsed);

        m.move_group(&d, 0).unwrap();
        let order: Vec<&str> = m.groups.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(order, [d.as_str(), "a", "b", c.as_str()]);
        m.move_group("a", 99).unwrap();
        assert_eq!(m.groups.last().unwrap().id, "a", "clamped to the end");
        assert!(m.move_group("nope", 0).is_err());
    }

    #[test]
    fn deleting_a_group_hands_back_its_tabs_and_never_deletes_the_last_one() {
        let mut m = model();
        let tabs = m.delete_group("a").unwrap();
        assert_eq!(tabs.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["t1", "t2", "t3"]);
        assert_eq!(m.groups.len(), 1);
        assert_eq!(m.tabs.len(), 1);
        assert_eq!(m.active_tab_id.as_deref(), Some("t4"), "the active Tab went with the Group");
        assert!(m.delete_group("b").is_err(), "the last Group stays");
        assert!(m.delete_group("nope").is_err());
    }

    #[test]
    fn deleting_a_background_group_keeps_the_active_tab() {
        let mut m = model();
        m.delete_group("b").unwrap();
        assert_eq!(m.active_tab_id.as_deref(), Some("t2"));
    }

    #[test]
    fn a_snapshot_carries_everything_in_order() {
        let m = model();
        let s = m.snapshot(3);
        assert_eq!(s.revision, 3);
        assert_eq!(s.groups, m.groups);
        assert_eq!(s.tabs.len(), 4);
        assert_eq!(s.active_tab_id.as_deref(), Some("t2"));
        assert_eq!(m.ordered_tab_ids(), ["t1", "t2", "t3", "t4"]);
        assert_eq!(m.neighbour_of("nope").as_deref(), Some("t1"), "unknown: the first");
    }
}
