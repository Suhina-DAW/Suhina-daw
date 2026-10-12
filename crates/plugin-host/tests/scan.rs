//! The plug-in scanner (`loopwerk_plugins::scan`) against the real `loopwerk-plugin-host --scan`.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use loopwerk_plugins::scan::{Catalog, Scanned, find_files, scan_file, standard_folders};

const PATIENT: Duration = Duration::from_secs(10);

fn host() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_loopwerk-plugin-host"))
}

/// A fresh, empty folder for one test.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loopwerk-scan-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

#[test]
fn reads_what_a_plugin_file_holds() {
    let Scanned::Plugins(plugins) = scan_file(&host(), "test:clap", PATIENT) else { panic!("no plug-ins") };
    let [gain] = plugins.as_slice() else { panic!("{plugins:?}") };
    assert_eq!((gain.id.as_str(), gain.name.as_str()), ("org.loopwerk.test.gain", "Loopwerk Test Gain"));
}

#[test]
fn a_crash_or_a_hang_while_loading_is_a_failure_not_a_stall() {
    let crashed = scan_file(&host(), "test:crash", PATIENT);
    assert!(matches!(&crashed, Scanned::Failed(why) if why.contains("crashed")), "{crashed:?}");

    let started = Instant::now();
    let hung = scan_file(&host(), "test:hang", Duration::from_millis(500));
    assert!(matches!(&hung, Scanned::Failed(why) if why.contains("hung")), "{hung:?}");
    // Far less than "forever"; process starts are slow on a busy machine (antivirus).
    assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());

    let dir = folder("not-a-plugin");
    let fake = dir.join("fake.clap");
    write(&fake, "not a plug-in");
    let failed = scan_file(&host(), &fake.display().to_string(), PATIENT);
    assert!(matches!(&failed, Scanned::Failed(why) if why.contains("could not load")), "{failed:?}");
    let missing = scan_file(Path::new("no-such-host"), "test:clap", PATIENT);
    assert!(matches!(&missing, Scanned::Failed(why) if why.contains("start")), "{missing:?}");
}

#[test]
fn finds_plugin_files_in_folders() {
    let dir = folder("find");
    write(&dir.join("a.clap"), "x");
    write(&dir.join("vendor/b.CLAP"), "x");
    write(&dir.join("vendor/readme.txt"), "x");
    write(&dir.join("1/2/3/4/5/6/7/deep.clap"), "x");
    write(&dir.join("1/2/3/4/5/6/7/8/9/too-deep.clap"), "x");
    // On macOS a plug-in is a folder named *.clap; it is found, not looked into.
    write(&dir.join("bundle.clap/Contents/inner.clap"), "x");
    let found: Vec<String> = find_files(std::slice::from_ref(&dir))
        .iter()
        .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(found, ["1/2/3/4/5/6/7/deep.clap", "a.clap", "bundle.clap", "vendor/b.CLAP"]);
    assert!(find_files(&[dir.join("no-such-folder")]).is_empty());
    assert!(standard_folders().iter().all(|f| f.is_dir()));
}

#[test]
fn a_scan_reads_only_new_and_changed_files() {
    let dir = folder("catalog");
    let (a, b) = (dir.join("a.clap"), dir.join("b.clap"));
    write(&a, "not a plug-in");
    write(&b, "not one either");
    let folders = [dir.clone()];
    let mut catalog = Catalog::new();

    let first = catalog.scan(&host(), &folders, PATIENT);
    assert_eq!((first.read.len(), first.failed.len(), first.unchanged), (2, 2, 0));
    assert_eq!(catalog.entries().len(), 2);
    assert_eq!(catalog.plugins().count(), 0);

    let second = catalog.scan(&host(), &folders, PATIENT);
    assert_eq!((second.read.len(), second.unchanged), (0, 2));

    write(&a, "not a plug-in, and longer now");
    std::fs::remove_file(&b).unwrap();
    let third = catalog.scan(&host(), &folders, PATIENT);
    assert_eq!(third.read, std::slice::from_ref(&a));
    assert_eq!(third.removed, [b]);
    assert_eq!(catalog.entries().len(), 1);

    let saved = Catalog::from_json(&catalog.to_json());
    assert_eq!(saved, catalog);
}

