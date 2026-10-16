use std::io::Read;
use std::path::{Component, Path};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};

const MAX_BYTES: u64 = 20 * 1024 * 1024;

pub(super) async fn read(params: &Value) -> Result<Value, String> {
    let run_dir = super::required(params, "runDir")?.to_owned();
    let reference = super::required(params, "ref")?.to_owned();
    tokio::task::spawn_blocking(move || read_artifact(Path::new(&run_dir), &reference))
        .await
        .map_err(|error| error.to_string())?
}

fn read_artifact(run_dir: &Path, reference: &str) -> Result<Value, String> {
    if !run_dir.is_absolute()
        || run_dir
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(
            "invalid_params: runDir must be an absolute path without parent components".into(),
        );
    }
    let digest = reference
        .strip_prefix("attachment:blake3:")
        .filter(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or("invalid_params: ref must be a canonical attachment content reference")?;
    let path = run_dir
        .join("artifacts")
        .join(format!("{digest}.attachment"));
    let file = harness_core::store::open_private_file(&path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_BYTES {
        return Err("attachment exceeds the 20 MiB size limit".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("attachment exceeds the 20 MiB size limit".into());
    }
    if blake3::hash(&bytes).to_hex().as_str() != digest {
        return Err("attachment content does not match its digest".into());
    }
    Ok(json!({"dataBase64": STANDARD.encode(bytes)}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn attachments_verify_content_and_reject_escape_symlinks_and_oversize(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let artifacts = root.path().join("artifacts");
        std::fs::create_dir(&artifacts)?;
        let bytes = b"tool image";
        let digest = blake3::hash(bytes).to_hex();
        let path = artifacts.join(format!("{digest}.attachment"));
        let params = json!({"runDir":root.path(),"ref":format!("attachment:blake3:{digest}")});
        std::fs::write(&path, bytes)?;
        assert_eq!(
            read(&params).await?,
            json!({"dataBase64":STANDARD.encode(bytes)})
        );
        std::fs::write(&path, b"corrupted")?;
        assert!(read(&params).await.is_err());
        std::fs::remove_file(&path)?;
        let outside = root.path().join("outside");
        std::fs::write(&outside, bytes)?;
        std::os::unix::fs::symlink(&outside, &path)?;
        assert!(read(&params).await.is_err());
        std::fs::remove_file(&path)?;
        std::fs::remove_dir(&artifacts)?;
        let external = tempfile::tempdir()?;
        std::fs::write(external.path().join(format!("{digest}.attachment")), bytes)?;
        std::os::unix::fs::symlink(external.path(), &artifacts)?;
        assert!(read(&params).await.is_err());
        std::fs::remove_file(&artifacts)?;
        std::fs::create_dir(&artifacts)?;
        std::fs::File::create(&path)?.set_len(MAX_BYTES + 1)?;
        assert!(read(&params).await.is_err());
        for reference in [
            "attachment:blake3:../../outside",
            "attachment:blake3:ABC",
            "file:/etc/passwd",
        ] {
            assert!(read(&json!({"runDir":root.path(),"ref":reference}))
                .await
                .is_err());
        }
        Ok(())
    }
}
