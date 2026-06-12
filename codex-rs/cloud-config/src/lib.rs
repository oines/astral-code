//! Provider-neutral configuration bundle hook for Astral.
//!
//! Parsing and composition remain in `codex-config`; this crate exposes disabled
//! bundle loaders until a provider-neutral replacement exists.

mod bundle_loader;

pub use bundle_loader::cloud_config_bundle_loader;
pub use bundle_loader::cloud_config_bundle_loader_for_storage;
