//! The `soundmap.*` commands (`docs/spec/sample-map.md` › Commands): analysis of the browser's
//! samples, layouts by how they sound, similar samples.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};

use loopwerk_library::Library;
use loopwerk_session::{Projects, Session, blank_project};
use serde_json::{Value, json};

fn temp(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("loopwerk-soundmap-cmd-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wav(path: &Path, samples: &[f32]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 22_050,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for s in samples {
        writer.write_sample((s.clamp(-1.0, 1.0) * 30_000.0) as i16).unwrap();
    }
    writer.finalize().unwrap();
}

fn kick(variant: usize) -> Vec<f32> {
    let mut phase = 0.0f32;
    (0..11_025)
        .map(|i| {
            let t = i as f32 / 22_050.0;
            phase += std::f32::consts::TAU * (50.0 + variant as f32 * 2.0 + 120.0 * (-t * 30.0).exp()) / 22_050.0;
            phase.sin() * (-t * 8.0).exp() * 0.8
        })
        .collect()
}

fn hat(variant: usize) -> Vec<f32> {
    let mut state = variant as u32 * 7919 + 1;
    (0..4_000)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32 - 0.5) * (-(i as f32) / 500.0).exp()
        })
        .collect()
}

fn projects(settings: &Path) -> Projects {
    let plugins = loopwerk_plugins::library::Library::new(Some(settings.join("plugins")), settings.join("no-host"));
    let mut projects = Projects::with_library(plugins);
    projects.set_file_library(Library::new(Some(settings.to_path_buf())));
    projects.add(Session::new(blank_project()), None).unwrap();
    projects
}

fn run(projects: &mut Projects, id: &str, params: Value) -> Result<Value, String> {
    projects.execute(id, &params).map_err(|e| e.0)
}

#[test]
fn samples_are_analysed_laid_out_and_compared() {
    let settings = temp("settings");
    let samples = temp("samples");
    for v in 0..10 {
        wav(&samples.join("Kicks").join(format!("kick {v}.wav")), &kick(v));
        wav(&samples.join("Hats").join(format!("hat {v}.wav")), &hat(v));
    }
    wav(&samples.join("thing.wav"), &kick(30));
    let mut p = projects(&settings);
    let folder = samples.display().to_string();
    run(&mut p, "library.addFolder", json!({ "folder": folder })).unwrap();

    // Nothing is analysed until asked.
    let status = run(&mut p, "soundmap.status", json!({})).unwrap();
    assert_eq!(status["started"], false);
    let none = run(&mut p, "soundmap.layout", json!({})).unwrap();
    assert_eq!((none["items"].as_array().unwrap().len(), none["pending"].as_u64()), (0, Some(21)));

    let queued = run(&mut p, "soundmap.analyse", json!({ "folder": folder })).unwrap();
    assert_eq!(queued["queued"], 21);
    p.wait_sound_map();
    let status = run(&mut p, "soundmap.status", json!({})).unwrap();
    assert_eq!(status["folders"][0]["analysed"], 21);

    let map = run(&mut p, "soundmap.layout", json!({ "text": "kick", "grid": true })).unwrap();
    let items = map["items"].as_array().unwrap();
    assert_eq!(items.len(), 21);
    let of = |name: &str| items.iter().find(|i| i["item"].as_str().unwrap().ends_with(name)).unwrap().clone();
    assert_eq!(of("kick 2.wav")["kind"], "kick");
    assert_eq!(of("kick 2.wav")["matches"], true);
    assert_eq!(of("hat 2.wav")["kind"], "hat");
    assert_eq!(of("hat 2.wav")["matches"], false);
    assert_eq!(of("thing.wav")["kind"], "kick");
    assert_eq!(of("thing.wav")["guessed"], true);
    assert!(of("hat 2.wav")["column"].is_u64());
    assert!(map["columns"].as_u64().unwrap() * map["rows"].as_u64().unwrap() >= 21);

    // A place narrows the scope.
    let kicks = samples.join("Kicks").display().to_string();
    let only = run(&mut p, "soundmap.layout", json!({ "place": kicks })).unwrap();
    assert_eq!(only["items"].as_array().unwrap().len(), 10);

    let kick0 = samples.join("Kicks").join("kick 0.wav").display().to_string();
    let similar = run(&mut p, "soundmap.similar", json!({ "item": kick0, "count": 5 })).unwrap();
    let similar = similar["items"].as_array().unwrap();
    assert_eq!(similar.len(), 5);
    assert!(similar.iter().all(|s| !s["item"].as_str().unwrap().contains("hat")), "{similar:?}");

    let info = run(&mut p, "soundmap.info", json!({ "item": kick0 })).unwrap();
    assert_eq!(
        (info["state"].as_str(), info["kind"].as_str(), info["kindFrom"].as_str()),
        (Some("analysed"), Some("kick"), Some("name"))
    );
    assert_eq!(info["features"].as_array().unwrap().len(), loopwerk_soundmap::features::FEATURE_COUNT);
    let thing = samples.join("thing.wav").display().to_string();
    let info = run(&mut p, "soundmap.info", json!({ "item": thing })).unwrap();
    assert_eq!((info["kind"].as_str(), info["kindFrom"].as_str()), (Some("kick"), Some("guess")));

    // Bad parameters are refused.
    assert!(run(&mut p, "soundmap.similar", json!({ "item": kick0, "count": 0 })).is_err());
    assert!(run(&mut p, "soundmap.similar", json!({ "item": "nowhere.wav" })).is_err());
    assert!(run(&mut p, "soundmap.layout", json!({ "maxSeconds": 0.5 })).is_err());
    assert!(run(&mut p, "soundmap.analyse", json!({ "folder": "nowhere" })).is_err());
    assert!(run(&mut p, "soundmap.info", json!({ "item": "nowhere.wav" })).is_err());

    // The analysis is kept: new projects with the same settings need none.
    drop(p);
    let mut again = projects(&settings);
    again.start_sound_map();
    let status = run(&mut again, "soundmap.status", json!({})).unwrap();
    assert_eq!(status["folders"][0]["analysed"], 21);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&samples);
}
