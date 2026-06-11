/// Legacy compatibility shim for Codex callers that used to attach a process-global ChatGPT
/// Cloudflare cookie jar.
///
/// Astral does not use ChatGPT's hosted backend as a control plane, so this intentionally returns
/// the builder unchanged and never installs a shared cookie store.
pub fn with_chatgpt_cloudflare_cookie_store(
    builder: reqwest::ClientBuilder,
) -> reqwest::ClientBuilder {
    builder
}
