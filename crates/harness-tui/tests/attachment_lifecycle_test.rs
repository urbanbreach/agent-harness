use std::error::Error;
use std::fs;
use std::path::Path;

use harness_tui::attachment_lifecycle::{
    active_temp_artifacts, AttachmentError, AttachmentIngestor, AttachmentPolicy,
    CancellationToken, EditorCommand, ExternalEditor, Limits, Preview, TempArtifact,
};

fn ingestor(root: &Path, limits: Limits) -> Result<AttachmentIngestor, AttachmentError> {
    Ok(AttachmentIngestor::new(
        AttachmentPolicy::new(root)?.with_limits(limits),
    ))
}

fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
    ];
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 2, 0, 0, 0]);
    bytes
}

fn zip_local_header(compressed: u32, decompressed: u32) -> Vec<u8> {
    let mut bytes = vec![b'P', b'K', 3, 4, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    bytes.extend(compressed.to_le_bytes());
    bytes.extend(decompressed.to_le_bytes());
    bytes.extend([0, 0, 0, 0]);
    bytes
}

#[test]
fn allowed_png_jpeg_and_text_are_ingested_with_typed_previews() -> Result<(), Box<dyn Error>> {
    // arrange
    let tempdir = tempfile::tempdir()?;
    let ingestor = ingestor(tempdir.path(), Limits::default())?;
    let png = tempdir.path().join("allowed.png");
    let jpeg = tempdir.path().join("allowed.jpg");
    let text = tempdir.path().join("allowed.txt");
    fs::write(&png, png_header(1, 1))?;
    fs::write(&jpeg, [0xff, 0xd8, 0xff, 0xd9])?;
    fs::write(&text, "bounded text")?;

    // act
    let cancellation = CancellationToken::new();
    let png_attachment = ingestor.ingest_file(&png, &cancellation)?;
    let jpeg_attachment = ingestor.ingest_file(&jpeg, &cancellation)?;
    let text_attachment = ingestor.ingest_file(&text, &cancellation)?;

    // assert
    assert!(matches!(png_attachment.preview(), Preview::Image { .. }));
    assert!(matches!(jpeg_attachment.preview(), Preview::Image { .. }));
    assert!(matches!(text_attachment.preview(), Preview::Text { .. }));
    Ok(())
}

#[test]
fn unknown_mime_is_rejected() -> Result<(), Box<dyn Error>> {
    // arrange
    // act
    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path().join("unknown.bin");
    fs::write(&path, [0, 159, 146, 150])?;
    let error =
        ingestor(tempdir.path(), Limits::default())?.ingest_file(&path, &CancellationToken::new());

    // assert
    assert!(matches!(error, Err(AttachmentError::MimeRejected { .. })));
    Ok(())
}

#[test]
fn oversized_input_is_rejected_before_preview() -> Result<(), Box<dyn Error>> {
    // arrange
    // act
    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path().join("large.txt");
    fs::write(&path, vec![b'x'; 9])?;
    let limits = Limits::default().with_max_bytes(8);
    let error = ingestor(tempdir.path(), limits)?.ingest_file(&path, &CancellationToken::new());

    // assert
    assert!(matches!(error, Err(AttachmentError::SizeLimit { .. })));
    Ok(())
}

#[test]
fn decompression_bomb_is_rejected_by_decompressed_limit() -> Result<(), Box<dyn Error>> {
    // arrange
    // act
    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path().join("bomb.zip");
    fs::write(&path, zip_local_header(1, 1_000_000_000))?;
    let error =
        ingestor(tempdir.path(), Limits::default())?.ingest_file(&path, &CancellationToken::new());

    // assert
    assert!(matches!(
        error,
        Err(AttachmentError::DecompressionLimit { .. })
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_rejected_after_canonicalization() -> Result<(), Box<dyn Error>> {
    // arrange
    // act
    let tempdir = tempfile::tempdir()?;
    let link = tempdir.path().join("escape");
    std::os::unix::fs::symlink("/etc/passwd", &link)?;
    let error =
        ingestor(tempdir.path(), Limits::default())?.ingest_file(&link, &CancellationToken::new());

    // assert
    assert!(matches!(error, Err(AttachmentError::PathEscape)));
    Ok(())
}

#[cfg(unix)]
#[test]
fn unreadable_file_is_rejected_without_path_details() -> Result<(), Box<dyn Error>> {
    // arrange
    use std::os::unix::fs::PermissionsExt;

    // act
    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path().join("private.txt");
    fs::write(&path, "secret")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000))?;
    let error =
        ingestor(tempdir.path(), Limits::default())?.ingest_file(&path, &CancellationToken::new());

    // assert
    assert!(matches!(error, Err(AttachmentError::Unreadable)));
    Ok(())
}

#[test]
fn editor_nonzero_exit_is_redacted_and_typed() -> Result<(), Box<dyn Error>> {
    // arrange
    let command = EditorCommand::new("sh").arg("-c").arg("exit 7");
    let error = ExternalEditor::new(command).edit(b"draft", &CancellationToken::new());

    // act
    let error = match error {
        Ok(_) => return Err("nonzero editor unexpectedly succeeded".into()),
        Err(error) => error,
    };
    // assert
    assert!(matches!(error, AttachmentError::EditorNonZero { .. }));
    assert_eq!(
        error.to_string(),
        "external editor exited unsuccessfully (status 7)"
    );
    Ok(())
}

#[test]
fn cancellation_mid_ingest_drops_partial_state() -> Result<(), Box<dyn Error>> {
    // arrange
    // act
    let tempdir = tempfile::tempdir()?;
    let path = tempdir.path().join("cancel.txt");
    fs::write(&path, vec![b'x'; 128 * 1024])?;
    let token = CancellationToken::new();
    let error = ingestor(tempdir.path(), Limits::default())?.ingest_file_with_observer(
        &path,
        &token,
        |read, token| {
            if read >= 65_536 {
                token.cancel();
            }
        },
    );

    // assert
    assert!(matches!(error, Err(AttachmentError::Cancelled)));
    Ok(())
}

#[test]
fn path_errors_never_include_absolute_paths_or_usernames() -> Result<(), Box<dyn Error>> {
    // arrange
    // act
    let tempdir = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let error = ingestor(tempdir.path(), Limits::default())?.ingest_file(
        outside.path().join("missing").as_path(),
        &CancellationToken::new(),
    );
    let message = match error {
        Ok(_) => return Err("outside path unexpectedly succeeded".into()),
        Err(error) => error.to_string(),
    };

    // assert
    assert!(message.contains("<redacted-path>"));
    assert!(!message.contains(tempdir.path().to_string_lossy().as_ref()));
    assert!(!message.contains(outside.path().to_string_lossy().as_ref()));
    Ok(())
}

#[test]
fn temp_artifact_drop_removes_file_and_directory() -> Result<(), Box<dyn Error>> {
    // arrange
    let artifact = TempArtifact::new("attachment-test", b"draft")?;
    let file = artifact.path().to_owned();
    let directory = artifact.root_path().to_owned();
    assert!(file.exists());
    assert!(active_temp_artifacts() > 0);

    // act
    drop(artifact);

    // assert
    assert!(!file.exists());
    assert!(!directory.exists());
    Ok(())
}
