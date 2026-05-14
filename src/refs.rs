use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefKind {
    Window,
    Element,
}

#[derive(Debug, Clone)]
pub struct RefMetadata {
    pub kind: RefKind,
    pub payload: String,
    pub minted_at: Instant,
}

#[derive(Debug, thiserror::Error)]
pub enum RefError {
    #[error("stale handle")]
    Stale,
    #[error("unknown handle")]
    Unknown,
}

/// Opaque-ref store for spec §4.5 poka-yoke. Refs are minted by observation
/// tools (Phase 1) and consumed by action tools that take `element_ref` /
/// `window_ref`. The TTL bounds how long a fabricated ref can collide with a
/// real one, but the primary guarantee is structural: tool schemas don't
/// accept anything but a ref minted by this store.
pub struct RefStore {
    inner: Arc<RwLock<Inner>>,
}

struct Inner {
    map: HashMap<String, RefMetadata>,
    ttl: Duration,
    next_id: u64,
}

impl RefStore {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner {
                map: HashMap::new(),
                ttl,
                next_id: 0,
            })),
        }
    }

    pub async fn mint(&self, kind: RefKind, payload: impl Into<String>) -> String {
        let mut g = self.inner.write().await;
        let prefix = match kind {
            RefKind::Window => "win",
            RefKind::Element => "el",
        };
        let id = format!("{prefix}#{:x}", g.next_id);
        g.next_id += 1;
        g.map.insert(
            id.clone(),
            RefMetadata {
                kind,
                payload: payload.into(),
                minted_at: Instant::now(),
            },
        );
        id
    }

    pub async fn resolve(&self, id: &str) -> Result<RefMetadata, RefError> {
        let g = self.inner.read().await;
        match g.map.get(id) {
            None => Err(RefError::Unknown),
            Some(m) if m.minted_at.elapsed() > g.ttl => Err(RefError::Stale),
            Some(m) => Ok(m.clone()),
        }
    }

    pub async fn purge_expired(&self) {
        let mut g = self.inner.write().await;
        let ttl = g.ttl;
        g.map.retain(|_, m| m.minted_at.elapsed() <= ttl);
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.map.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mint_then_resolve_returns_metadata() {
        let store = RefStore::new(Duration::from_secs(60));
        let id = store.mint(RefKind::Window, "kwin-uuid:abc").await;
        let m = store.resolve(&id).await.unwrap();
        assert_eq!(m.kind, RefKind::Window);
        assert_eq!(m.payload, "kwin-uuid:abc");
        assert!(id.starts_with("win#"));
    }

    #[tokio::test]
    async fn unknown_handle_errors() {
        let store = RefStore::new(Duration::from_secs(60));
        let err = store.resolve("nonsense#0").await.unwrap_err();
        assert!(matches!(err, RefError::Unknown));
    }

    #[tokio::test]
    async fn expired_handle_returns_stale() {
        let store = RefStore::new(Duration::from_millis(50));
        let id = store.mint(RefKind::Element, "atspi-path").await;
        tokio::time::sleep(Duration::from_millis(80)).await;
        let err = store.resolve(&id).await.unwrap_err();
        assert!(matches!(err, RefError::Stale));
    }

    #[tokio::test]
    async fn purge_removes_only_expired() {
        let store = RefStore::new(Duration::from_millis(50));
        let old = store.mint(RefKind::Window, "old").await;
        tokio::time::sleep(Duration::from_millis(80)).await;
        let fresh = store.mint(RefKind::Window, "fresh").await;
        store.purge_expired().await;
        assert!(matches!(store.resolve(&old).await, Err(RefError::Unknown)));
        assert!(store.resolve(&fresh).await.is_ok());
        assert_eq!(store.len().await, 1);
    }

    #[tokio::test]
    async fn window_and_element_have_distinct_prefixes() {
        let store = RefStore::new(Duration::from_secs(60));
        let w = store.mint(RefKind::Window, "w").await;
        let e = store.mint(RefKind::Element, "e").await;
        assert!(w.starts_with("win#"));
        assert!(e.starts_with("el#"));
    }
}
