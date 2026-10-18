//! Immutable, content-addressed inputs; never substitute current content for a missing snapshot.
use crate::source::{SourceText, read_source};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const MAX_STORE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparisonSpec {
    pub before: Option<Snapshot>,
    pub after: Option<Snapshot>,
    pub live_after: Option<String>,
    pub before_label: String,
    pub after_label: String,
}

impl ComparisonSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.before_label.len() > 512
            || self.after_label.len() > 512
            || self.before_label.contains('\0')
            || self.after_label.contains('\0')
        {
            return Err("comparison labels must be at most 512 bytes without NUL bytes".into());
        }
        for snapshot in self.before.iter().chain(self.after.iter()) {
            snapshot.validate()?;
        }
        if self
            .live_after
            .as_ref()
            .is_some_and(|path| !Path::new(path).is_absolute() || path.contains('\0'))
        {
            return Err("live comparison paths must be absolute".into());
        }
        Ok(())
    }
}

impl Snapshot {
    fn validate(&self) -> Result<(), String> {
        if self.digest.len() != 64
            || !self
                .digest
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("invalid snapshot digest".into());
        }
        Ok(())
    }
}

pub struct SnapshotStore {
    directory: PathBuf,
}

impl SnapshotStore {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn from_environment() -> Result<Self, String> {
        let store =
            crate::persistence::MetadataStore::from_xdg_environment().map_err(|e| e.to_string())?;
        Ok(Self::new(
            store
                .state_file()
                .parent()
                .expect("state directory")
                .join("snapshots"),
        ))
    }

    pub fn capture(&self, path: &Path) -> Result<Snapshot, String> {
        let source = read_source(path)?;
        self.put(&source)
    }

    pub fn put(&self, source: &SourceText) -> Result<Snapshot, String> {
        let digest = format!("{:x}", Sha256::digest(source.text.as_bytes()));
        let snapshot = Snapshot { digest };
        std::fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&self.directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        let path = self.directory.join(&snapshot.digest);
        if path.exists() {
            self.read(&snapshot)?;
            return Ok(snapshot);
        }
        let mut total = 0_u64;
        for (count, entry) in std::fs::read_dir(&self.directory)
            .map_err(|e| e.to_string())?
            .enumerate()
        {
            if count + 1 >= 4096 {
                return Err("snapshot store entry limit reached".into());
            }
            total = total.saturating_add(
                entry
                    .map_err(|e| e.to_string())?
                    .metadata()
                    .map_err(|e| e.to_string())?
                    .len(),
            );
        }
        if total.saturating_add(source.text.len() as u64) > MAX_STORE_BYTES {
            return Err("snapshot store reached 128 MiB; archive/remove unused comparison snapshots with SMAD closed".into());
        }
        let mut file =
            tempfile::NamedTempFile::new_in(&self.directory).map_err(|error| error.to_string())?;
        file.write_all(source.text.as_bytes())
            .map_err(|error| error.to_string())?;
        file.as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        if let Err(error) = file.persist_noclobber(&path) {
            if error.error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(error.to_string());
            }
            self.read(&snapshot)?;
        }
        std::fs::File::open(&self.directory)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| error.to_string())?;
        Ok(snapshot)
    }

    pub fn read(&self, snapshot: &Snapshot) -> Result<SourceText, String> {
        snapshot.validate()?;
        let source = read_source(&self.directory.join(&snapshot.digest))?;
        if format!("{:x}", Sha256::digest(source.text.as_bytes())) != snapshot.digest {
            return Err("snapshot contents no longer match their digest".into());
        }
        Ok(source)
    }

    pub fn read_optional(&self, snapshot: Option<&Snapshot>) -> Result<SourceText, String> {
        snapshot.map_or_else(
            || SourceText::parse(String::new()),
            |snapshot| self.read(snapshot),
        )
    }
}
