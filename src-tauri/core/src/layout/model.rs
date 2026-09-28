//! The layout as pure state: Groups, Tabs, their order and the active Tab, with every
//! transition a plain method that touches no pty and no file. `layout/mod.rs` wraps this in a
//! lock and adds the side effects (Session spawn and kill, the `layout` event, `layout.json`).
//! The rules are those of docs/architecture.md "v1 product defaults" (Naming, Interaction).
//!
//! A linked Tab (ADR 0003) is a Tab like any other here, ordered and moved with the rest, whose
//! Session lives on a paired Host: it never gets a Session of this Host's. `reconcile_links`
//! keeps one Host's links in line with the Tabs that Host has.

use crate::model::{Group, LayoutSnapshot, SessionId, Tab, TabLink};
use std::collections::{BTreeMap, HashSet};

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

/// What `reconcile_links` did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reconciled {
    /// The Tabs linked just now (the Host's Tabs that had no link), in the Host's order.
    pub linked: Vec<String>,
    /// The linked Tabs taken out: their Tab is gone from the Host.
    pub unlinked: Vec<Tab>,
}

/// A Tab-, Group- or id-shaped identifier: `tab_<32 hex>`, `group_<32 hex>`.
pub fn new_id(prefix: &str) -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{prefix}_{hex}")
}

