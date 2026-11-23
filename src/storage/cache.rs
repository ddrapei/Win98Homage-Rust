//! Photos already read down, kept on disk: what a frame's child made of a
//! photo, so that the next time the round comes to it -- every round after
//! the first -- it is read back from a small file in a moment, instead of
//! decoded again, which costs from a twentieth to a third of a second of the
//! processor (and a child to do it in).
//!
//! An entry is found by its key: the photo's path, its size and the time it
//! was last written, what it was read down to (the frame's look, its size
//! and the colour under transparent parts), and this program's own size and
//! time.  A photo changed on disk, or a program built since -- which may read
//! photos down another way -- finds nothing, and the photo is read afresh.
//! The key is kept in the entry and checked whole, so two keys whose file
//! names clash are never taken for each other.
//!
//! The entries are kept to `MOST` bytes in all: past that, the ones longest
//! unused go first.  The folder holds nothing else, and can be deleted at any
//! time.  What is in an entry after its key is not this module's business:
//! the frame hands over bytes, and asks for as many back.

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::settings;

/// The most the entries take on disk, in all: some five hundred Dither
/// Frame photos, or two thousand of the Photo Frame's.
const MOST: u64 = 32 << 20;

/// What an entry's file name ends in.
const EXTENSION: &str = "px";

/// Where the entries are kept: with the program's other files of its own.
pub fn dir() -> PathBuf {
    settings::data_dir().join("Photo cache")
}

/// What an entry is found by (see the head of this file): its bytes, which
/// begin with their own length, so that no key is the start of another.
pub struct Key(Vec<u8>);

impl Key {
    /// The key of the photo at `path` read down by `look` to `w` x `h` on
    /// `bg`; None if the file's size and time, or this program's, cannot be
    /// had -- the photo is then read as if there were no entries at all.
    pub fn of(path: &Path, look: &str, w: usize, h: usize, bg: u32) -> Option<Key> {
        let meta = std::fs::metadata(path).ok()?;
        let mut key = program()?.to_vec();
        key.extend_from_slice(look.as_bytes());
        key.push(0);
        for n in [w as u64, h as u64, bg as u64, meta.len(), nanos(meta.modified().ok()?)?] {
            key.extend_from_slice(&n.to_le_bytes());
        }
        for unit in path.as_os_str().encode_wide() {
            key.extend_from_slice(&unit.to_le_bytes());
        }
        let mut whole = (key.len() as u32).to_le_bytes().to_vec();
        whole.extend(key);
        Some(Key(whole))
    }

    /// The entry's file name: the key's hash.
    fn name(&self) -> String {
        format!("{:016x}.{EXTENSION}", fnv1a(&self.0))
    }
}

/// The `len` bytes kept under `key` in `dir`, if there are those; and the
/// entry is then the last to go.
pub fn load(dir: &Path, key: &Key, len: usize) -> Option<Vec<u8>> {
    let path = dir.join(key.name());
    let bytes = std::fs::read(&path).ok()?;
    let body = bytes.strip_prefix(key.0.as_slice()).filter(|body| body.len() == len)?;
    let _ = std::fs::File::options().write(true).open(&path).and_then(|f| f.set_modified(SystemTime::now()));
    Some(body.to_vec())
}

/// Keep `body` under `key` in `dir`, and the entries there to `MOST` bytes.
/// Nothing is said if it cannot be kept: the photo is only read again.
pub fn store(dir: &Path, key: &Key, body: &[u8]) {
    store_within(dir, key, body, MOST);
}

fn store_within(dir: &Path, key: &Key, body: &[u8], most: u64) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let mut bytes = Vec::with_capacity(key.0.len() + body.len());
    bytes.extend_from_slice(&key.0);
    bytes.extend_from_slice(body);
    if settings::write_whole(&dir.join(key.name()), &bytes).is_ok() {
        prune(dir, most);
    }
}

