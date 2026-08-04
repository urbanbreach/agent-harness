use harness_tui::inline_image::{
    GraphicsProtocol, ImageCache, ImageCacheKey, ImageCapability, ImageError, ImagePipeline,
    ImageRequest, ImageState,
};

fn request(source: &[u8], width: u16, height: u16) -> ImageRequest {
    ImageRequest {
        source: source.to_vec(),
        target_width: width,
        target_height: height,
        post_flush: true,
    }
}

#[test]
fn capability_availability_matches_protocol() {
    assert!(ImageCapability::kitty(10, 10).is_available());
    assert!(ImageCapability::iterm2(10, 10).is_available());
    assert!(!ImageCapability::unsupported().is_available());
    assert_eq!(GraphicsProtocol::Kitty.label(), "kitty");
    assert!(GraphicsProtocol::Sixel.supports_truecolor());
    assert!(!GraphicsProtocol::None.supports_truecolor());
}

#[test]
fn cache_keys_hash_source_deterministically() {
    let first = ImageCacheKey::new(b"image", 10, 20, GraphicsProtocol::Kitty);
    assert_eq!(
        first,
        ImageCacheKey::new(b"image", 10, 20, GraphicsProtocol::Kitty)
    );
    assert_ne!(
        first,
        ImageCacheKey::new(b"other", 10, 20, GraphicsProtocol::Kitty)
    );
}

fn cached(
    key: ImageCacheKey,
    bytes: &[u8],
    tick: u64,
) -> harness_tui::inline_image::cache::CachedImage {
    harness_tui::inline_image::cache::CachedImage {
        key,
        rendered_bytes: bytes.to_vec(),
        width: key.target_width,
        height: key.target_height,
        created_at_tick: tick,
    }
}

#[test]
fn cache_inserts_gets_and_evicts_oldest_entries() {
    let protocol = GraphicsProtocol::Kitty;
    let key_a = ImageCacheKey::new(b"a", 1, 1, protocol);
    let key_b = ImageCacheKey::new(b"b", 1, 1, protocol);
    let key_c = ImageCacheKey::new(b"c", 1, 1, protocol);
    let mut cache = ImageCache::new(2, 4);
    cache.insert(cached(key_a, b"aa", 1));
    cache.insert(cached(key_b, b"b", 2));
    assert_eq!(
        cache
            .get(&key_a)
            .map(|image| image.rendered_bytes.as_slice()),
        Some(b"aa".as_slice())
    );
    cache.insert(cached(key_c, b"c", 3));
    assert!(cache.get(&key_a).is_none());
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.bytes(), 2);
}

#[test]
fn cache_invalidates_by_resize_and_protocol() {
    let key_kitty = ImageCacheKey::new(b"kitty", 1, 1, GraphicsProtocol::Kitty);
    let key_iterm = ImageCacheKey::new(b"iterm", 1, 1, GraphicsProtocol::ITerm2);
    let mut cache = ImageCache::default();
    cache.insert(cached(key_kitty, b"k", 1));
    cache.insert(cached(key_iterm, b"i", 2));
    cache.invalidate_protocol(GraphicsProtocol::Kitty);
    assert!(cache.get(&key_kitty).is_none());
    assert_eq!(cache.bytes(), 1);
    cache.invalidate_resize();
    assert_eq!(cache.len(), 0);
}

#[test]
fn pipeline_enforces_flush_capability_and_validation() {
    let image_request = request(b"source", 10, 10);
    let mut pipeline = ImagePipeline::new(ImageCapability::kitty(20, 20));
    assert_eq!(
        pipeline.submit(image_request.clone(), 1).state,
        ImageState::Pending
    );
    pipeline.mark_flush_complete();
    assert_eq!(
        pipeline.submit(image_request, 1).state,
        ImageState::Decoding
    );

    let mut unsupported = ImagePipeline::default();
    assert_eq!(
        unsupported.submit(request(b"source", 1, 1), 1).state,
        ImageState::Pending
    );
    unsupported.mark_flush_complete();
    assert_eq!(
        unsupported.submit(request(b"source", 1, 1), 1).state,
        ImageState::Failed(ImageError::UnsupportedProtocol)
    );

    let mut pipeline = ImagePipeline::new(ImageCapability::kitty(20, 20));
    pipeline.mark_flush_complete();
    assert_eq!(
        pipeline.submit(request(b"", 1, 1), 1).state,
        ImageState::Failed(ImageError::CorruptInput)
    );
    assert_eq!(
        pipeline.submit(request(b"source", 21, 20), 1).state,
        ImageState::Failed(ImageError::Oversized {
            actual: (21, 20),
            max: (20, 20)
        })
    );
}

#[test]
fn pipeline_caches_completed_decode_and_respects_post_flush_ordering() {
    let mut pipeline = ImagePipeline::new(ImageCapability::kitty(20, 20));
    let image_request = request(b"source", 10, 10);
    let key = image_request.cache_key(GraphicsProtocol::Kitty);
    assert_eq!(
        pipeline.submit(image_request.clone(), 1).state,
        ImageState::Pending
    );
    pipeline.mark_flush_complete();
    assert_eq!(
        pipeline.submit(image_request.clone(), 1).state,
        ImageState::Decoding
    );
    pipeline.complete_decode(&key, b"rendered".to_vec(), 10, 10, 2);
    let result = pipeline.submit(image_request, 3);
    assert_eq!(result.state, ImageState::Rendered(b"rendered".to_vec()));
    assert!(result.from_cache);
    pipeline.reset_flush();
    assert_eq!(
        pipeline.submit(request(b"source", 10, 10), 4).state,
        ImageState::Pending
    );
}
