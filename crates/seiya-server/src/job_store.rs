//! Bounded single-writer snapshots; never a model permission or replay queue.
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
const META: usize = 6 * 1024 * 1024;
const BLOBS: usize = 32 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"SEIYAJ01";
pub(crate) struct Store {
    root: PathBuf,
    _lock: File,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    manifest: Value,
    blobs: Vec<(String, usize)>,
}
pub(crate) type Loaded = (Value, BTreeMap<String, Vec<u8>>);
fn regular(path: &Path, cap: u64) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() && m.len() <= cap => {
            #[cfg(unix)]
            if m.permissions().mode() & 0o077 != 0 {
                return Err("snapshot_permissions".into());
            }
            Ok(Some(m))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        _ => Err("snapshot_regular_bounded_required".into()),
    }
}
impl Store {
    pub(crate) fn open(root: &Path) -> Result<Self, String> {
        if !root.is_absolute() || root.canonicalize().map_err(|_| "store_directory")? != root {
            return Err("canonical_private_directory_required".into());
        }
        let m = fs::symlink_metadata(root).map_err(|_| "store_directory")?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err("store_directory".into());
        }
        #[cfg(unix)]
        if m.permissions().mode() & 0o077 != 0 {
            return Err("store_directory_permissions".into());
        }
        let path = root.join("owner.lock");
        regular(&path, 0)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        options.mode(0o600);
        let lock = options.open(&path).map_err(|_| "store_lock_open")?;
        if !lock
            .metadata()
            .map_err(|_| "store_lock_metadata")?
            .is_file()
        {
            return Err("store_lock_type".into());
        }
        lock.try_lock().map_err(|_| "store_already_owned")?;
        Ok(Self {
            root: root.to_owned(),
            _lock: lock,
        })
    }
    pub(crate) fn load(&self) -> Result<Option<Loaded>, String> {
        let path = self.root.join("snapshot.bin");
        let Some(meta) = regular(&path, (META + BLOBS + 16) as u64)? else {
            return Ok(None);
        };
        let mut file = File::open(path).map_err(|_| "snapshot_open")?;
        if file.metadata().map_err(|_| "snapshot_metadata")?.len() != meta.len() {
            return Err("snapshot_changed".into());
        }
        let mut prefix = [0u8; 16];
        file.read_exact(&mut prefix)
            .map_err(|_| "snapshot_header")?;
        let len = u64::from_le_bytes(prefix[8..16].try_into().unwrap()) as usize;
        if &prefix[..8] != MAGIC || len > META {
            return Err("snapshot_version_or_size".into());
        }
        let mut bytes = vec![0; len];
        file.read_exact(&mut bytes)
            .map_err(|_| "snapshot_metadata_read")?;
        let h: Header = serde_json::from_slice(&bytes).map_err(|_| "snapshot_metadata_json")?;
        if h.version != 1 || h.blobs.len() > 8 {
            return Err("snapshot_version_or_blob_count".into());
        }
        let mut blobs = BTreeMap::new();
        let mut total = 0usize;
        for (id, len) in h.blobs {
            total = total.checked_add(len).ok_or("snapshot_blob_size")?;
            if uuid::Uuid::parse_str(&id).is_err()
                || blobs.contains_key(&id)
                || len > 16 * 1024 * 1024
                || total > BLOBS
            {
                return Err("snapshot_blob_size_or_id".into());
            }
            let mut bytes = vec![0; len];
            file.read_exact(&mut bytes)
                .map_err(|_| "snapshot_blob_read")?;
            blobs.insert(id, bytes);
        }
        let mut tail = [0];
        if file.read(&mut tail).map_err(|_| "snapshot_tail")? != 0 {
            return Err("snapshot_trailing_data".into());
        }
        Ok(Some((h.manifest, blobs)))
    }
    pub(crate) fn save(&self, manifest: Value, blobs: Vec<(String, &[u8])>) -> Result<(), String> {
        if blobs.len() > 8
            || blobs.iter().any(|(_, b)| b.len() > 16 * 1024 * 1024)
            || blobs.iter().map(|(_, b)| b.len()).sum::<usize>() > BLOBS
        {
            return Err("snapshot_blob_quota".into());
        }
        let header = serde_json::to_vec(&Header {
            version: 1,
            manifest,
            blobs: blobs.iter().map(|(id, b)| (id.clone(), b.len())).collect(),
        })
        .map_err(|_| "snapshot_encode")?;
        if header.len() > META {
            return Err("snapshot_metadata_quota".into());
        }
        let target = self.root.join("snapshot.bin");
        regular(&target, (META + BLOBS + 16) as u64)?;
        let temp = self
            .root
            .join(format!("snapshot-{}.tmp", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temp).map_err(|_| "snapshot_temp")?;
        let result = (|| -> std::io::Result<()> {
            file.write_all(MAGIC)?;
            file.write_all(&(header.len() as u64).to_le_bytes())?;
            file.write_all(&header)?;
            for (_, b) in blobs {
                file.write_all(b)?;
            }
            file.sync_all()?;
            fs::rename(&temp, &target)?;
            File::open(&self.root)?.sync_all()
        })()
        .map_err(|_| "snapshot_commit".to_owned());
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn directory() -> PathBuf {
        let p = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("seiya-history-fixture-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        p
    }
    #[test]
    fn roundtrip_atomic_replace_single_writer_and_truncation() {
        let p = directory();
        let s = Store::open(&p).unwrap();
        assert!(s.load().unwrap().is_none());
        assert!(Store::open(&p).is_err());
        let id = uuid::Uuid::new_v4().to_string();
        s.save(
            serde_json::json!({"fixture":1}),
            vec![(id.clone(), b"synthetic")],
        )
        .unwrap();
        let (m, b) = s.load().unwrap().unwrap();
        assert_eq!(m["fixture"], 1);
        assert_eq!(b[&id], b"synthetic");
        s.save(serde_json::json!({"fixture":2}), vec![]).unwrap();
        assert_eq!(s.load().unwrap().unwrap().0["fixture"], 2);
        let file = OpenOptions::new()
            .write(true)
            .open(p.join("snapshot.bin"))
            .unwrap();
        file.set_len(5).unwrap();
        assert!(s.load().is_err());
        drop(s);
        assert!(Store::open(&p).is_ok());
        fs::remove_dir_all(p).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn reject_symlink_and_public_permissions() {
        use std::os::unix::fs::symlink;
        let p = directory();
        let s = Store::open(&p).unwrap();
        let outside = p.join("outside");
        fs::write(&outside, b"never overwrite").unwrap();
        symlink(&outside, p.join("snapshot.bin")).unwrap();
        assert!(s.save(Value::Null, vec![]).is_err());
        assert!(s.load().is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"never overwrite");
        drop(s);
        fs::remove_dir_all(p).unwrap();
        let p = directory();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Store::open(&p).is_err());
        fs::remove_dir_all(p).unwrap();
    }
}