/// Past `most` bytes of entries in all, the longest unused go -- down to
/// three quarters of it, so that one more is not followed at once by
/// another pruning.
fn prune(dir: &Path, most: u64) {
    let Ok(listing) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<(SystemTime, u64, PathBuf)> = listing
        .flatten()
        .filter(|e| Path::new(&e.file_name()).extension().is_some_and(|x| x == EXTENSION))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((meta.modified().ok()?, meta.len(), e.path()))
        })
        .collect();
    let mut total: u64 = entries.iter().map(|e| e.1).sum();
    if total <= most {
        return;
    }
    entries.sort();
    for (_, len, path) in entries {
        if total <= most / 4 * 3 {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

/// This program's size and the time it was built, once: in every key, so
/// that another build of it finds none of this one's entries.
fn program() -> Option<&'static [u8]> {
    static STAMP: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    STAMP
        .get_or_init(|| {
            let meta = std::fs::metadata(std::env::current_exe().ok()?).ok()?;
            let mut stamp = meta.len().to_le_bytes().to_vec();
            stamp.extend_from_slice(&nanos(meta.modified().ok()?)?.to_le_bytes());
            Some(stamp)
        })
        .as_deref()
}

fn nanos(t: SystemTime) -> Option<u64> {
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_nanos() as u64)
}

/// FNV-1a, 64 bits: the same name for the same key in every build.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("win98-cache-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn age(path: &Path, seconds_ago: u64) {
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(seconds_ago)).unwrap();
    }

    #[test]
    fn a_photo_read_down_once_is_read_back_while_it_is_as_it_was() {
        let dir = folder("back");
        let (photo, kept) = (dir.join("a.jpg"), dir.join("kept"));
        std::fs::write(&photo, b"a photo").unwrap();
        let key = Key::of(&photo, "photo", 4, 2, 0xFF00_0000).unwrap();
        assert_eq!(load(&kept, &key, 16), None);
        let body: Vec<u8> = (0..16).collect();
        store(&kept, &key, &body);
        assert_eq!(load(&kept, &Key::of(&photo, "photo", 4, 2, 0xFF00_0000).unwrap(), 16), Some(body.clone()));
        // asked for another length, it is not taken
        assert_eq!(load(&kept, &key, 15), None);
        // read down another way -- the other look, another size, another
        // colour underneath -- there is nothing kept
        for other in [
            Key::of(&photo, "dither", 4, 2, 0xFF00_0000),
            Key::of(&photo, "photo", 2, 4, 0xFF00_0000),
            Key::of(&photo, "photo", 4, 2, 0xFFFF_FFFF),
        ] {
            assert_eq!(load(&kept, &other.unwrap(), 16), None);
        }
        // and once the photo is written again, nothing is kept for it
        age(&photo, 3600);
        assert_eq!(load(&kept, &Key::of(&photo, "photo", 4, 2, 0xFF00_0000).unwrap(), 16), None);
        std::fs::write(&photo, b"another photo").unwrap();
        assert_eq!(load(&kept, &Key::of(&photo, "photo", 4, 2, 0xFF00_0000).unwrap(), 16), None);
        // nor for a file that is not there
        assert!(Key::of(&dir.join("gone.jpg"), "photo", 4, 2, 0).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_entry_under_the_right_name_with_another_key_is_not_taken() {
        let dir = folder("clash");
        let kept = dir.join("kept");
        let (a, b) = (dir.join("a.jpg"), dir.join("b.jpg"));
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let (ka, kb) = (Key::of(&a, "photo", 1, 1, 0).unwrap(), Key::of(&b, "photo", 1, 1, 0).unwrap());
        // b's entry where a's would be, as if their names had clashed
        store(&kept, &kb, &[1, 2]);
        std::fs::rename(kept.join(kb.name()), kept.join(ka.name())).unwrap();
        assert_eq!(load(&kept, &ka, 2), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn past_the_most_the_longest_unused_go_first() {
        let dir = folder("prune");
        let kept = dir.join("kept");
        let keys: Vec<Key> = (0..6)
            .map(|i| {
                let photo = dir.join(format!("{i}.jpg"));
                std::fs::write(&photo, [i as u8]).unwrap();
                Key::of(&photo, "photo", 10, 10, 0).unwrap()
            })
            .collect();
        let body = [7u8; 200];
        let size = (keys[0].0.len() + body.len()) as u64;
        // five, each older than the next; then the oldest is used again
        for (i, key) in keys[..5].iter().enumerate() {
            store_within(&kept, key, &body, u64::MAX);
            age(&kept.join(key.name()), 100 - i as u64 * 10);
        }
        assert!(load(&kept, &keys[0], body.len()).is_some());
        // a sixth, with room for five: down to three, the longest unused gone
        store_within(&kept, &keys[5], &body, 5 * size);
        let left: Vec<bool> = keys.iter().map(|k| load(&kept, k, body.len()).is_some()).collect();
        assert_eq!(left, [true, false, false, false, true, true]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
