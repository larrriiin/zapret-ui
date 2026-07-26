mod channel;
mod installation;
mod manager;
mod paths;
mod provider;

pub use channel::{compare_versions, resolve_stable, CoreUpdateStatus};
pub use installation::{CoreInstallation, CoreInstallationState, CoreManifest};
pub use manager::CoreManager;
pub use paths::CorePaths;
pub use provider::{Checksum, CoreArtifact, CoreProvider, CoreRelease};
