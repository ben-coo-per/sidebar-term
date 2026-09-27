//! `layout.json`: the persisted layout, and the defensive read of whatever is on disk (stale,
//! hand-edited, or written by an older version). Version 2 holds only what the Host owns:
//! Groups (id, name, order, collapsed), Tabs (id, Group, order, custom Title, last cwd) and the
//! active Tab. Session ids are never persisted: Tabs respawn at their last cwd (ADR 0002).
//!
//! Version 1 (the webview's, `serialize()` in the old `src/lib/layout.svelte.ts`) also held the
//! sidebar width, the Panel and each Tab's unread mark. Those are a client's, so the one-time
//! migration moves them into `settings.json` under `sidebar` (the webview's section) instead of
//! keeping them here, and the file is rewritten as version 2.

use super::model::Model;
use crate::host::Paths;
use crate::model::{Group, Tab};
use crate::store;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub const VERSION: u32 = 2;

/// The file as written.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    pub version: u32,
    pub groups: Vec<Group>,
    pub tabs: Vec<PersistedTab>,
    pub active_tab_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTab {
    pub id: String,
    pub group_id: String,
    pub custom_title: Option<String>,
    pub last_cwd: Option<String>,
}

impl File {
    pub fn from_model(model: &Model) -> Self {
        File {
            version: VERSION,
            groups: model.groups.clone(),
            tabs: model
                .ordered_tab_ids()
                .iter()
                .filter_map(|id| model.tabs.get(id))
                .map(|t| PersistedTab {
                    id: t.id.clone(),
                    group_id: t.group_id.clone(),
                    custom_title: t.custom_title.clone(),
                    last_cwd: t.last_cwd.clone(),
                })
                .collect(),
            active_tab_id: model.active_tab_id.clone(),
        }
    }
}

/// What a read of the file yields.
#[derive(Debug, PartialEq, Eq)]
pub struct Parsed {
    pub model: Model,
    /// A version-1 file's presentation state (`sidebarWidth`, `panel`, the unread Tab ids),
    /// for the webview's `sidebar` settings section.
    pub client: Option<Value>,
}

fn string(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::to_owned)
}

