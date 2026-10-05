use super::*;
use harness_core::attachment_transport::AttachmentMetadata;
use image::{ImageFormat, ImageReader, Limits};
use std::io::Cursor;

// Match the engine's 2000px / 4.5 MiB base64 budget, using the native image library.
pub(super) fn attachment(id: String, bytes: Vec<u8>) -> Result<AttachmentMetadata, ToolError> {
    let mut reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
    let format = reader
        .format()
        .ok_or_else(|| failure("unrecognized eval image"))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(32_768);
    limits.max_image_height = Some(32_768);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut image = reader.decode().map_err(failure)?;
    const MAX_BYTES: usize = 4_718_592 / 4 * 3;
    if image.width() <= 2000 && image.height() <= 2000 && bytes.len() <= MAX_BYTES {
        if let Some(mime) =
            crate::media::mime(&bytes).filter(|_| !matches!(format, ImageFormat::WebP))
        {
            return crate::media::attachment(id, mime, &bytes);
        }
    }
    image = image.thumbnail(2000, 2000);
    loop {
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, ImageFormat::Png)
            .map_err(failure)?;
        let mut encoded = png.into_inner();
        let mut mime = "image/png";
        if !image.color().has_alpha() {
            let mut jpeg = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80)
                .encode_image(&image)
                .map_err(failure)?;
            if jpeg.len() < encoded.len() {
                encoded = jpeg;
                mime = "image/jpeg";
            }
        }
        if encoded.len() <= MAX_BYTES {
            return crate::media::attachment(id, mime, &encoded);
        }
        image = image.thumbnail(
            (image.width() * 3 / 4).max(1),
            (image.height() * 3 / 4).max(1),
        );
    }
}
