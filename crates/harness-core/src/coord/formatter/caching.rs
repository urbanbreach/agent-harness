use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;

use super::{DiscoveryContext, FormatterDiscovery};

type CacheKey = (String, PathBuf);

/// Discovery wrapper that caches `resolve` results per `(formatter_name, target_dir)`.
///
/// The cache uses a standard `Mutex` for interior mutability. The lock is never held
/// across an `.await` point: each call locks only to clone an existing result or to
/// insert a newly-computed one.
#[derive(Debug)]
pub struct CachingFormatterDiscovery<D> {
    inner: D,
    cache: Mutex<HashMap<CacheKey, Option<Vec<String>>>>,
}

impl<D: FormatterDiscovery> CachingFormatterDiscovery<D> {
    pub fn new(inner: D) -> Self {
        Self {
            inner,
            cache: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl<D: FormatterDiscovery> FormatterDiscovery for CachingFormatterDiscovery<D> {
    async fn resolve(&self, name: &str, context: &DiscoveryContext) -> Option<Vec<String>> {
        let key = (name.to_string(), context.target_dir.clone());

        let cached = {
            let guard = match self.cache.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.get(&key).cloned()
        };

        if let Some(result) = cached {
            return result;
        }

        let result = self.inner.resolve(name, context).await;

        {
            let mut guard = match self.cache.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.insert(key, result.clone());
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::super::{DiscoveryContext, FormatterDiscovery};
    use super::CachingFormatterDiscovery;

    #[derive(Debug)]
    struct CountingDiscovery {
        count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl FormatterDiscovery for CountingDiscovery {
        async fn resolve(&self, name: &str, _context: &DiscoveryContext) -> Option<Vec<String>> {
            self.count.fetch_add(1, Ordering::SeqCst);
            Some(vec![name.to_string(), "$FILE".to_string()])
        }
    }

    #[tokio::test]
    async fn caching_discovery_deduplicates_identical_requests() {
        // arrange
        let count = Arc::new(AtomicUsize::new(0));
        let inner = CountingDiscovery {
            count: Arc::clone(&count),
        };
        let caching = CachingFormatterDiscovery::new(inner);
        let context = DiscoveryContext {
            workspace_root: PathBuf::from("."),
            target_dir: PathBuf::from("."),
            experimental_oxfmt: false,
        };

        // act
        let first = caching.resolve("rustfmt", &context).await;
        let second = caching.resolve("rustfmt", &context).await;

        // assert
        assert!(first.is_some());
        assert_eq!(first, second);
        assert_eq!(count.load(Ordering::SeqCst), 1, "inner resolve called once");
    }

    #[tokio::test]
    async fn caching_discovery_separates_keys_by_name_and_target_dir() {
        // arrange
        let count = Arc::new(AtomicUsize::new(0));
        let inner = CountingDiscovery {
            count: Arc::clone(&count),
        };
        let caching = CachingFormatterDiscovery::new(inner);
        let ctx_a = DiscoveryContext {
            workspace_root: PathBuf::from("."),
            target_dir: PathBuf::from("a"),
            experimental_oxfmt: false,
        };
        let ctx_b = DiscoveryContext {
            workspace_root: PathBuf::from("."),
            target_dir: PathBuf::from("b"),
            experimental_oxfmt: false,
        };

        // act
        caching.resolve("rustfmt", &ctx_a).await;
        caching.resolve("rustfmt", &ctx_b).await;
        caching.resolve("rustfmt", &ctx_a).await;
        caching.resolve("prettier", &ctx_a).await;

        // assert
        assert_eq!(count.load(Ordering::SeqCst), 3, "three distinct keys");
    }
}
