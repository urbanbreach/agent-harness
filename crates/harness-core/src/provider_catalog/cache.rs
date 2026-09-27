use super::*;
use std::{fs, path::PathBuf, time::Duration};
const TTL: Duration = Duration::from_secs(300);
impl ProviderCatalog {
    pub fn fetch_from_url(url: &str) -> Result<Self, CatalogError> {
        Self::parse(&fetch(url)?, false, url)
    }
    pub fn cached(path: &Path, url: Option<&str>) -> Result<Self, CatalogError> {
        if fetch_disabled() {
            return Self::from_embedded();
        }
        if let Ok(catalog) = Self::from_path(path) {
            let fresh = fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| m.elapsed().ok())
                .is_some_and(|age| age < TTL);
            if !fresh {
                if let Some(url) = url {
                    Self::refresh_in_background(path, url.into());
                }
            }
            return Ok(catalog);
        }
        url.and_then(|url| refresh(path, url).ok())
            .map_or_else(Self::from_embedded, Ok)
    }
    pub fn refresh_in_background(path: &Path, url: String) {
        if fetch_disabled() {
            return;
        }
        let path = path.to_owned();
        let _ = std::thread::Builder::new()
            .name("catalog-refresh".into())
            .spawn(move || {
                let _ = refresh(&path, &url);
            });
    }
    pub fn from_env() -> Result<Self, CatalogError> {
        let path = std::env::var_os("HARNESS_MODELS_PATH")
            .map(PathBuf::from)
            .or_else(|| {
                crate::auth::CredentialStore::from_env()
                    .map(|s| s.data_dir().join("models-cache.json"))
            });
        let Some(path) = path else {
            return Self::from_embedded();
        };
        let url = std::env::var("HARNESS_MODELS_URL")
            .unwrap_or_else(|_| "https://models.dev/api.json".into());
        Self::cached(&path, Some(&url))
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
