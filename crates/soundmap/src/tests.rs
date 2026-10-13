use super::*;

/// A folder of generated samples, removed when dropped.
struct Library {
    root: PathBuf,
}

impl Library {
    fn new(name: &str) -> Self {
        let root = unique_dir(name);
        std::fs::create_dir_all(&root).unwrap();
        Library { root }
    }

    fn write(&self, relative: &str, samples: &[f32]) -> SourceFile {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for s in samples {
            writer.write_sample(*s).unwrap();
        }
        writer.finalize().unwrap();
        self.file(relative)
    }

    fn file(&self, relative: &str) -> SourceFile {
        let path = self.root.join(relative);
        let meta = std::fs::metadata(&path).unwrap();
        let modified = meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        SourceFile { folder: self.root.clone(), path, size: meta.len(), modified }
    }
}

/// A new folder name for every call, so runs whose process ids repeat never meet a folder
/// another run is still deleting.
pub(crate) fn unique_dir(name: &str) -> PathBuf {
    static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!("suhina-soundmap-{name}-{}-{nanos}-{n}", std::process::id()))
}

impl Drop for Library {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn noise(seed: u32, n: usize) -> Vec<f32> {
    let mut state = seed.max(1);
    (0..n)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f32 / u32::MAX as f32 - 0.5
        })
        .collect()
}

/// A kick: a falling low sine with a fast decay.
fn kick(variant: u32) -> Vec<f32> {
    let rate = 44_100.0;
    let mut phase = 0.0f32;
    (0..22_050)
        .map(|i| {
            let t = i as f32 / rate;
            let hz = 50.0 + variant as f32 * 2.0 + 120.0 * (-t * 30.0).exp();
            phase += std::f32::consts::TAU * hz / rate;
            phase.sin() * (-t * 8.0).exp() * 0.8
        })
        .collect()
}

/// A hat: short high noise.
fn hat(variant: u32) -> Vec<f32> {
    let raw = noise(variant + 11, 8_000);
    let mut out: Vec<f32> = raw.windows(2).map(|w| (w[1] - w[0]) * (1.0 - 0.0)).collect();
    for (i, s) in out.iter_mut().enumerate() {
        *s *= (-(i as f32) / 900.0).exp();
    }
    out
}

fn analysed(library: &Library, files: &[SourceFile]) -> SoundMap {
    let mut map = SoundMap::new(Some(library.root.join(".cache")), 2);
    map.update(files);
    map.wait();
    map
}

#[test]
fn a_library_is_analysed_laid_out_and_kinds_are_found() {
    let library = Library::new("layout");
    let mut files = Vec::new();
    for v in 0..12 {
        files.push(library.write(&format!("Kicks/kick {v}.wav"), &kick(v)));
        files.push(library.write(&format!("Hats/hat {v}.wav"), &hat(v)));
    }
    // Unnamed sounds in a folder with no kind.
    files.push(library.write("Misc/thing a.wav", &kick(20)));
    files.push(library.write("Misc/thing b.wav", &hat(20)));
    let mut map = analysed(&library, &files);
    let status = map.status(&files);
    assert_eq!(status, vec![FolderStatus { folder: library.root.clone(), analysed: 26, queued: 0, failed: 0 }]);

    let laid = map.map(&files, DEFAULT_MAX_SECONDS);
    assert_eq!(laid.points.len(), 26);
    assert_eq!(laid.pending, 0);
    let kind = |name: &str| laid.points.iter().find(|p| p.path.ends_with(name)).map(|p| (p.kind, p.guessed)).unwrap();
    assert_eq!(kind("kick 3.wav"), (Kind::Kick, false));
    assert_eq!(kind("hat 3.wav"), (Kind::Hat, false));
    assert_eq!(kind("thing a.wav"), (Kind::Kick, true));
    assert_eq!(kind("thing b.wav"), (Kind::Hat, true));

    // Similar items of a kick are kicks.
    let similar = laid.similar(&library.root.join("Kicks/kick 0.wav"), 8);
    assert_eq!(similar.len(), 8);
    assert!(
        similar.iter().all(|(p, _)| p.to_string_lossy().contains("kick") || p.ends_with("thing a.wav")),
        "{similar:?}"
    );

    // The same scope gives the same map, also in a new sound map that reads the kept layout.
    assert_eq!(map.map(&files, DEFAULT_MAX_SECONDS).points, laid.points);
    drop(map);
    let mut reopened = SoundMap::new(Some(library.root.join(".cache")), 1);
    reopened.update(&files);
    assert!(reopened.queued.is_empty());
    assert_eq!(reopened.map(&files, DEFAULT_MAX_SECONDS).points, laid.points);
    let grid = laid.grid(1.0);
    assert!(layout_from_bytes(&layout_to_bytes(&[[0.0, 1.0]])).is_some());
    assert!(layout_from_bytes(&[1, 2, 3]).is_none());
    assert!(layout_from_bytes(&layout_to_bytes(&[[2.0, 1.0]])).is_none());
    assert_eq!(grid.cells.len(), 26);
}