/// Defensive parse of whatever `layout.json` holds: unknown JSON, possibly stale or hand-edited.
/// `None` means "start fresh". Every rule of the webview's `validateAndMigrate` holds: Tabs and
/// Groups without an id (or a Group without a name) are dropped, so are duplicate Group ids and
/// Tabs whose Group is gone; a Group lists only existing Tabs, each once; a Tab its Group
/// forgot is appended back; the active Tab must exist.
pub fn parse(raw: &Value) -> Option<Parsed> {
    let r = raw.as_object()?;
    let raw_groups = r.get("groups")?.as_array()?;
    let raw_tabs = r.get("tabs")?.as_array()?;
    let version = r.get("version").and_then(Value::as_u64).unwrap_or(1);

    let mut groups: Vec<Group> = Vec::new();
    let mut group_ids = HashSet::new();
    for g in raw_groups {
        let (Some(id), Some(name)) = (string(g.get("id")), string(g.get("name"))) else {
            continue;
        };
        if !group_ids.insert(id.clone()) {
            continue; // a duplicate id
        }
        let tab_ids = g
            .get("tabIds")
            .and_then(Value::as_array)
            .map(|ids| ids.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default();
        groups.push(Group {
            id,
            name,
            collapsed: g.get("collapsed").and_then(Value::as_bool).unwrap_or(false),
            tab_ids,
        });
    }
    if groups.is_empty() {
        return None;
    }

    let mut tabs: Vec<PersistedTab> = Vec::new();
    let mut unread: Vec<String> = Vec::new();
    for t in raw_tabs {
        let (Some(id), Some(group_id)) = (string(t.get("id")), string(t.get("groupId"))) else {
            continue;
        };
        if !group_ids.contains(&group_id) {
            continue; // its Group is gone
        }
        if t.get("unread") == Some(&Value::Bool(true)) {
            unread.push(id.clone());
        }
        tabs.push(PersistedTab {
            id,
            group_id,
            custom_title: string(t.get("customTitle")),
            last_cwd: string(t.get("lastCwd")),
        });
    }
    let valid: HashSet<&str> = tabs.iter().map(|t| t.id.as_str()).collect();

    // Every Group lists only existing Tabs, each exactly once.
    let mut claimed: HashSet<String> = HashSet::new();
    for g in &mut groups {
        g.tab_ids.retain(|id| valid.contains(id.as_str()) && claimed.insert(id.clone()));
    }
    // A Tab its Group forgot (a corrupt or truncated save) goes back at the end of it.
    for t in &tabs {
        if !claimed.contains(&t.id) {
            if let Some(g) = groups.iter_mut().find(|g| g.id == t.group_id) {
                g.tab_ids.push(t.id.clone());
                claimed.insert(t.id.clone());
            }
        }
    }
    let tabs: BTreeMap<String, Tab> = tabs
        .into_iter()
        .filter(|t| claimed.contains(&t.id))
        .map(|t| {
            let tab = Tab {
                id: t.id.clone(),
                group_id: t.group_id,
                session_id: None,
                custom_title: t.custom_title,
                last_cwd: t.last_cwd,
            };
            (t.id, tab)
        })
        .collect();
    let active_tab_id = string(r.get("activeTabId")).filter(|id| tabs.contains_key(id));

    let client = (version < 2).then(|| {
        let mut client = serde_json::Map::new();
        if let Some(width) = r.get("sidebarWidth").filter(|w| w.is_number()) {
            client.insert("width".into(), width.clone());
        }
        if let Some(panel) = r.get("panel").filter(|p| p.is_object()) {
            client.insert("panel".into(), panel.clone());
        }
        unread.retain(|id| tabs.contains_key(id));
        if !unread.is_empty() {
            client.insert("unread".into(), json!(unread));
        }
        Value::Object(client)
    });

    Some(Parsed {
        model: Model {
            groups,
            tabs,
            active_tab_id,
        },
        client,
    })
}

/// Read `layout.json` from the Host's data dir. `Ok(None)`: no file, or one to start fresh from.
pub fn load(paths: &dyn Paths) -> Result<Option<Parsed>, String> {
    Ok(store::load(paths, store::LAYOUT)?.as_ref().and_then(parse))
}

pub fn save(paths: &dyn Paths, model: &Model) -> Result<(), String> {
    let file = File::from_model(model);
    let value = serde_json::to_value(&file).map_err(|e| e.to_string())?;
    store::save(paths, store::LAYOUT, &value)
}

/// A version-1 file's presentation state becomes the webview's `sidebar` settings section, once:
/// an existing section is never touched (the webview has written it since, or a newer layout).
pub fn migrate_client_state(paths: &dyn Paths, client: &Value) -> Result<(), String> {
    let mut settings = store::load(paths, store::SETTINGS)?.unwrap_or_else(|| json!({}));
    let Some(map) = settings.as_object_mut() else {
        return Ok(()); // not ours to fix; the webview starts its settings fresh
    };
    if map.contains_key("sidebar") {
        return Ok(());
    }
    map.insert("sidebar".into(), client.clone());
    store::save(paths, store::SETTINGS, &settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::testutil::TempDir;
    use std::path::PathBuf;

    struct Dir(PathBuf);

    impl Paths for Dir {
        fn data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.0.clone())
        }
    }

    fn v1() -> Value {
        json!({
            "version": 1,
            "groups": [
                { "id": "g1", "name": "Work", "collapsed": false, "tabIds": ["t1", "t2"] },
                { "id": "g2", "name": "Play", "collapsed": true, "tabIds": [] }
            ],
            "tabs": [
                { "id": "t1", "groupId": "g1", "customTitle": "Build", "lastCwd": "/w", "unread": false },
                { "id": "t2", "groupId": "g1", "customTitle": null, "lastCwd": null, "unread": true }
            ],
            "activeTabId": "t2",
            "sidebarWidth": 300,
            "panel": { "view": "usage", "collapsed": true, "height": 200 }
        })
    }

    #[test]
    fn a_version_1_file_reads_whole_and_yields_its_presentation_state() {
        let p = parse(&v1()).expect("parses");
        assert_eq!(p.model.groups.len(), 2);
        assert_eq!(p.model.groups[0].tab_ids, ["t1", "t2"]);
        assert!(p.model.groups[1].collapsed);
        assert_eq!(p.model.tabs["t1"].custom_title.as_deref(), Some("Build"));
        assert_eq!(p.model.tabs["t1"].last_cwd.as_deref(), Some("/w"));
        assert_eq!(p.model.tabs["t2"].custom_title, None);
        assert_eq!(p.model.tabs["t2"].session_id, None);
        assert_eq!(p.model.active_tab_id.as_deref(), Some("t2"));
        assert_eq!(
            p.client,
            Some(json!({
                "width": 300,
                "panel": { "view": "usage", "collapsed": true, "height": 200 },
                "unread": ["t2"]
            }))
        );
    }

    #[test]
    fn a_version_2_file_round_trips_and_carries_no_presentation_state() {
        let p = parse(&v1()).unwrap();
        let file = File::from_model(&p.model);
        assert_eq!(file.version, VERSION);
        assert_eq!(file.tabs.len(), 2);
        let value = serde_json::to_value(&file).unwrap();
        assert!(value.get("sidebarWidth").is_none());
        assert!(value["tabs"][0].get("unread").is_none());
        assert!(value["tabs"][0].get("sessionId").is_none());
        let again = parse(&value).unwrap();
        assert_eq!(again.model, p.model);
        assert_eq!(again.client, None);
    }

    #[test]
    fn not_a_layout_starts_fresh() {
        assert!(parse(&json!(null)).is_none());
        assert!(parse(&json!("x")).is_none());
        assert!(parse(&json!({ "groups": [] , "tabs": []})).is_none(), "no Groups");
        assert!(parse(&json!({ "groups": "x", "tabs": [] })).is_none());
        assert!(parse(&json!({ "groups": [{ "id": "g", "name": "G" }] })).is_none(), "no tabs key");
    }

    #[test]
    fn bad_tabs_and_groups_are_dropped_and_the_rest_kept() {
        let raw = json!({
            "groups": [
                { "id": "g1", "name": "A", "tabIds": ["t1", 7, "ghost", "t1"] },
                { "id": "g1", "name": "dup" },
                { "name": "no id" },
                { "id": "g2" }
            ],
            "tabs": [
                { "id": "t1", "groupId": "g1", "customTitle": 5, "lastCwd": 5 },
                { "id": "t2", "groupId": "gone" },
                { "groupId": "g1" },
                "junk"
            ],
            "activeTabId": "t2"
        });
        let p = parse(&raw).unwrap();
        assert_eq!(p.model.groups.len(), 1);
        assert_eq!(p.model.groups[0].tab_ids, ["t1"], "non-strings, unknown and repeated ids dropped");
        assert_eq!(p.model.tabs.len(), 1);
        assert_eq!(p.model.tabs["t1"].custom_title, None, "a non-string Title is none");
        assert_eq!(p.model.tabs["t1"].last_cwd, None);
        assert_eq!(p.model.active_tab_id, None, "the active Tab must exist");
    }

    #[test]
    fn a_tab_its_group_forgot_is_appended_back_and_a_tab_claimed_twice_stays_once() {
        let raw = json!({
            "groups": [
                { "id": "g1", "name": "A", "tabIds": ["t2"] },
                { "id": "g2", "name": "B", "tabIds": ["t2", "t3"] }
            ],
            "tabs": [
                { "id": "t1", "groupId": "g1" },
                { "id": "t2", "groupId": "g1" },
                { "id": "t3", "groupId": "g2", "unread": true }
            ],
            "activeTabId": "t1"
        });
        let p = parse(&raw).unwrap();
        assert_eq!(p.model.groups[0].tab_ids, ["t2", "t1"]);
        assert_eq!(p.model.groups[1].tab_ids, ["t3"]);
        assert_eq!(p.model.active_tab_id.as_deref(), Some("t1"));
        assert_eq!(p.client, Some(json!({ "unread": ["t3"] })), "a v1 file with no width or Panel");
    }

    #[test]
    fn groups_with_no_tabs_are_kept() {
        let raw = json!({ "version": 2, "groups": [{ "id": "g", "name": "Empty" }], "tabs": [] });
        let p = parse(&raw).unwrap();
        assert_eq!(p.model.groups[0].name, "Empty");
        assert!(p.model.tabs.is_empty());
        assert_eq!(p.client, None);
    }

    #[test]
    fn the_presentation_state_moves_into_settings_once() {
        let dir = TempDir::new("layout-migrate");
        let paths = Dir(dir.path().to_path_buf());
        store::save(&paths, store::SETTINGS, &json!({ "version": 1, "hotkeys": { "x": 1 } })).unwrap();
        let client = json!({ "width": 300, "unread": ["t2"] });
        migrate_client_state(&paths, &client).unwrap();
        let settings = store::load(&paths, store::SETTINGS).unwrap().unwrap();
        assert_eq!(settings["hotkeys"]["x"], 1, "other sections kept");
        assert_eq!(settings["sidebar"], client);

        // Migrating again (an old layout.json copied back) never overwrites the webview's section.
        migrate_client_state(&paths, &json!({ "width": 180 })).unwrap();
        let settings = store::load(&paths, store::SETTINGS).unwrap().unwrap();
        assert_eq!(settings["sidebar"], client);
    }

    #[test]
    fn no_settings_file_yet_is_fine() {
        let dir = TempDir::new("layout-migrate-fresh");
        let paths = Dir(dir.path().to_path_buf());
        migrate_client_state(&paths, &json!({ "width": 240 })).unwrap();
        let settings = store::load(&paths, store::SETTINGS).unwrap().unwrap();
        assert_eq!(settings, json!({ "sidebar": { "width": 240 } }));
    }

    #[test]
    fn load_and_save_go_through_the_data_dir() {
        let dir = TempDir::new("layout-file");
        let paths = Dir(dir.path().to_path_buf());
        assert!(load(&paths).unwrap().is_none(), "no file yet");
        let model = parse(&v1()).unwrap().model;
        save(&paths, &model).unwrap();
        let back = load(&paths).unwrap().unwrap();
        assert_eq!(back.model, model);
        assert_eq!(back.client, None, "saved as version 2");
        std::fs::write(dir.path().join(store::LAYOUT), "{ nope").unwrap();
        assert!(load(&paths).unwrap().is_none(), "a corrupt file starts fresh");
    }
}
