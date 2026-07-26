#[cfg(not(target_os = "android"))]
pub(crate) struct ClipboardLease {
    _clipboard: arboard::Clipboard,
}

#[cfg(target_os = "android")]
pub(crate) struct ClipboardLease;

#[cfg(not(target_os = "android"))]
pub(crate) fn copy_to_clipboard(text: &str) -> Result<ClipboardLease, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("clipboard unavailable: {error}"))?;
    clipboard
        .set_text(text)
        .map_err(|error| format!("failed to copy response: {error}"))?;
    Ok(ClipboardLease {
        _clipboard: clipboard,
    })
}

#[cfg(target_os = "android")]
pub(crate) fn copy_to_clipboard(_text: &str) -> Result<ClipboardLease, String> {
    Err("clipboard copy is unavailable on Android".to_string())
}
