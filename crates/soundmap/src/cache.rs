//! The feature cache of one library folder (ADR 0013 › Cache): `features-<hash>.bin` beside
//! the library's index.
//!
//! Layout, little-endian: the magic `LWSM`, the file version (u32), the feature version (u32),
//! the entry count (u32), then per entry: the path below the folder (u16 length, UTF-8), size
//! (u64), modification time (u64), a status byte (0 analysed, 1 failed) and, when analysed,
//! the file's length in seconds (f32) and [`FEATURE_COUNT`] f32 values; when failed, the reason
//! (u16 length, UTF-8).

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::features::{FEATURE_COUNT, FEATURE_VERSION, Features};

const MAGIC: &[u8; 4] = b"LWSM";
const FILE_VERSION: u32 = 1;
/// A cache file larger than this is not read (spec › Limits).
pub const MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024;

/// What is known of one file.
#[derive(Debug, Clone, PartialEq)]
pub enum Analysis {
    /// The features and the file's whole length in seconds.
    Analysed {
        features: Features,
        seconds: f32,
    },
    Failed(String),
}

/// One cached file: the size and time it had when analysed, and the result.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub size: u64,
    pub modified: u64,
    pub analysis: Analysis,
}

/// The cache of one folder, by path below the folder.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FolderCache {
    pub entries: HashMap<PathBuf, Entry>,
}

impl FolderCache {
    /// The entry for a file, when it was analysed at this size and time.
    pub fn current(&self, relative: &Path, size: u64, modified: u64) -> Option<&Analysis> {
        self.entries.get(relative).filter(|e| e.size == size && e.modified == modified).map(|e| &e.analysis)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FILE_VERSION.to_le_bytes());
        out.extend_from_slice(&FEATURE_VERSION.to_le_bytes());
        let mut entries: Vec<(&PathBuf, &Entry)> = self.entries.iter().filter(|(p, _)| p.to_str().is_some()).collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let entries: Vec<_> = entries
            .into_iter()
            .filter(|(p, e)| {
                p.to_str().is_some_and(|s| s.len() <= usize::from(u16::MAX))
                    && match &e.analysis {
                        Analysis::Failed(reason) => reason.len() <= usize::from(u16::MAX),
                        Analysis::Analysed { .. } => true,
                    }
            })
            .collect();
        out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for (path, entry) in entries {
            let path = path.to_str().unwrap_or_default();
            out.extend_from_slice(&(path.len() as u16).to_le_bytes());
            out.extend_from_slice(path.as_bytes());
            out.extend_from_slice(&entry.size.to_le_bytes());
            out.extend_from_slice(&entry.modified.to_le_bytes());
            match &entry.analysis {
                Analysis::Analysed { features, seconds } => {
                    out.push(0);
                    out.extend_from_slice(&seconds.to_le_bytes());
                    for v in features {
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                }
                Analysis::Failed(reason) => {
                    out.push(1);
                    out.extend_from_slice(&(reason.len() as u16).to_le_bytes());
                    out.extend_from_slice(reason.as_bytes());
                }
            }
        }
        out
    }

    /// Reads a cache. Anything malformed, of another version or too large gives `None`: the
    /// folder is then analysed again.
    pub fn from_bytes(bytes: &[u8]) -> Option<FolderCache> {
        if bytes.len() as u64 > MAX_CACHE_BYTES {
            return None;
        }
        let mut r = Reader { bytes, at: 0 };
        if r.take(4)? != MAGIC || r.u32()? != FILE_VERSION || r.u32()? != FEATURE_VERSION {
            return None;
        }
        let count = r.u32()? as usize;
        // Every entry takes at least 19 bytes, so a count the file cannot hold is refused before
        // anything is allocated for it.
        if count > bytes.len() / 19 {
            return None;
        }
        let mut entries = HashMap::with_capacity(count);
        for _ in 0..count {
            let length = usize::from(r.u16()?);
            let path = std::str::from_utf8(r.take(length)?).ok()?;
            let size = r.u64()?;
            let modified = r.u64()?;
            let analysis = match r.take(1)?.first()? {
                0 => {
                    let seconds = f32::from_le_bytes(r.take(4)?.try_into().ok()?);
                    if !seconds.is_finite() || seconds < 0.0 {
                        return None;
                    }
                    let mut features = [0.0f32; FEATURE_COUNT];
                    for slot in features.iter_mut() {
                        *slot = f32::from_le_bytes(r.take(4)?.try_into().ok()?);
                        if !slot.is_finite() {
                            return None;
                        }
                    }
                    Analysis::Analysed { features, seconds }
                }
                1 => {
                    let length = usize::from(r.u16()?);
                    Analysis::Failed(std::str::from_utf8(r.take(length)?).ok()?.to_string())
                }
                _ => return None,
            };
            entries.insert(PathBuf::from(path), Entry { size, modified, analysis });
        }
        (r.at == bytes.len()).then_some(FolderCache { entries })
    }

