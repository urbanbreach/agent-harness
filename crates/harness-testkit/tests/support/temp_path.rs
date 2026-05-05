use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn unique_temp_file(prefix: &str, ext: &str) -> PathBuf {
    unique_temp_path(prefix, Some(ext))
}

pub(crate) fn create_unique_temp_dir(prefix: &str) -> PathBuf {
    let dir = unique_temp_path(prefix, None);
    fs::create_dir_all(&dir)
        .unwrap_or_else(|err| panic!("failed creating temp dir {}: {err}", dir.display()));
    dir
}

fn unique_temp_path(prefix: &str, ext: Option<&str>) -> PathBuf {
    static TEMP_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

    let base = env::temp_dir().join("harness-testkit");
    fs::create_dir_all(&base).expect("create base temp dir");

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();

    let sequence = TEMP_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = format!("{prefix}-{}-{nanos}-{sequence}", std::process::id());
    if let Some(ext) = ext {
        name.push('.');
        name.push_str(ext);
    }
    base.join(name)
}
