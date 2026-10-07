use super::{ProviderId, StoredCredential, StoredCredentialKind};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CredentialStoreError {
    #[error("credential storage: {0}")]
    Io(#[from] io::Error),
    #[error("credential storage: {0}")]
    Invalid(&'static str),
    /// Another writer held the store past the lock budget.
    #[error("Credential store is busy: lock {path} was held for {waited_ms}ms. Another harness process may be refreshing credentials; close unused sessions if contention persists.")]
    Busy { path: String, waited_ms: u128 },
}

/// Bounded lock wait; contention never blocks a request indefinitely.
pub const CREDENTIAL_LOCK_RETRY_BUDGET: std::time::Duration =
    std::time::Duration::from_millis(5_500);

/// An exclusive hold on the credential directory; released on drop.
pub struct CredentialStoreLock {
    _file: Option<File>,
}
#[derive(Debug, Clone)]
pub struct CredentialStore {
    data_dir: PathBuf,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CredentialStoreManifestEntry {
    pub provider: ProviderId,
    pub status: String,
    pub kind: Option<StoredCredentialKind>,
    pub relative_path: String,
    pub absolute_path: String,
}
impl CredentialStore {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }
    pub fn from_env() -> Option<Self> {
        Self::from_lookup(&|key| std::env::var(key).ok())
    }
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Option<Self> {
        let get = |key| {
            lookup(key)
                .filter(|v| !v.trim().is_empty())
                .map(PathBuf::from)
        };
        if let Some(path) = get("HARNESS_DATA_HOME") {
            return Some(Self::new(path.join("harness")));
        }
        #[cfg(windows)]
        let path = get("LOCALAPPDATA").or_else(|| get("APPDATA"));
        #[cfg(not(windows))]
        let path = get("XDG_DATA_HOME").or_else(|| get("HOME").map(|p| p.join(".local/share")));
        path.map(|p| Self::new(p.join("harness")))
    }
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
    pub fn credential_path(&self, provider: &ProviderId) -> PathBuf {
        self.data_dir
            .join("credentials")
            .join(format!("{provider}.json"))
    }
    pub fn load(
        &self,
        provider: &ProviderId,
    ) -> Result<Option<StoredCredential>, CredentialStoreError> {
        let path = self.credential_path(provider);
        let Some(bytes) = crate::store::read_private_bytes(&path, MAX_BYTES)? else {
            return Ok(None);
        };
        // JSON errors can quote credential values; never expose the parser's error text.
        let value: StoredCredential = serde_json::from_slice(&bytes)
            .map_err(|_| CredentialStoreError::Invalid("invalid credential document"))?;
        value.validate(provider)?;
        Ok(Some(value))
    }
    pub fn stored_provider_ids(&self) -> Result<Vec<ProviderId>, CredentialStoreError> {
        let path = self.data_dir.join("credentials");
        validate_path(&path)?;
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut providers = BTreeSet::new();
        for (index, entry) in entries.enumerate() {
            if index >= 4096 {
                return Err(CredentialStoreError::Invalid(
                    "credential directory exceeds 4096 entries",
                ));
            }
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                validate_path(&path)?;
                if !entry.file_type()?.is_file() {
                    return Err(CredentialStoreError::Invalid(
                        "credential must be a regular file",
                    ));
                }
                let id = path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .and_then(ProviderId::parse)
                    .ok_or(CredentialStoreError::Invalid("invalid credential filename"))?;
                providers.insert(id);
            }
        }
        Ok(providers.into_iter().collect())
    }
    pub fn save(&self, credential: &StoredCredential) -> Result<(), CredentialStoreError> {
        credential.validate(&credential.provider)?;
        let _lock = self.lock(true)?;
        self.write(credential)
    }
    pub fn delete(&self, provider: &ProviderId) -> Result<bool, CredentialStoreError> {
        let path = self.credential_path(provider);
        validate_path(&path)?;
        let Some(_lock) = self.lock(false)? else {
            return Ok(false);
        };
        validate_path(&path)?;
        match fs::remove_file(&path) {
            Ok(()) => {
                #[cfg(unix)]
                File::open(self.data_dir.join("credentials"))?.sync_all()?;
                Ok(true)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
    pub(super) fn replace_if_unchanged(
        &self,
        old: &StoredCredential,
        new: &StoredCredential,
    ) -> Result<(), CredentialStoreError> {
        let _lock = self.lock(false)?;
        if self.load(&old.provider)?.as_ref() != Some(old) {
            return Err(CredentialStoreError::Invalid(
                "credential changed during refresh",
            ));
        }
        self.write(new)
    }
    fn write(&self, credential: &StoredCredential) -> Result<(), CredentialStoreError> {
        credential.validate(&credential.provider)?;
        let path = self.credential_path(&credential.provider);
        validate_path(&path)?;
        let bytes = serde_json::to_vec(credential)
            .map_err(|_| CredentialStoreError::Invalid("cannot encode credential"))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(CredentialStoreError::Invalid("credential exceeds 1 MiB"));
        }
        crate::store::write_private_atomic(&path, &bytes)?;
        Ok(())
    }
    /// Locked read-modify-write; `Ok(None)` from `update` leaves the store unchanged and
    /// returns the current credential.
    pub fn modify(
        &self,
        provider: &ProviderId,
        update: impl FnOnce(Option<StoredCredential>) -> Result<Option<StoredCredential>, String>,
    ) -> Result<Option<StoredCredential>, CredentialStoreError> {
        let _lock = self.lock_bounded(CREDENTIAL_LOCK_RETRY_BUDGET)?;
        self.modify_locked(provider, update)
    }
    /// `modify` for a caller that already holds [`Self::lock_bounded`].
    pub fn modify_locked(
        &self,
        provider: &ProviderId,
        update: impl FnOnce(Option<StoredCredential>) -> Result<Option<StoredCredential>, String>,
    ) -> Result<Option<StoredCredential>, CredentialStoreError> {
        let current = self.load(provider)?;
        match update(current.clone())
            .map_err(|_| CredentialStoreError::Invalid("credential update was rejected"))?
        {
            Some(next) => {
                self.write(&next)?;
                Ok(Some(next))
            }
            None => Ok(current),
        }
    }
    /// Takes the store lock, retrying with backoff for at most `budget`.
    pub fn lock_bounded(
        &self,
        budget: std::time::Duration,
    ) -> Result<CredentialStoreLock, CredentialStoreError> {
        let path = self.data_dir.join("credentials");
        validate_path(&path)?;
        #[cfg(not(unix))]
        {
            let _ = budget;
            return Err(CredentialStoreError::Invalid(
                "private credential storage is unavailable on this platform",
            ));
        }
        #[cfg(unix)]
        {
            crate::store::create_private_dir(&path)?;
            let file = File::open(&path)?;
            let started = std::time::Instant::now();
            let mut delay = std::time::Duration::from_millis(100);
            loop {
                match file.try_lock() {
                    Ok(()) => return Ok(CredentialStoreLock { _file: Some(file) }),
                    Err(std::fs::TryLockError::WouldBlock) if started.elapsed() < budget => {
                        std::thread::sleep(delay.min(budget.saturating_sub(started.elapsed())));
                        delay = (delay * 2).min(std::time::Duration::from_secs(1));
                    }
                    Err(std::fs::TryLockError::WouldBlock) => {
                        return Err(CredentialStoreError::Busy {
                            path: path.display().to_string(),
                            waited_ms: started.elapsed().as_millis(),
                        });
                    }
                    Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
                }
            }
        }
    }
    fn lock(&self, create: bool) -> Result<Option<File>, CredentialStoreError> {
        let path = self.data_dir.join("credentials");
        validate_path(&path)?;
        // Unix directory locks survive atomic replacement without creating lock artifacts.
        #[cfg(not(unix))]
        return Err(CredentialStoreError::Invalid(
            "private credential storage is unavailable on this platform",
        ));
        #[cfg(unix)]
        {
            if create {
                crate::store::create_private_dir(&path)?;
            }
            let lock = match File::open(&path) {
                Ok(lock) => lock,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            lock.lock()?;
            Ok(Some(lock))
        }
    }
    pub fn manifest_entries(
        &self,
        providers: impl IntoIterator<Item = ProviderId>,
    ) -> Vec<CredentialStoreManifestEntry> {
        providers
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|provider| {
                let (status, kind) = match self.load(&provider) {
                    Ok(Some(value)) => ("excluded_stored", Some(value.kind)),
                    Ok(None) => ("not_stored", None),
                    Err(_) => ("unavailable", None),
                };
                CredentialStoreManifestEntry {
                    relative_path: format!("credentials/{provider}.json"),
                    absolute_path: self.credential_path(&provider).display().to_string(),
                    provider,
                    status: status.into(),
                    kind,
                }
            })
            .collect()
    }
}
fn validate_path(path: &Path) -> Result<(), CredentialStoreError> {
    for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.is_symlink() => {
                return Err(CredentialStoreError::Invalid(
                    "credential paths cannot be symlinks",
                ))
            }
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}
pub fn credential_file_mode(path: &Path) -> io::Result<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(fs::symlink_metadata(path)?.permissions().mode())
    }
    #[cfg(not(unix))]
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Unix file modes are unavailable",
    ))
}
