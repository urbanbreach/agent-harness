use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use harness_testkit::tui_fidelity_cache::{
    ReferenceBinaryCache, ReferenceCache, ReferenceCacheInputs,
};
use harness_testkit::tui_fidelity_compare::hash_bytes;
use harness_testkit::tui_fidelity_deadline::{
    CommandSpec, CommandStatus, DeadlineRunner, InterruptFlag, ResourceLimits,
};

#[test]
fn deadline_kills_process_group_and_reports_no_surviving_descendants() {
    // Given: a command whose child process outlives the deadline.
    let runner = DeadlineRunner::new(
        Duration::from_millis(50),
        Duration::from_secs(1),
        ResourceLimits::unrestricted(),
        InterruptFlag::new_for_test(),
    );
    let command = CommandSpec::new("/bin/sh").args(["-c", "sleep 30 & wait"]);

    // When: the isolated process group exceeds its deadline.
    let receipt = runner.run(&command).expect("bounded command receipt");

    // Then: timeout is explicit and descendant cleanup is complete.
    assert_eq!(receipt.status, CommandStatus::TimedOut);
    assert!(receipt.cleanup.forced_termination);
    assert!(receipt.cleanup.surviving_pids.is_empty());
    assert!(receipt.duration_millis < 2_000);
}

#[test]
fn reference_cache_publishes_one_digest_valid_entry_under_concurrency() {
    // Given: two writers targeting the same content-addressed Grok key.
    let root = tempfile::tempdir().expect("cache root");
    let source = tempfile::tempdir().expect("capture source");
    std::fs::write(source.path().join("receipt.json"), b"reference").expect("source receipt");
    let cache = Arc::new(ReferenceCache::new(root.path()));
    let key = ReferenceCacheInputs::synthetic("same-key")
        .digest()
        .expect("cache key");

    // When: both writers publish concurrently.
    let handles = (0..2)
        .map(|_| {
            let cache = Arc::clone(&cache);
            let key = key.clone();
            let source = source.path().to_path_buf();
            std::thread::spawn(move || cache.publish(&key, &source))
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().expect("cache writer").expect("cache publish");
    }

    // Then: the sole published entry validates its artifact digests.
    let entry = cache
        .load(&key)
        .expect("cache lookup")
        .expect("published cache entry");
    assert_eq!(entry.artifact_count, 1);
}

#[test]
fn reference_binary_cache_stages_distinct_worker_launch_paths() {
    // Given: one immutable executable shared by concurrently starting workers.
    let root = tempfile::tempdir().expect("reference root");
    let source = root.path().join("reference-bin");
    std::fs::write(&source, b"#!/bin/sh\nprintf '%s\\n' worker\n").expect("source binary");
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o755))
        .expect("source executable");
    let digest = hash_bytes(&std::fs::read(&source).expect("source bytes")).expect("source digest");
    let cache = Arc::new(ReferenceBinaryCache::new(
        root.path().join("binary-cache"),
        source,
        digest,
    ));
    let barrier = Arc::new(Barrier::new(4));

    // When: workers stage and launch the same reference at the same time.
    let handles = (0..4)
        .map(|index| {
            let cache = Arc::clone(&cache);
            let barrier = Arc::clone(&barrier);
            let worker_root = root.path().join(format!("worker-{index}"));
            std::thread::spawn(move || {
                std::fs::create_dir_all(&worker_root).expect("worker root");
                barrier.wait();
                let staged = cache
                    .stage_for_worker(&worker_root)
                    .expect("worker-local reference");
                let output = std::process::Command::new(&staged)
                    .output()
                    .expect("staged reference launch");
                assert!(output.status.success());
                staged
            })
        })
        .collect::<Vec<_>>();
    let paths = handles
        .into_iter()
        .map(|handle| handle.join().expect("worker launch"))
        .collect::<BTreeSet<_>>();

    // Then: every launch uses a separate path while retaining identical content.
    assert_eq!(paths.len(), 4);
    assert!(paths.iter().all(|path| {
        hash_bytes(&std::fs::read(path).expect("staged bytes")).expect("staged digest")
            == cache.digest()
    }));
}
