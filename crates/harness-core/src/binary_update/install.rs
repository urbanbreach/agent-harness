use super::*;
use crate::{
    redact::{DefaultRedactor, Redactor},
    store::{create_private_dir, lock_private_parent, validate_private_path},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    time::Duration,
};

const MAX_BYTES: u64 = 256 * 1024 * 1024;

pub(super) fn parse_url(value: &str) -> io::Result<reqwest::Url> {
    let url = reqwest::Url::parse(value).map_err(|_| invalid("invalid update URL"))?;
    let local = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if value.len() > 8192
        || value.chars().any(char::is_control)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !(url.scheme() == "https" || url.scheme() == "file" || url.scheme() == "http" && local)
    {
        return Err(invalid(
            "update URL must be HTTPS, a local HTTP endpoint, or a local file without credentials",
        ));
    }
    Ok(url)
}
fn safe_url(value: &str) -> String {
    let Ok(mut url) = parse_url(value) else {
        return "[invalid URL]".into();
    };
    url.set_query(None);
    DefaultRedactor::default().redact_text(url.as_str())
}
pub fn download_update_artifact(
    url: &str,
    expected_sha256: Option<&str>,
    dest_dir: &Path,
) -> BinaryUpdateDownload {
    let safe = safe_url(url);
    let url = url.to_owned();
    let digest = expected_sha256.map(str::to_owned);
    let dest_dir = dest_dir.to_owned();
    // This synchronous API is also called from async adapters; blocking HTTP owns its thread.
    let result = std::thread::Builder::new()
        .name("harness-update-download".into())
        .spawn(move || download(&url, digest.as_deref(), &dest_dir))
        .and_then(|worker| worker.join().map_err(|_| invalid("update worker stopped")))
        .and_then(std::convert::identity);
    match result {
        Ok((path, bytes)) => BinaryUpdateDownload::Downloaded {
            url: safe,
            artifact_path: path.display().to_string(),
            bytes,
            sha256_verified: expected_sha256.map(|_| true),
        },
        Err(error) => BinaryUpdateDownload::Unavailable {
            url: safe,
            reason: format!(
                "artifact download or verification failed ({})",
                error.kind()
            ),
        },
    }
}
fn download(value: &str, expected: Option<&str>, destination: &Path) -> io::Result<(PathBuf, u64)> {
    let url = parse_url(value)?;
    if expected.is_some_and(|s| !check::valid_digest(s)) {
        return Err(invalid("invalid SHA-256"));
    }
    let mut reader: Box<dyn Read> = if url.scheme() == "file" {
        if url.query().is_some() {
            return Err(invalid("local file URL cannot contain a query"));
        }
        Box::new(open_regular(
            &url.to_file_path()
                .map_err(|()| invalid("invalid local file URL"))?,
        )?)
    } else {
        let response = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| invalid("cannot initialize update transport"))?
            .get(url)
            .send()
            .map_err(|_| invalid("update request failed"))?;
        if !response.status().is_success()
            || response.content_length().is_some_and(|n| n > MAX_BYTES)
        {
            return Err(invalid("update response is unsuccessful or oversized"));
        }
        Box::new(response)
    };
    validate_private_path(destination)?;
    create_private_dir(destination)?;
    let mut staged = tempfile::Builder::new()
        .prefix("harness-update-")
        .suffix(".bin")
        .tempfile_in(destination)?;
    let (bytes, digest) = copy_artifact(&mut reader, &mut staged)?;
    if expected.is_some_and(|s| !s.eq_ignore_ascii_case(&digest)) {
        return Err(invalid("SHA-256 mismatch"));
    }
    staged.as_file().sync_all()?;
    let (_, path) = staged.keep().map_err(|e| e.error)?;
    if let Err(error) = sync_directory(destination) {
        fs::remove_file(&path)?;
        return Err(error);
    }
    Ok((path, bytes))
}

