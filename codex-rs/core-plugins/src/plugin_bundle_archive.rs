use flate2::read::GzDecoder;
use std::fs;
use std::io;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use tar::Archive;

#[derive(Debug, thiserror::Error)]
pub(crate) enum PluginBundleUnpackError {
    #[error(
        "plugin bundle extracted size would be {bytes} bytes, exceeding maximum total size of {max_bytes} bytes"
    )]
    ExtractedBundleTooLarge { bytes: u64, max_bytes: u64 },

    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        #[source]
        source: io::Error,
    },

    #[error("{0}")]
    InvalidBundle(String),
}

impl PluginBundleUnpackError {
    fn io(context: &'static str, source: io::Error) -> Self {
        Self::Io { context, source }
    }
}

pub(crate) fn unpack_plugin_bundle_tar_gz(
    bytes: &[u8],
    destination: &Path,
    max_total_bytes: u64,
) -> Result<(), PluginBundleUnpackError> {
    fs::create_dir_all(destination).map_err(|source| {
        PluginBundleUnpackError::io(
            "failed to create plugin bundle extraction directory",
            source,
        )
    })?;

    let archive = GzDecoder::new(std::io::Cursor::new(bytes));
    let mut archive = Archive::new(archive);
    unpack_plugin_bundle_tar(&mut archive, destination, max_total_bytes)
}

fn unpack_plugin_bundle_tar<R: Read>(
    archive: &mut Archive<R>,
    destination: &Path,
    max_total_bytes: u64,
) -> Result<(), PluginBundleUnpackError> {
    let mut extracted_bytes = 0u64;
    let entries = archive.entries().map_err(|source| {
        PluginBundleUnpackError::io("failed to read plugin bundle tar", source)
    })?;
    for entry in entries {
        let mut entry = entry.map_err(|source| {
            PluginBundleUnpackError::io("failed to read plugin bundle tar entry", source)
        })?;
        let entry_type = entry.header().entry_type();
        let entry_size = entry.size();
        let entry_path = entry
            .path()
            .map_err(|source| {
                PluginBundleUnpackError::io("failed to read plugin bundle tar entry path", source)
            })?
            .into_owned();
        let output_path = checked_tar_output_path(destination, &entry_path)?;

        if entry_type.is_dir() {
            fs::create_dir_all(&output_path).map_err(|source| {
                PluginBundleUnpackError::io("failed to create plugin bundle directory", source)
            })?;
            continue;
        }

        if entry_type.is_file() {
            enforce_total_extracted_size(entry_size, &mut extracted_bytes, max_total_bytes)?;
            let Some(parent) = output_path.parent() else {
                return Err(PluginBundleUnpackError::InvalidBundle(format!(
                    "plugin bundle output path has no parent: {}",
                    output_path.display()
                )));
            };
            fs::create_dir_all(parent).map_err(|source| {
                PluginBundleUnpackError::io("failed to create plugin bundle directory", source)
            })?;
            entry.unpack(&output_path).map_err(|source| {
                PluginBundleUnpackError::io("failed to unpack plugin bundle entry", source)
            })?;
            continue;
        }

        if entry_type.is_hard_link() || entry_type.is_symlink() {
            return Err(PluginBundleUnpackError::InvalidBundle(format!(
                "plugin bundle tar entry `{}` is a link",
                entry_path.display()
            )));
        }

        return Err(PluginBundleUnpackError::InvalidBundle(format!(
            "plugin bundle tar entry `{}` has unsupported type {:?}",
            entry_path.display(),
            entry_type
        )));
    }

    Ok(())
}

fn checked_tar_output_path(
    destination: &Path,
    entry_name: &Path,
) -> Result<PathBuf, PluginBundleUnpackError> {
    let mut output_path = destination.to_path_buf();
    let mut has_component = false;
    for component in entry_name.components() {
        match component {
            std::path::Component::Normal(component) => {
                has_component = true;
                output_path.push(component);
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(PluginBundleUnpackError::InvalidBundle(format!(
                    "plugin bundle tar entry `{}` escapes extraction root",
                    entry_name.display()
                )));
            }
        }
    }
    if !has_component {
        return Err(PluginBundleUnpackError::InvalidBundle(
            "plugin bundle tar entry has an empty path".to_string(),
        ));
    }
    Ok(output_path)
}

fn enforce_total_extracted_size(
    entry_size: u64,
    extracted_bytes: &mut u64,
    max_total_bytes: u64,
) -> Result<(), PluginBundleUnpackError> {
    let next_total = extracted_bytes.checked_add(entry_size).ok_or(
        PluginBundleUnpackError::ExtractedBundleTooLarge {
            bytes: u64::MAX,
            max_bytes: max_total_bytes,
        },
    )?;
    if next_total > max_total_bytes {
        return Err(PluginBundleUnpackError::ExtractedBundleTooLarge {
            bytes: next_total,
            max_bytes: max_total_bytes,
        });
    }
    *extracted_bytes = next_total;
    Ok(())
}