#[test]
fn a_saved_catalog_finds_plugins_by_id_or_name() {
    let json = r#"{"version":2,"entries":[
        {"path":"/plugins/fx.clap","modified":5,"size":7,"plugins":[
            {"id":"com.example.delay","name":"Echo Box","vendor":"Example","version":"1.2","features":["audio-effect","stereo"]},
            {"id":"com.example.verb","name":"Room","vendor":"Example","version":"1.2","features":[]}]},
        {"path":"/plugins/broken.clap","modified":1,"size":2,"failed":"crashed while loading"},
        {"path":"/plugins/odd.clap","modified":"x","size":2,"failed":"?"}]}"#;
    let catalog = Catalog::from_json(json);
    assert_eq!(catalog.entries().len(), 2);
    assert_eq!(catalog.plugins().count(), 2);
    let delay = catalog.find("com.example.delay").unwrap();
    assert_eq!(delay.features, ["audio-effect", "stereo"]);
    assert_eq!(delay.spec(), format!("clap:{}#com.example.delay", Path::new("/plugins/fx.clap").display()));
    assert_eq!(catalog.find("room").map(|p| p.id.as_str()), Some("com.example.verb"));
    assert!(catalog.find("nothing").is_none());
    assert_eq!(Catalog::from_json(&catalog.to_json()), catalog);
    // A catalog saved in the earlier format is dropped, so the next scan reads everything.
    assert_eq!(Catalog::from_json(&json.replace(r#""version":2"#, r#""version":1"#)).entries().len(), 0);

    for garbage in ["", "{", r#"{"version":2,"entries":[]}"#, r#"{"version":1}"#] {
        assert_eq!(Catalog::from_json(garbage), Catalog::new(), "{garbage}");
    }
}

#[test]
fn the_library_scans_its_folders_and_remembers_across_starts() {
    use loopwerk_plugins::library::Library;
    let dir = folder("library");
    write(&dir.join("plugins/fake.clap"), "not a plug-in");
    let mut library = Library::new(Some(dir.join("settings")), host()).without_standard_folders();
    assert!(library.add_folder(&dir.join("plugins")).unwrap());
    let first = library.scan(false).unwrap();
    assert_eq!((first.read.len(), first.failed.len()), (1, 1));

    // A new start loads the saved catalog: nothing to read again, unless asked to.
    let mut later = Library::new(Some(dir.join("settings")), host()).without_standard_folders();
    assert_eq!(later.catalog().entries().len(), 1);
    assert_eq!(later.scan(false).unwrap().unchanged, 1);
    assert_eq!(later.scan(true).unwrap().read.len(), 1);
}

#[test]
fn worker_processes_read_many_files_and_survive_crashes_and_hangs() {
    use loopwerk_plugins::scan_pool::scan_files;
    let files: Vec<String> =
        ["test:clap", "test:crash", "test:vst3", "test:hang", "test:clap", "test:vst3"].map(String::from).to_vec();
    let started = Instant::now();
    let results = scan_files(&host(), &files, 2, &|_| Duration::from_secs(3));
    assert_eq!(results.len(), files.len());
    let names: Vec<String> = results
        .iter()
        .map(|r| match r {
            Scanned::Plugins(p) => p.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(","),
            Scanned::Failed(why) => format!("failed: {}", why.split(':').next().unwrap_or_default()),
        })
        .collect();
    assert_eq!(names[0], "Loopwerk Test Gain");
    assert!(names[1].starts_with("failed: crashed"), "{names:?}");
    assert_eq!(names[2], "Loopwerk Test Gain");
    assert!(names[3].starts_with("failed: hung"), "{names:?}");
    assert_eq!(names[4..], ["Loopwerk Test Gain", "Loopwerk Test Gain"]);
    // The VST3 test plug-in's id is in the canonical text form.
    let Scanned::Plugins(vst3) = &results[2] else { panic!() };
    assert_eq!(vst3[0].id.len(), 32);
    // What its compatibility class says it replaces: a VST2 plug-in, by its derived class id.
    assert_eq!(vst3[0].replaces, ["5653544761696E6761696E0000000000"]);
    assert_eq!(vst3[0].replaces[0], loopwerk_plugins::matching::vst2_class_id(0x4761_696E, "Gain"));
    // One hang costs its timeout once, not per file. The room is for slow process starts: on
    // a loaded Windows machine each start can take seconds while the antivirus checks it.
    assert!(started.elapsed() < Duration::from_secs(60), "{:?}", started.elapsed());
}