pub fn apply_update(artifact_path: &Path, target_path: &Path) -> BinaryUpdateApply {
    let mut rolled_back = false;
    let result = replace(artifact_path, target_path, &mut rolled_back);
    let redact = |path: &Path| DefaultRedactor::default().redact_text(&path.display().to_string());
    match result {
        Ok(backup) => BinaryUpdateApply::Applied {
            artifact_path: redact(artifact_path),
            target_path: redact(target_path),
            backup_path: redact(&backup),
        },
        Err(error) => BinaryUpdateApply::Failed {
            artifact_path: redact(artifact_path),
            target_path: redact(target_path),
            reason: format!("replacement failed ({})", error.kind()),
            rolled_back,
        },
    }
}
fn replace(artifact: &Path, target: &Path, rolled_back: &mut bool) -> io::Result<PathBuf> {
    validate_private_path(target)?;
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(invalid("target directory does not exist"));
    }
    let _lock = lock_private_parent(target)?;
    let mut original = open_regular(target)?;
    let mut replacement = open_regular(artifact)?;
    if fs::canonicalize(target)? == fs::canonicalize(artifact)? {
        return Err(invalid("artifact is the installed binary"));
    }
    let permissions = ordinary_permissions(&original)?;
    let mut name = target
        .file_name()
        .ok_or_else(|| invalid("invalid target name"))?
        .to_os_string();
    name.push(".backup");
    let backup_path = parent.join(name);
    validate_private_path(&backup_path)?;
    if backup_path.exists() {
        return Err(io::Error::from(io::ErrorKind::AlreadyExists));
    }
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    copy_artifact(&mut replacement, &mut staged)?;
    staged.as_file().set_permissions(permissions.clone())?;
    staged.as_file().sync_all()?;
    let mut backup = tempfile::NamedTempFile::new_in(parent)?;
    let (_, digest) = copy_artifact(&mut original, &mut backup)?;
    backup.as_file().set_permissions(permissions)?;
    backup.as_file().sync_all()?;
    backup
        .persist_noclobber(&backup_path)
        .map_err(|e| e.error)?;
    sync_directory(parent)?;
    // An editor or package manager need not cooperate with our directory lock.
    if copy_artifact(&mut open_regular(target)?, &mut io::sink())?.1 != digest {
        return Err(invalid("installed binary changed during replacement"));
    }
    staged.persist(target).map_err(|e| e.error)?;
    if let Err(error) = sync_directory(parent) {
        *rolled_back = restore(&backup_path, target, parent).is_ok();
        return Err(error);
    }
    Ok(backup_path)
}
fn restore(backup: &Path, target: &Path, parent: &Path) -> io::Result<()> {
    let mut source = open_regular(backup)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    copy_artifact(&mut source, &mut staged)?;
    staged
        .as_file()
        .set_permissions(ordinary_permissions(&source)?)?;
    staged.as_file().sync_all()?;
    staged.persist(target).map_err(|e| e.error)?;
    sync_directory(parent)
}
fn open_regular(path: &Path) -> io::Result<File> {
    let file = crate::store::open_private_file(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err(invalid(
            "artifact must be a regular file of at most 256 MiB",
        ));
    }
    Ok(file)
}
fn copy_artifact(reader: &mut impl Read, writer: &mut impl Write) -> io::Result<(u64, String)> {
    let mut hash = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > MAX_BYTES {
            return Err(invalid("artifact exceeds 256 MiB"));
        }
        writer.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
    }
    if bytes == 0 {
        return Err(invalid("empty artifact"));
    }
    Ok((bytes, hex::encode(hash.finalize())))
}
fn ordinary_permissions(file: &File) -> io::Result<fs::Permissions> {
    let permissions = file.metadata()?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(fs::Permissions::from_mode(permissions.mode() & 0o777))
    }
    #[cfg(not(unix))]
    {
        Ok(permissions)
    }
}
fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}
