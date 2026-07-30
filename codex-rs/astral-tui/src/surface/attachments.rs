use crate::LinkTarget;
use crate::composer::LocalImage;
use crate::composer::PastePreview;
use crate::modal::ModalRow;
use crate::modal::ModalState;

use super::SurfaceState;

impl SurfaceState {
    pub(crate) fn composer_image_for_preview(&self) -> Option<LocalImage> {
        self.composer.local_image_for_preview()
    }

    pub(crate) fn composer_paste_for_preview(&self) -> Option<PastePreview<'_>> {
        self.composer.paste_for_preview()
    }

    pub(crate) fn open_composer_image_at_cursor(&mut self) -> bool {
        let Some(image) = self.composer.local_image_at_cursor() else {
            return false;
        };
        self.open_local_image(image);
        true
    }

    pub(crate) fn open_local_image(&mut self, image: LocalImage) {
        let dimensions = image
            .dimensions
            .or_else(|| image::image_dimensions(&image.path).ok())
            .map_or_else(
                || "unknown".to_string(),
                |(width, height)| format!("{width}×{height}"),
            );
        let byte_len = image.byte_len.or_else(|| {
            std::fs::metadata(&image.path)
                .ok()
                .map(|metadata| metadata.len())
        });
        let size = byte_len.map_or_else(|| "unknown".to_string(), format_bytes);
        let format = image
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            .map_or_else(|| "unknown".to_string(), str::to_uppercase);
        let target = LinkTarget::File(image.path.clone());
        self.open_modal(ModalState::openable_info(
            format!("Image #{}", image.display_number),
            vec![
                ModalRow::new("Format", format),
                ModalRow::new("Path", image.path.display().to_string()),
                ModalRow::new("Dimensions", dimensions),
                ModalRow::new("Size", size),
            ],
            target,
        ));
    }
}

fn format_bytes(byte_len: u64) -> String {
    if byte_len >= 1_000_000 {
        format!("{:.1} MB", byte_len as f64 / 1_000_000.0)
    } else if byte_len >= 1_000 {
        format!("{:.1} KB", byte_len as f64 / 1_000.0)
    } else {
        format!("{byte_len} bytes")
    }
}