    /// Reads the cache at `path`; a missing or unreadable one is empty.
    pub fn load(path: &Path) -> FolderCache {
        match std::fs::metadata(path) {
            Ok(m) if m.len() <= MAX_CACHE_BYTES => {}
            _ => return FolderCache::default(),
        }
        std::fs::read(path).ok().and_then(|b| FolderCache::from_bytes(&b)).unwrap_or_default()
    }

    /// Writes the cache to a file beside `path` and moves it over `path`.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let temporary = path.with_extension("bin.tmp");
        {
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(&self.to_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&temporary, path)
    }
}

/// The cache file name of a folder: a stable hash of its path (FNV-1a), so it does not change
/// between runs or versions of Rust.
pub fn file_name(folder: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in folder.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("features-{hash:016x}.bin")
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FolderCache {
        let mut cache = FolderCache::default();
        let mut features = [0.0f32; FEATURE_COUNT];
        features.iter_mut().enumerate().for_each(|(i, v)| *v = i as f32 * 0.5 - 3.0);
        cache.entries.insert(
            PathBuf::from("Kicks/kick 1.wav"),
            Entry { size: 1234, modified: 99, analysis: Analysis::Analysed { features, seconds: 0.75 } },
        );
        cache.entries.insert(
            PathBuf::from("broken.wav"),
            Entry { size: 5, modified: 7, analysis: Analysis::Failed("no audio decoded".into()) },
        );
        cache
    }

    #[test]
    fn round_trips() {
        let cache = sample();
        assert_eq!(FolderCache::from_bytes(&cache.to_bytes()), Some(cache));
        let empty = FolderCache::default();
        assert_eq!(FolderCache::from_bytes(&empty.to_bytes()), Some(empty));
    }

    #[test]
    fn a_changed_file_is_not_current() {
        let cache = sample();
        let path = Path::new("Kicks/kick 1.wav");
        assert!(cache.current(path, 1234, 99).is_some());
        assert!(cache.current(path, 1234, 100).is_none());
        assert!(cache.current(path, 1235, 99).is_none());
        assert!(cache.current(Path::new("other.wav"), 1, 1).is_none());
    }

    #[test]
    fn malformed_caches_are_refused() {
        let bytes = sample().to_bytes();
        // Every truncation, and every single changed header byte.
        for end in 0..bytes.len() {
            assert_eq!(FolderCache::from_bytes(&bytes[..end]), None, "cut at {end}");
        }
        for i in 0..16 {
            let mut changed = bytes.clone();
            changed[i] ^= 0xff;
            assert_eq!(FolderCache::from_bytes(&changed), None, "byte {i}");
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(FolderCache::from_bytes(&trailing), None);
        // A huge count with no entries behind it allocates nothing and is refused.
        let mut huge = bytes[..12].to_vec();
        huge.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(FolderCache::from_bytes(&huge), None);
        // Arbitrary bytes never panic.
        let mut state = 3u32;
        for _ in 0..200 {
            let noise: Vec<u8> = (0..64)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state as u8
                })
                .collect();
            let mut with_header = bytes[..12].to_vec();
            with_header.extend_from_slice(&noise);
            let _ = FolderCache::from_bytes(&with_header);
        }
    }

    #[test]
    fn saves_and_loads() {
        let folder = crate::tests::unique_dir("cache-file");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(file_name(Path::new("/samples")));
        let cache = sample();
        cache.save(&path).unwrap();
        assert_eq!(FolderCache::load(&path), cache);
        std::fs::write(&path, b"garbage").unwrap();
        assert_eq!(FolderCache::load(&path), FolderCache::default());
        assert_eq!(FolderCache::load(&folder.join("missing.bin")), FolderCache::default());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn file_names_are_stable() {
        assert_eq!(file_name(Path::new("/samples")), file_name(Path::new("/samples")));
        assert_ne!(file_name(Path::new("/samples")), file_name(Path::new("/samples2")));
        assert!(file_name(Path::new("C:\\x")).starts_with("features-"));
    }
}
