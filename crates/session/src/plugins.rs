//! Commands for the plug-in library: scanning for installed plug-ins, listing them, and the
//! folders a scan looks in (ADR 0006). They belong to Loopwerk, not to one project, so they run
//! on [`Projects`] like the `projects.*` commands.

use std::path::Path;

use loopwerk_plugins::scan::{Report, Scanned};
use serde_json::{Value, json};

use crate::commands::{CommandKind, CommandSpec, Param, ParamType, optional, param};
use crate::{CommandError, Projects};

const FOLDER: Param = param("folder", ParamType::Text, "a folder that holds plug-ins");

static PLUGIN_COMMANDS: &[CommandSpec<Projects>] = &[
    CommandSpec {
        id: "plugins.list",
        label: "List the plug-ins the last scan found (id, name, vendor, version, features, file) and the files that failed",
        params: &[],
        kind: CommandKind::Plugins,
        run: list,
    },
    CommandSpec {
        id: "plugins.scan",
        label: "Look for installed plug-ins in the standard folders and the added ones; reads only new and changed files unless all is true",
        params: &[optional("all", ParamType::Bool, "read every file again, failed ones too (default false)")],
        kind: CommandKind::Plugins,
        run: scan,
    },
    CommandSpec {
        id: "plugins.folders",
        label: "List the folders a scan looks in: the system's standard ones and the ones added",
        params: &[],
        kind: CommandKind::Plugins,
        run: folders,
    },
    CommandSpec {
        id: "plugins.addFolder",
        label: "Add a folder for scans to look in, besides the standard ones",
        params: &[FOLDER],
        kind: CommandKind::Plugins,
        run: add_folder,
    },
    CommandSpec {
        id: "plugins.removeFolder",
        label: "Stop looking in a folder that was added",
        params: &[FOLDER],
        kind: CommandKind::Plugins,
        run: remove_folder,
    },
];

/// The commands for the plug-in library.
pub fn plugin_commands() -> &'static [CommandSpec<Projects>] {
    PLUGIN_COMMANDS
}

fn folder_param(params: &Value) -> Result<&Path, CommandError> {
    params
        .get("folder")
        .and_then(Value::as_str)
        .filter(|f| !f.is_empty())
        .map(Path::new)
        .ok_or_else(|| "folder must be a path".into())
}

fn paths(list: &[std::path::PathBuf]) -> Vec<String> {
    list.iter().map(|p| p.display().to_string()).collect()
}

fn list(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    let catalog = projects.library_mut().catalog();
    let plugins: Vec<Value> = catalog
        .plugins()
        .map(|p| {
            json!({
                "format": "clap",
                "id": p.id,
                "name": p.name,
                "vendor": p.vendor,
                "version": p.version,
                "features": p.features,
                "file": p.path.display().to_string(),
            })
        })
        .collect();
    let failed: Vec<Value> = catalog
        .entries()
        .iter()
        .filter_map(|e| match &e.scanned {
            Scanned::Failed(why) => Some(json!({ "file": e.path.display().to_string(), "reason": why })),
            Scanned::Plugins(_) => None,
        })
        .collect();
    Ok(json!({ "plugins": plugins, "failed": failed }))
}

fn report(report: &Report, found: usize) -> Value {
    json!({
        "plugins": found,
        "read": paths(&report.read),
        "unchanged": report.unchanged,
        "removed": paths(&report.removed),
        "failed": report.failed.iter().map(|(file, why)| json!({ "file": file.display().to_string(), "reason": why })).collect::<Vec<_>>(),
    })
}

fn scan(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let all = params.get("all").map(|v| v.as_bool().ok_or("all must be true or false")).transpose()?.unwrap_or(false);
    let library = projects.library_mut();
    let done = library.scan(all)?;
    let found = library.catalog().plugins().count();
    Ok(report(&done, found))
}

fn folders(projects: &mut Projects, _: &Value) -> Result<Value, CommandError> {
    let library = projects.library_mut();
    let custom = paths(library.custom_folders());
    Ok(json!({ "standard": paths(&loopwerk_plugins::scan::standard_folders()), "added": custom }))
}

fn add_folder(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let added = projects.library_mut().add_folder(folder_param(params)?)?;
    Ok(json!({ "added": added, "folders": paths(projects.library_mut().custom_folders()) }))
}

fn remove_folder(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let removed = projects.library_mut().remove_folder(folder_param(params)?)?;
    Ok(json!({ "removed": removed, "folders": paths(projects.library_mut().custom_folders()) }))
}
