use std::process::Command;
use std::process::Stdio;

use crate::LinkTarget;

pub(crate) fn open_link(target: &LinkTarget) -> Result<(), String> {
    if let Ok(path) = std::env::var("ASTRAL_TUI_TEST_OPEN_LINK_FILE") {
        use std::io::Write as _;

        return std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| writeln!(file, "{}", target.display()))
            .map_err(|error| format!("Could not record link open: {error}"));
    }

    if let LinkTarget::Url(url) = target
        && !is_safe_url(url)
    {
        return Err("Refused to open an unsupported link".to_string());
    }

    #[cfg(all(not(any(target_os = "macos", target_os = "windows")), not(test)))]
    if std::env::var_os("DISPLAY").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_none()
        && std::env::var_os("BROWSER").is_none()
    {
        return Err(format!(
            "Could not open link. Open manually: {}",
            target.display()
        ));
    }

    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = Command::new("cmd");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = Command::new("xdg-open");

    #[cfg(target_os = "windows")]
    command.args(["/c", "start", ""]);
    match target {
        LinkTarget::Url(url) => command.arg(url),
        LinkTarget::File(path) => command.arg(path),
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open {}: {error}", target.display()))
}

fn is_safe_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}
