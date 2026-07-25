use std::path::PathBuf;

use crate::providers::FlowsealProvider;

use super::{CoreProvider, CoreRelease};

/// Composition root for the active core. Flowseal is selected here only; UI
/// and Tauri command orchestration depend on the provider-neutral interface.
pub struct CoreManager {
    provider: FlowsealProvider,
}

impl CoreManager {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            provider: FlowsealProvider::new(root),
        }
    }

    pub fn provider(&self) -> &dyn CoreProvider {
        &self.provider
    }

    pub async fn fetch_fallback_release(
        &self,
        client: &reqwest::Client,
    ) -> Result<CoreRelease, String> {
        self.provider.fetch_fallback_release(client).await
    }

    pub fn fallback_latest_url(&self) -> &'static str {
        self.provider.fallback_latest_url()
    }
}
