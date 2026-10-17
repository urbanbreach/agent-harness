use std::io::Read;
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use harness_core::attachment_transport::AttachmentMetadata;
use harness_core::redact::{DefaultRedactor, Redactor, SecretRedactor, SecretRegistry};
use harness_core::store::{create_private_dir, open_private_file, write_private_atomic};
use harness_providers::attachment_protocol::MAX_ATTACHMENT_BYTES;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const MAX_BASE64_BYTES: usize = MAX_ATTACHMENT_BYTES.div_ceil(3) * 4;
const MAX_DOCUMENT_BYTES: u64 = (MAX_BASE64_BYTES + 1024) as u64;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Blob {
    mime: String,
    data_base64: String,
}

pub(super) async fn dispatch(
    server: &super::Server,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    let root = server
        .config_context
        .discovery
        .data_dir
        .as_ref()
        .ok_or("unavailable: Harness home is not configured")?
        .join("gui")
        .join("blobs");
    let mut deps = harness::CliDeps::real()
        .with_current_dir(server.config_context.discovery.current_dir.clone());
    for (name, value) in [
        (
            "HARNESS_HOME",
            server
                .config_context
                .discovery
                .data_dir
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
        ),
        (
            "HARNESS_CONFIG",
            server
                .config_context
                .discovery
                .runtime_config_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
        ),
        (
            "HARNESS_CONFIG_CONTENT",
            server.config_context.runtime_content.clone(),
        ),
    ] {
        deps = match value {
            Some(value) => deps.with_env(name, value),
            None => deps.without_env(name),
        };
    }
    let config = server.config_path.clone();
    let method = method.to_owned();
    let params = params.clone();
    tokio::task::spawn_blocking(move || {
        let loaded = harness_core::config::load_resolved_config_with_lookup(
            config.as_deref(),
            &deps.config_load_context(),
            &|name| deps.env_var_value(name),
        )
        .map_err(|error| error.to_string())?;
        let config = loaded.map(|loaded| loaded.config).unwrap_or_default();
        let store =
            harness_core::auth::CredentialStore::from_lookup(&|name| deps.env_var_value(name));
        let registry = std::sync::Arc::new(SecretRegistry::default());
        registry.register(crate::bootstrap::secret_values(
            &config,
            &deps,
            store.as_ref(),
        )?)?;
        let redactor =
            SecretRedactor::new(std::sync::Arc::new(DefaultRedactor::default()), registry);
        handle(&root, &method, &params, &redactor)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn safe_path(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors().filter(|path| !path.as_os_str().is_empty()) {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.is_symlink() => {
                return Err("invalid_params: blob storage cannot contain symlinks".into())
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.to_string())
            }
            _ => {}
        }
    }
    Ok(())
}

fn blob_path(root: &Path, digest: &str) -> Result<PathBuf, String> {
    let hex = digest
        .strip_prefix("attachment:blake3:")
        .filter(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or("invalid_params: digest must be a canonical attachment content reference")?;
    let path = root.join(format!("{hex}.json"));
    safe_path(&path)?;
    Ok(path)
}

fn validate(blob: &Blob, redactor: &dyn Redactor) -> Result<AttachmentMetadata, String> {
    if blob.data_base64.len() > MAX_BASE64_BYTES {
        return Err("invalid_params: blob exceeds the attachment size limit".into());
    }
    let bytes = STANDARD
        .decode(&blob.data_base64)
        .map_err(|error| format!("invalid_params: invalid dataBase64: {error}"))?;
    let digest = format!("attachment:blake3:{}", blake3::hash(&bytes));
    let content = String::from_utf8_lossy(&bytes);
    if redactor.redact_text(&content) != content
        || DefaultRedactor::default().secret_finding_count(&content) != 0
    {
        return Err("invalid_params: attachment contains credential data".into());
    }
    AttachmentMetadata::new("blob", &blob.mime, bytes.len() as u64, None, digest)
        .with_bytes(bytes)
        .map_err(|error| format!("invalid_params: {error}"))
}

fn read_blob(path: &Path) -> Result<Option<Blob>, String> {
    let file = match open_private_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_DOCUMENT_BYTES {
        return Err("blob document exceeds the attachment size limit".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err("blob document exceeds the attachment size limit".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn handle(
    root: &Path,
    method: &str,
    params: &Value,
    redactor: &dyn Redactor,
) -> Result<Value, String> {
    match method {
        "blobs.put" => {
            let blob: Blob = serde_json::from_value(params.clone())
                .map_err(|error| format!("invalid_params: {error}"))?;
            let metadata = validate(&blob, redactor)?;
            let path = blob_path(root, &metadata.content_ref)?;
            // One private atomic document publishes bytes and MIME together.
            create_private_dir(root).map_err(|error| error.to_string())?;
            safe_path(&path)?;
            let bytes = serde_json::to_vec(&blob).map_err(|error| error.to_string())?;
            write_private_atomic(&path, &bytes).map_err(|error| error.to_string())?;
            Ok(json!({"digest":metadata.content_ref}))
        }
        "blobs.get" => {
            let digest = super::required(params, "digest")?;
            let path = blob_path(root, digest)?;
            let Some(blob) = read_blob(&path)? else {
                return Ok(Value::Null);
            };
            let metadata = validate(&blob, redactor)?;
            if metadata.content_ref != digest {
                return Err("blob content does not match its digest".into());
            }
            serde_json::to_value(blob).map_err(|error| error.to_string())
        }
        _ => Err(format!("method_not_found: {method}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blobs_round_trip_validate_content_and_confine_storage(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("gui/blobs");
        let redactor = DefaultRedactor::default();
        let missing = format!("attachment:blake3:{}", "0".repeat(64));
        assert_eq!(
            handle(&root, "blobs.get", &json!({"digest":missing}), &redactor)?,
            Value::Null
        );
        assert!(!root.exists());
        let input = json!({"mime":"text/plain","dataBase64":STANDARD.encode(b"native attachment")});
        let result = handle(&root, "blobs.put", &input, &redactor)?;
        let digest = result["digest"].as_str().ok_or("missing digest")?;
        assert_eq!(
            digest,
            format!("attachment:blake3:{}", blake3::hash(b"native attachment"))
        );
        assert_eq!(
            handle(&root, "blobs.get", &json!({"digest":digest}), &redactor)?,
            input
        );
        for invalid in ["../../outside", "attachment:blake3:abc", "0"] {
            assert!(handle(&root, "blobs.get", &json!({"digest":invalid}), &redactor).is_err());
        }
        for invalid in [
            json!({"mime":"text/plain","dataBase64":"YQ"}),
            json!({"mime":"image/png","dataBase64":STANDARD.encode(b"not an image")}),
            json!({"mime":"text/plain","dataBase64":STANDARD.encode(b"-----BEGIN PRIVATE KEY-----\nsecret-body")}),
        ] {
            assert!(handle(&root, "blobs.put", &invalid, &redactor).is_err());
        }
        let path = blob_path(&root, digest)?;
        std::fs::write(
            &path,
            serde_json::to_vec(
                &json!({"mime":"text/plain","dataBase64":STANDARD.encode(b"tampered")}),
            )?,
        )?;
        assert!(handle(&root, "blobs.get", &json!({"digest":digest}), &redactor).is_err());
        #[cfg(unix)]
        {
            let alias = temp.path().join("alias");
            std::os::unix::fs::symlink(&root, &alias)?;
            assert!(handle(&alias, "blobs.put", &input, &redactor).is_err());
            let target = temp.path().join("outside");
            std::fs::write(&target, "untouched")?;
            std::fs::remove_file(&path)?;
            std::os::unix::fs::symlink(&target, &path)?;
            assert!(handle(&root, "blobs.get", &json!({"digest":digest}), &redactor).is_err());
            assert!(handle(&root, "blobs.put", &input, &redactor).is_err());
            assert_eq!(std::fs::read_to_string(target)?, "untouched");
        }
        Ok(())
    }
}
