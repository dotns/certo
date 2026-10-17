use std::sync::{Arc, PoisonError, RwLock};

use hickory_server::{
    net::runtime::Time,
    proto::rr::LowerName,
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
    zone_handler::{Catalog, ZoneHandler},
};
use rustc_hash::FxHashMap;

/// A [`Catalog`] whose zones can be replaced while the server is running.
///
/// Every change builds a new catalog and swaps it in. Requests run against the snapshot
/// they started with and never hold a lock while awaiting.
#[derive(Clone, Default)]
pub struct DynamicCatalog {
    inner: Arc<RwLock<State>>,
}

#[derive(Default)]
struct State {
    zones: FxHashMap<LowerName, Arc<dyn ZoneHandler>>,
    catalog: Arc<Catalog>,
}

impl State {
    fn rebuild(&mut self) {
        let mut catalog = Catalog::default();
        for (origin, zone) in &self.zones {
            catalog.upsert(origin.clone(), vec![Arc::clone(zone)]);
        }
        self.catalog = Arc::new(catalog);
    }
}

impl std::fmt::Debug for DynamicCatalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynamicCatalog").finish_non_exhaustive()
    }
}

impl DynamicCatalog {
    /// Creates an empty catalog.
    pub fn new() -> Self {
        Self::default()
    }

    fn snapshot(&self) -> Arc<Catalog> {
        Arc::clone(
            &self
                .inner
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .catalog,
        )
    }

    fn update(
        &self,
        change: impl FnOnce(&mut FxHashMap<LowerName, Arc<dyn ZoneHandler>>) -> bool,
    ) -> bool {
        let mut state = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        let changed = change(&mut state.zones);
        if changed {
            state.rebuild();
        }
        changed
    }

    /// Inserts or replaces the handler for a zone.
    pub fn upsert(&self, zone: Arc<dyn ZoneHandler>) {
        let origin = zone.origin().clone();
        self.update(|zones| {
            zones.insert(origin, zone);
            true
        });
    }

    /// Removes a zone; returns whether it existed.
    pub fn remove(&self, origin: &LowerName) -> bool {
        self.update(|zones| zones.remove(origin).is_some())
    }

    /// Reports whether some loaded zone contains `name`.
    pub fn covers(&self, name: &LowerName) -> bool {
        self.snapshot().find(name).is_some()
    }

    /// Reports whether a zone with exactly this origin is loaded.
    pub fn contains(&self, origin: &LowerName) -> bool {
        self.snapshot().contains(origin)
    }
}

#[async_trait::async_trait]
impl RequestHandler for DynamicCatalog {
    async fn handle_request<R: ResponseHandler, T: Time>(
        &self,
        request: &Request,
        response_handle: R,
    ) -> ResponseInfo {
        self.snapshot()
            .handle_request::<R, T>(request, response_handle)
            .await
    }
}
