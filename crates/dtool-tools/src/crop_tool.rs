use dtool_core::crop::{CropMode, CropOverlay, CropPreview, crop_preview, straighten_angle};
use dtool_core::Document;
use dtool_protocol::CommandError;

pub struct CropTool {
    pub preview: Option<CropPreview>,
    pub doc_version_at_preview: u64,
}

impl CropTool {
    pub fn new() -> Self {
        Self { 
            preview: None,
            doc_version_at_preview: 0,
        }
    }

    pub fn set_preview(&mut self, doc: &Document, p: CropPreview) -> Result<(), CommandError> {
        let validated = crop_preview(doc, &p)?;
        self.preview = Some(validated);
        self.doc_version_at_preview = doc.version;
        Ok(())
    }

    pub fn clear_preview(&mut self) {
        self.preview = None;
        self.doc_version_at_preview = 0;
    }

    pub fn get_preview(&self) -> Option<&CropPreview> {
        self.preview.as_ref()
    }

    pub fn straighten_from_line(&mut self, line: [[f32; 2]; 2]) {
        if let Some(ref mut p) = self.preview {
            p.rotation_deg = straighten_angle(line);
        }
    }

    pub fn apply_crop(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        if self.preview.is_none() {
            return Err(CommandError::validation("No crop preview active"));
        }

        if self.doc_version_at_preview != doc.version {
            return Err(CommandError::validation("STALE_PREVIEW"));
        }

        let preview = self.preview.take().unwrap();
        
        dtool_core::crop::apply_crop(doc, preview.rect, preview.rotation_deg)?;
        
        Ok(())
    }

    pub fn cancel_crop(&mut self) {
        self.clear_preview();
    }
}
