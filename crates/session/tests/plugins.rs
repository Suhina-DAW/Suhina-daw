//! The `plugins.*` commands: where scans look, what they found, and how the commands sit next
//! to the project commands. Scanning itself runs the plug-in host, so it is tested with the
//! host in `crates/plugin-host/tests/scan.rs`.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;

use loopwerk_model::Project;
use loopwerk_plugins::library::Library;
use loopwerk_session::{CommandKind, ProjectId, Projects, Session, plugin_commands};
use serde_json::{Value, json};

/// Projects whose plug-in library lives in a fresh folder of its own.
fn projects(name: &str) -> (Projects, PathBuf) {
    let dir = std::env::temp_dir().join(format!("loopwerk-plugin-commands-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("plugins")).unwrap();
    let library = Library::new(Some(dir.join("settings")), dir.join("no-such-host"));
    (Projects::with_library(library), dir)
}

fn run(projects: &mut Projects, id: &str, params: Value) -> Result<Value, String> {
    projects.execute(id, &params).map_err(|e| e.0)
}

#[test]
fn folders_are_added_listed_removed_and_kept() {
    let (mut p, dir) = projects("folders");
    let folder = dir.join("plugins");
    let added = run(&mut p, "plugins.addFolder", json!({ "folder": folder })).unwrap();
    assert_eq!(added["added"], true);
    assert_eq!(added["folders"].as_array().unwrap().len(), 1);
    let again = run(&mut p, "plugins.addFolder", json!({ "folder": folder })).unwrap();
    assert_eq!(again["added"], false);
    let listed = run(&mut p, "plugins.folders", json!({})).unwrap();
    assert_eq!(listed["added"].as_array().unwrap().len(), 1);
    assert!(listed["standard"].is_array());

    // A new start reads the saved folders back.
    let mut later = Projects::with_library(Library::new(Some(dir.join("settings")), dir.join("no-such-host")));
    let listed = run(&mut later, "plugins.folders", json!({})).unwrap();
    assert_eq!(listed["added"].as_array().unwrap().len(), 1);

    let removed = run(&mut later, "plugins.removeFolder", json!({ "folder": folder })).unwrap();
    assert_eq!((removed["removed"].clone(), removed["folders"].clone()), (json!(true), json!([])));
    let not_there = run(&mut later, "plugins.removeFolder", json!({ "folder": folder })).unwrap();
    assert_eq!(not_there["removed"], false);
}

#[test]
fn bad_folders_and_parameters_are_errors() {
    let (mut p, dir) = projects("bad");
    let missing = run(&mut p, "plugins.addFolder", json!({ "folder": dir.join("nope") })).unwrap_err();
    assert!(missing.contains("not a folder"), "{missing}");
    assert!(run(&mut p, "plugins.addFolder", json!({})).is_err());
    assert!(run(&mut p, "plugins.addFolder", json!({ "folder": 5 })).is_err());
    assert!(run(&mut p, "plugins.scan", json!({ "all": "yes" })).is_err());
    assert!(run(&mut p, "plugins.list", json!({ "extra": 1 })).is_err());
}

#[test]
fn nothing_scanned_lists_nothing_and_a_missing_host_is_an_error() {
    let (mut p, _) = projects("empty");
    assert_eq!(run(&mut p, "plugins.list", json!({})).unwrap(), json!({ "plugins": [], "failed": [] }));
    let error = run(&mut p, "plugins.scan", json!({})).unwrap_err();
    assert!(error.contains("plug-in host") && error.contains("missing"), "{error}");
}

#[test]
fn plugin_commands_belong_to_loopwerk_not_to_a_project() {
    assert!(plugin_commands().iter().all(|c| c.id.starts_with("plugins.") && c.kind == CommandKind::Plugins));
    let (mut p, _) = projects("kinds");
    let all = run(&mut p, "commands.list", json!({})).unwrap();
    let kinds: Vec<(&str, &str)> = all
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| Some((c["id"].as_str()?, c["kind"].as_str()?)))
        .filter(|(id, _)| id.starts_with("plugins."))
        .collect();
    assert_eq!(kinds.len(), plugin_commands().len());
    assert!(kinds.iter().all(|(_, kind)| *kind == "plugins"));

    // They run without any open project, and refuse a target project.
    assert!(p.is_empty());
    assert!(run(&mut p, "plugins.folders", json!({})).is_ok());
    let id: ProjectId = p.add(Session::new(Project::default()), None).unwrap();
    let error = p.execute_in(Some(id), "plugins.folders", &json!({})).unwrap_err().0;
    assert!(error.contains("not run on one project"), "{error}");
}
