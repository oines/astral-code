//! Cloud-hosted configuration data for Astral.
//!
//! Astral does not enable the legacy ChatGPT-hosted configuration control plane
//! by default. Parsing and composition remain in `codex-config`; the old remote
//! transport is retained only for tests while the provider-neutral replacement is
//! designed.

#[cfg(test)]
mod backend;
mod bundle_loader;
#[cfg(test)]
mod cache;
#[cfg(test)]
mod metrics;
#[cfg(test)]
mod service;
#[cfg(test)]
mod validation;

pub use bundle_loader::cloud_config_bundle_loader;
pub use bundle_loader::cloud_config_bundle_loader_for_storage;