#[test]
fn the_cache_is_kept_and_follows_changes() {
    let library = Library::new("cache");
    let a = library.write("kick a.wav", &kick(1));
    let b = library.write("kick b.wav", &kick(2));
    let broken = library.root.join("broken.wav");
    std::fs::write(&broken, b"not audio").unwrap();
    let broken = library.file("broken.wav");
    let files = vec![a.clone(), b.clone(), broken.clone()];
    {
        let mut map = analysed(&library, &files);
        assert!(matches!(map.analysis(&broken), Some(Analysis::Failed(_))));
        assert!(matches!(map.analysis(&a), Some(Analysis::Analysed { .. })), "{:?}", map.analysis(&a));
        assert!(matches!(map.analysis(&b), Some(Analysis::Analysed { .. })), "{:?}", map.analysis(&b));
        let laid = map.map(&files, DEFAULT_MAX_SECONDS);
        assert_eq!((laid.points.len(), laid.failed), (2, 1));
    }
    // A new sound map reads the cache and queues nothing.
    let mut map = SoundMap::new(Some(library.root.join(".cache")), 1);
    map.update(&files);
    assert!(map.queued.is_empty());
    assert!(matches!(map.analysis(&a), Some(Analysis::Analysed { .. })));

    // A changed file is analysed again; a removed one is forgotten.
    let mut changed = b.clone();
    changed.modified += 1;
    map.update(&[a.clone(), changed.clone()]);
    assert!(map.queued.contains(&changed.path));
    map.wait();
    assert!(map.analysis(&changed).is_some());
    assert!(map.caches.values().all(|c| !c.entries.contains_key(Path::new("broken.wav"))));

    // Reanalysing clears the folder's cache and queues its files.
    map.reanalyse(&library.root, std::slice::from_ref(&a));
    assert!(map.queued.contains(&a.path));
    map.wait();
    assert!(map.analysis(&a).is_some());
}

#[test]
fn long_files_are_left_out_and_counted() {
    let library = Library::new("long");
    let short = library.write("kick.wav", &kick(1));
    let mut long_audio = kick(2);
    long_audio.resize(44_100 * 3, 0.0);
    let long = library.write("long kick.wav", &long_audio);
    let files = vec![short, long];
    let mut map = analysed(&library, &files);
    let laid = map.map(&files, 1.0);
    assert_eq!((laid.points.len(), laid.too_long), (1, 1));
    let laid = map.map(&files, 5.0);
    assert_eq!((laid.points.len(), laid.too_long), (2, 0));
}

#[test]
fn unanalysed_files_are_pending_and_an_empty_scope_is_empty() {
    let library = Library::new("pending");
    let file = library.write("kick.wav", &kick(1));
    let mut map = SoundMap::new(None, 1);
    let laid = map.map(&[file], DEFAULT_MAX_SECONDS);
    assert_eq!((laid.points.len(), laid.pending), (0, 1));
    assert!(map.map(&[], DEFAULT_MAX_SECONDS).points.is_empty());
    assert!(laid.similar(Path::new("nothing"), 5).is_empty());
}

/// How long large scopes take to lay out and snap (run with `--ignored --nocapture` in release).
#[test]
#[ignore]
fn timing_of_large_scopes() {
    for n in [1_000, 10_000, 50_000] {
        let rows: Vec<features::Features> = (0..n)
            .map(|i| {
                let values = noise(i as u32 + 1, features::FEATURE_COUNT);
                let mut row = [0.0; features::FEATURE_COUNT];
                for (k, (slot, v)) in row.iter_mut().zip(values).enumerate() {
                    *slot = v + ((i % 12) * (k % 5)) as f32;
                }
                row
            })
            .collect();
        let started = std::time::Instant::now();
        let s = Standardised::new(&rows);
        let positions = layout::layout(&s, 1);
        let laid = started.elapsed();
        let started = std::time::Instant::now();
        let grid = grid::snap(&positions, 1.0);
        let snapped = started.elapsed();
        let started = std::time::Instant::now();
        let similar = s.similar(0, 16);
        println!("{n} items: layout {laid:?}, grid {snapped:?}, similar {:?} ({})", started.elapsed(), similar.len());
        assert_eq!(grid.cells.len(), n);
    }
}