/// A linked Tab: no Session, and no Title or cwd of its own (those are its Host's).
fn linked(id: String, group_id: String, link: TabLink) -> Tab {
    Tab {
        id,
        group_id,
        session_id: None,
        custom_title: None,
        last_cwd: None,
        link: Some(link),
    }
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

    /// The snapshot as this Host's own Remote clients get it: without linked Tabs, which they
    /// cannot reach through this Host (a phone paired to the Mac is not paired to the Mac's
    /// Hosts). A linked active Tab reads as none.
    pub fn snapshot_without_links(&self, revision: u64) -> LayoutSnapshot {
        LayoutSnapshot {
            revision,
            groups: self
                .groups
                .iter()
                .map(|g| Group {
                    tab_ids: g.tab_ids.iter().filter(|id| !self.is_linked(id)).cloned().collect(),
                    ..g.clone()
                })
                .collect(),
            tabs: self
                .tabs
                .iter()
                .filter(|(_, t)| !t.is_linked())
                .map(|(id, t)| (id.clone(), t.clone()))
                .collect(),
            active_tab_id: self.active_tab_id.clone().filter(|id| !self.is_linked(id)),
        }
    }

    /// Where `index`, counted among a Group's Tabs that are not linked (as a Remote client sees
    /// the Group), falls among all of them: at the unlinked Tab that is `index`th there, else
    /// the end.
    pub fn index_skipping_links(&self, group_id: &str, index: usize) -> usize {
        let Some(group) = self.group(group_id) else {
            return index;
        };
        group
            .tab_ids
            .iter()
            .enumerate()
            .filter(|(_, id)| !self.is_linked(id))
            .nth(index)
            .map_or(group.tab_ids.len(), |(i, _)| i)
    }

    /// Whether `tab_id` is a linked Tab.
    pub fn is_linked(&self, tab_id: &str) -> bool {
        self.tabs.get(tab_id).is_some_and(Tab::is_linked)
    }

    /// The Tab linked to Host `host_id`'s Tab `tab_id`, if there is one.
    pub fn linked_tab(&self, host_id: &str, tab_id: &str) -> Option<&Tab> {
        self.tabs
            .values()
            .find(|t| t.link.as_ref().is_some_and(|l| l.host_id == host_id && l.tab_id == tab_id))
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

    /// `placement`, but right after `after_tab_id` (in its Group) when the caller names a Tab.
    pub fn placement_after(
        &self,
        group_id: Option<&str>,
        after_tab_id: Option<&str>,
        cwd: Option<String>,
    ) -> Result<Placement, String> {
        let mut placement = self.placement(group_id, cwd)?;
        if let Some(after) = after_tab_id {
            let tab = self.tabs.get(after).ok_or_else(|| format!("no Tab {after}"))?;
            placement.group_id = tab.group_id.clone();
            placement.after = Some(after.to_owned());
        }
        Ok(placement)
    }

    /// Add `tab` to its Group after `after` (else at the end) and make it active.
    pub fn insert_tab(&mut self, tab: Tab, after: Option<&str>) -> Result<(), String> {
        let id = tab.id.clone();
        self.place_tab(tab, after)?;
        self.active_tab_id = Some(id);
        Ok(())
    }

    /// Add `tab` to its Group after `after` (else at the end), leaving the active Tab as it is.
    pub fn place_tab(&mut self, tab: Tab, after: Option<&str>) -> Result<(), String> {
        if self.tabs.contains_key(&tab.id) {
            return Err(format!("Tab {} exists", tab.id));
        }
        let group = self.group_mut(&tab.group_id)?;
        let at = after
            .and_then(|a| group.tab_ids.iter().position(|id| id == a))
            .map_or(group.tab_ids.len(), |i| i + 1);
        group.tab_ids.insert(at, tab.id.clone());
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

    // --- Linked Tabs ----------------------------------------------------------------------

    /// Link a paired Host's Tab in: a new linked Tab, placed as `placement_after` says and not
    /// made active. A Tab already linked stays where it is and is handed back.
    pub fn link_tab(&mut self, link: TabLink, group_id: Option<&str>, after_tab_id: Option<&str>) -> Result<Tab, String> {
        if let Some(tab) = self.linked_tab(&link.host_id, &link.tab_id) {
            return Ok(tab.clone());
        }
        let placement = self.placement_after(group_id, after_tab_id, None)?;
        let tab = linked(new_id("tab"), placement.group_id, link);
        self.place_tab(tab.clone(), placement.after.as_deref())?;
        Ok(tab)
    }

    /// Bring Host `host_id`'s links in line with the Tabs it has (`host_tabs`, in its order): a
    /// link whose Tab is gone goes, and each Tab with no link yet (made from elsewhere, or there
    /// before the Host was paired) is linked at the end of the first Group named `stray_group`,
    /// made at the end of the Groups when there is none. The active Tab moves only if it went.
    pub fn reconcile_links(&mut self, host_id: &str, host_tabs: &[String], stray_group: &str) -> Reconciled {
        let there: HashSet<&str> = host_tabs.iter().map(String::as_str).collect();
        let of_host = |t: &Tab| t.link.as_ref().filter(|l| l.host_id == host_id).map(|l| l.tab_id.clone());
        let gone: Vec<String> = self
            .tabs
            .values()
            .filter(|t| of_host(t).is_some_and(|id| !there.contains(id.as_str())))
            .map(|t| t.id.clone())
            .collect();
        let unlinked = gone.iter().filter_map(|id| self.remove_tab(id)).collect();

        let mut known: HashSet<String> = self.tabs.values().filter_map(of_host).collect();
        let strays: Vec<String> = host_tabs.iter().filter(|id| known.insert((*id).clone())).cloned().collect();
        let mut linked_ids = Vec::with_capacity(strays.len());
        if !strays.is_empty() {
            let name = stray_group.trim();
            let group_id = match self.groups.iter().find(|g| g.name == name) {
                Some(g) => g.id.clone(),
                None => self.new_group(Some(name)),
            };
            for tab_id in strays {
                let link = TabLink { host_id: host_id.to_owned(), tab_id };
                let tab = linked(new_id("tab"), group_id.clone(), link);
                linked_ids.push(tab.id.clone());
                // The Group is there and the id is fresh: placing cannot fail.
                let _ = self.place_tab(tab, None);
            }
        }
        Reconciled { linked: linked_ids, unlinked }
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
            link: None,
        }
    }

    fn link(host: &str, tab: &str) -> TabLink {
        TabLink { host_id: host.into(), tab_id: tab.into() }
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

    #[test]
    fn placing_a_tab_leaves_the_active_one() {
        let mut m = model();
        m.place_tab(tab("t5", "b"), Some("t4")).unwrap();
        assert_eq!(ids(&m, "b"), ["t4", "t5"]);
        assert_eq!(m.active_tab_id.as_deref(), Some("t2"));
        assert!(m.place_tab(tab("t5", "b"), None).is_err(), "ids are unique");
        assert!(m.place_tab(tab("t6", "nope"), None).is_err());
    }

    #[test]
    fn a_new_tab_can_go_right_after_a_named_tab_in_its_group() {
        let m = model();
        let p = m.placement_after(None, Some("t4"), None).unwrap();
        assert_eq!((p.group_id.as_str(), p.after.as_deref()), ("b", Some("t4")));
        let p = m.placement_after(Some("a"), Some("t4"), None).unwrap();
        assert_eq!(p.group_id, "b", "the named Tab's Group wins");
        assert!(m.placement_after(None, Some("nope"), None).is_err());
    }

    #[test]
    fn linking_places_a_hosts_tab_without_a_session_and_without_going_to_it() {
        let mut m = model();
        let t = m.link_tab(link("dell", "r1"), None, Some("t1")).unwrap();
        assert!(t.id.starts_with("tab_") && t.id != "r1", "a Tab id of this Host's");
        assert_eq!(t.session_id, None);
        assert_eq!(t.link, Some(link("dell", "r1")));
        assert_eq!(ids(&m, "a"), ["t1", t.id.as_str(), "t2", "t3"]);
        assert_eq!(m.active_tab_id.as_deref(), Some("t2"), "linking never changes the view");
        assert!(m.is_linked(&t.id) && !m.is_linked("t1"));
        assert_eq!(m.linked_tab("dell", "r1").map(|t| t.id.clone()), Some(t.id.clone()));
        assert!(m.linked_tab("other", "r1").is_none(), "links are per Host");

        // Linking it again hands back the same Tab, where it is.
        m.move_tab(&t.id, "b", None).unwrap();
        let again = m.link_tab(link("dell", "r1"), Some("a"), None).unwrap();
        assert_eq!(again.id, t.id);
        assert_eq!(m.tabs[&t.id].group_id, "b");

        // A linked Tab moves, activates and closes like any other.
        m.move_tab(&t.id, "a", Some(0)).unwrap();
        assert_eq!(ids(&m, "a")[0], t.id);
        assert!(m.activate(&t.id).unwrap());
        assert_eq!(m.remove_tab(&t.id).map(|t| t.link.is_some()), Some(true));
        assert_eq!(m.active_tab_id.as_deref(), Some("t1"), "its neighbour takes over");
    }

    #[test]
    fn a_hosts_tabs_without_a_link_land_at_the_end_of_a_group_named_after_it() {
        let mut m = model();
        let placed = m.link_tab(link("dell", "r2"), Some("a"), None).unwrap();
        let r = m.reconcile_links("dell", &["r1".into(), "r2".into(), "r3".into(), "r1".into()], "  bennet ");
        assert!(r.unlinked.is_empty());
        assert_eq!(r.linked.len(), 2, "r1 and r3; r2 was linked already, r1 counts once");
        let group = m.groups.last().unwrap().clone();
        assert_eq!(group.name, "bennet", "a new Group at the end");
        assert_eq!(group.tab_ids, r.linked);
        let hosted: Vec<&str> = r.linked.iter().map(|id| m.tabs[id].link.as_ref().unwrap().tab_id.as_str()).collect();
        assert_eq!(hosted, ["r1", "r3"], "in the Host's order");
        assert_eq!(m.tabs[&placed.id].group_id, "a", "a link the user placed stays put");
        assert_eq!(m.active_tab_id.as_deref(), Some("t2"));

        // Nothing new: nothing changes. A new stray joins the same Group, wherever it went.
        let all: Vec<String> = ["r1", "r2", "r3"].map(String::from).to_vec();
        assert_eq!(m.reconcile_links("dell", &all, "bennet"), Reconciled::default());
        m.move_group(&group.id, 0).unwrap();
        let r = m.reconcile_links("dell", &[all.clone(), vec!["r4".into()]].concat(), "bennet");
        assert_eq!(m.groups[0].tab_ids.last(), r.linked.first());
        assert_eq!(m.groups.len(), 3);

        // Renamed away: the next stray makes the Group again.
        let first = m.groups[0].id.clone();
        m.rename_group(&first, "Remote").unwrap();
        let r = m.reconcile_links("dell", &[all, vec!["r4".into(), "r5".into()]].concat(), "bennet");
        assert_eq!(m.groups.len(), 4);
        assert_eq!(m.groups[3].tab_ids, r.linked);
    }

    #[test]
    fn a_link_whose_tab_is_gone_from_its_host_goes() {
        let mut m = model();
        let r = m.reconcile_links("dell", &["r1".into(), "r2".into()], "dell");
        let (l1, l2) = (r.linked[0].clone(), r.linked[1].clone());
        m.reconcile_links("mini", &["r1".into()], "mini");
        m.activate(&l1).unwrap();

        let r = m.reconcile_links("dell", &["r2".into()], "dell");
        assert!(r.linked.is_empty());
        assert_eq!(r.unlinked.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), [l1.as_str()]);
        assert_eq!(m.active_tab_id.as_deref(), Some(l2.as_str()), "the active Tab went: its neighbour");
        assert!(m.linked_tab("mini", "r1").is_some(), "another Host's link with the same Tab id stays");

        // A Host removed: every link of it goes, and no Group is made.
        let groups = m.groups.len();
        let r = m.reconcile_links("dell", &[], "dell");
        assert_eq!(r.unlinked.len(), 1);
        assert!(m.linked_tab("dell", "r2").is_none());
        assert_eq!(m.groups.len(), groups);
        assert_eq!(ids(&m, "a"), ["t1", "t2", "t3"], "local Tabs are never touched");
    }

    #[test]
    fn remote_clients_see_the_layout_without_linked_tabs() {
        let mut m = model();
        let l = m.link_tab(link("dell", "r1"), None, Some("t1")).unwrap();
        m.activate(&l.id).unwrap();
        let s = m.snapshot_without_links(4);
        assert_eq!(s.revision, 4);
        assert_eq!(s.groups[0].tab_ids, ["t1", "t2", "t3"]);
        assert!(!s.tabs.contains_key(&l.id));
        assert_eq!(s.tabs.len(), 4);
        assert_eq!(s.active_tab_id, None, "a linked active Tab reads as none");
        m.activate("t3").unwrap();
        assert_eq!(m.snapshot_without_links(5).active_tab_id.as_deref(), Some("t3"));

        // Their indices count unlinked Tabs only: [t1, l, t2, t3].
        assert_eq!(m.index_skipping_links("a", 0), 0);
        assert_eq!(m.index_skipping_links("a", 1), 2, "before t2, past the link");
        assert_eq!(m.index_skipping_links("a", 3), 4, "past the last: the end");
        assert_eq!(m.index_skipping_links("nope", 2), 2);
    }
}
