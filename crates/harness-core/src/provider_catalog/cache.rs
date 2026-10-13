use super::*;
use std::{fs, path::PathBuf, time::Duration};
/// How long a downloaded catalog counts as current.
const TTL: Duration = Duration::from_secs(2 * 60 * 60);
const MODELS_DEV_URL: &str = "https://models.dev/api.json";
impl ProviderCatalog {
    pub fn fetch_from_url(url: &str) -> Result<Self, CatalogError> {
        Self::parse(&fetch(url)?, false, url)
    }
    /// The cached catalog, or the bundled one while there is none. Never waits for the
    /// network: an unreadable cache, like a stale one, refreshes in the background.
    pub fn cached(path: &Path, url: Option<&str>) -> Result<Self, CatalogError> {
        if fetch_disabled() {
            return Self::from_embedded();
        }
        let cached = Self::from_path(path);
        match url {
            Some(url) if cached.is_err() => drop(Self::refresh_in_background(path, url.into())),
            Some(url) => drop(Self::refresh_if_stale(path, url)),
            None => {}
        }
        cached.or_else(|_| Self::from_embedded())
    }
    /// Refreshes a missing or older than two hours cache in the background. Join the
    /// returned thread before a short-lived process exits, or the download is lost.
    pub fn refresh_if_stale(path: &Path, url: &str) -> Option<std::thread::JoinHandle<()>> {
        let fresh = fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age < TTL);
        (!fresh)
            .then(|| Self::refresh_in_background(path, url.into()))
            .flatten()
    }
    pub fn refresh_in_background(path: &Path, url: String) -> Option<std::thread::JoinHandle<()>> {
        if fetch_disabled() {
            return None;
        }
        let path = path.to_owned();
        std::thread::Builder::new()
            .name("catalog-refresh".into())
            .spawn(move || {
                let _ = Self::refresh(&path, &url);
            })
            .ok()
    }
    /// Downloads the catalog into `path` now; a failed download keeps the previous file.
    pub fn refresh(path: &Path, url: &str) -> Result<Self, CatalogError> {
        refresh(path, url)
    }
    /// The cache file under the Harness home, or `HARNESS_MODELS_PATH`, and the URL
    /// that refreshes it, `HARNESS_MODELS_URL` or models.dev.
    pub fn cache_location(lookup: &dyn Fn(&str) -> Option<String>) -> Option<(PathBuf, String)> {
        let path = lookup("HARNESS_MODELS_PATH")
            .map(PathBuf::from)
            .or_else(|| {
                crate::auth::CredentialStore::from_lookup(lookup)
                    .map(|s| s.data_dir().join("models-cache.json"))
            })?;
        let url = lookup("HARNESS_MODELS_URL").unwrap_or_else(|| MODELS_DEV_URL.into());
        Some((path, url))
    }
    pub fn from_env() -> Result<Self, CatalogError> {
        match Self::cache_location(&|name| std::env::var(name).ok()) {
            Some((path, url)) => Self::cached(&path, Some(&url)),
            None => Self::from_embedded(),
        }
    }
}
fn fetch_disabled() -> bool {
    std::env::var("HARNESS_DISABLE_MODELS_FETCH")
        .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}
fn fetch(url: &str) -> Result<String, CatalogError> {
    let url = reqwest::Url::parse(url)
        .map_err(|_| CatalogError::Invalid("invalid catalog URL".into()))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(CatalogError::Invalid(
            "catalog URL must be HTTP(S) without credentials".into(),
        ));
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| CatalogError::Invalid("cannot initialize catalog transport".into()))?;
    let response = client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|_| CatalogError::Invalid("catalog request failed".into()))?;
    let mut body = String::new();
    response.take(MAX_BYTES + 1).read_to_string(&mut body)?;
    if body.len() as u64 > MAX_BYTES {
        return Err(CatalogError::Invalid("catalog exceeds 16 MiB".into()));
    }
    Ok(body)
}
fn refresh(path: &Path, url: &str) -> Result<ProviderCatalog, CatalogError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    for part in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        match fs::symlink_metadata(part) {
            Ok(m) if m.is_symlink() => {
                return Err(CatalogError::Invalid(
                    "catalog cache paths cannot be symlinks".into(),
                ))
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    crate::store::create_private_dir(parent)?;
    // ponytail: one refresh per cache directory; use per-file locks if independent catalogs share it.
    let lock = fs::File::open(parent)?;
    lock.try_lock()
        .map_err(|_| CatalogError::Invalid("catalog refresh already running".into()))?;
    let body = fetch(url)?;
    let catalog = ProviderCatalog::parse(&body, false, url)?;
    crate::store::write_private_atomic(path, body.as_bytes())?;
    Ok(catalog)
}
