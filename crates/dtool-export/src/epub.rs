use std::io::{Cursor, Write};
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use zip::write::{FileOptions, SimpleFileOptions};
use zip::CompressionMethod;
use dtool_core::Document;
use dtool_protocol::CommandError;
use crate::{DocumentExportExt, ObjectExportExt};

const IMAGE_EXT: &str = "png";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EpubKind {
    FixedLayout,
    Reflowable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpubOptions {
    pub kind: EpubKind,
    pub page_size: [u32; 2],
    pub dpi: u32,
    pub raster_dpi: u32,
}

enum EpubItem {
    Text { text: String, x: f32, y: f32, size: Option<f32> },
    Image { href: String, x: f32, y: f32, w: f32, h: f32 },
}

struct EpubImage {
    href: String,
    id: String,
    encoded_bytes: Vec<u8>,
}

fn zip_error(e: impl std::fmt::Display) -> CommandError {
    crate::export_io_error(e)
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn encode_png(object: &dtool_core::Object) -> Option<Vec<u8>> {
    let data = object.raster_data.as_ref()?;
    let (w, h) = object.raster_size?;
    let img = image::RgbaImage::from_raw(w, h, data.clone())?;
    let mut bytes = Cursor::new(Vec::new());
    img.write_to(&mut bytes, image::ImageFormat::Png).ok()?;
    Some(bytes.into_inner())
}

fn collect_page_items(doc: &Document, page_index: usize, images: &mut Vec<EpubImage>) -> Vec<EpubItem> {
    let mut items = Vec::new();
    if doc.objects.is_empty() {
        return items;
    }
    for obj in doc.page_objects(page_index) {
        if !crate::export_visible(doc, obj) {
            continue;
        }
        match &obj.kind {
            dtool_core::ObjectKind::Text(_)
            | dtool_core::ObjectKind::ArtText { .. }
            | dtool_core::ObjectKind::FrameText { .. }
            | dtool_core::ObjectKind::PathText(_) => {
                if let Some(text) = obj.text().map(|t| t.to_string()).or_else(|| doc.object_text(obj)) {
                    items.push(EpubItem::Text { text, x: obj.x(), y: obj.y(), size: obj.font_size() });
                }
            }
            _ if obj.raster_data.is_some() => {
                if let Some(png) = encode_png(obj) {
                    let id = format!("image{}", obj.id.0);
                    let href = format!("images/{}.{}", id, IMAGE_EXT);
                    items.push(EpubItem::Image { href: href.clone(), x: obj.x(), y: obj.y(), w: obj.width(), h: obj.height() });
                    if !images.iter().any(|image| image.id == id) {
                        images.push(EpubImage { href, id, encoded_bytes: png });
                    }
                }
            }
            _ => {}
        }
    }
    items
}

pub fn export_epub(
    doc: &Document,
    path: PathBuf,
    opts: &EpubOptions,
) -> Result<u64, CommandError> {
    let file = std::fs::File::create(&path).map_err(zip_error)?;
    let mut zip = zip::ZipWriter::new(file);

    let mimetype_options: SimpleFileOptions = FileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644);

    zip.start_file("mimetype", mimetype_options).map_err(zip_error)?;
    zip.write_all(b"application/epub+zip").map_err(zip_error)?;

    let container_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

    let default_options: SimpleFileOptions = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    zip.start_file("META-INF/container.xml", default_options).map_err(zip_error)?;
    zip.write_all(container_xml.as_bytes()).map_err(zip_error)?;

    let pages = &doc.pages;
    let page_count = pages.len();

    let mut spine_items = Vec::new();
    let mut manifest_items = Vec::new();
    let mut nav_links = Vec::new();
    let mut images: Vec<EpubImage> = Vec::new();

    for (i, page) in pages.iter().enumerate() {
        let id = format!("page{}", i + 1);
        let href = format!("page{}.xhtml", i + 1);
        
        spine_items.push(format!("<itemref idref=\"{}\"/>", id));
        manifest_items.push(format!("<item id=\"{}\" href=\"{}\" media-type=\"application/xhtml+xml\"/>", id, href));

        let items = collect_page_items(doc, i, &mut images);
        let xhtml_content = match opts.kind {
            EpubKind::FixedLayout => generate_fixed_page(page, &items, i + 1, opts),
            EpubKind::Reflowable => generate_reflowable_page(page, &items, i + 1, opts),
        };

        zip.start_file(format!("OEBPS/{}", href), default_options).map_err(zip_error)?;
        zip.write_all(xhtml_content.as_bytes()).map_err(zip_error)?;

        nav_links.push(format!("<li><a href=\"{}\">Page {}</a></li>", href, i + 1));
    }

    for image in &images {
        manifest_items.push(format!("<item id=\"{}\" href=\"{}\" media-type=\"image/png\"/>", image.id, image.href));
        zip.start_file(format!("OEBPS/{}", image.href), mimetype_options).map_err(zip_error)?;
        zip.write_all(&image.encoded_bytes).map_err(zip_error)?;
    }
    manifest_items.push("<item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>".to_string());

    let nav_xhtml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Navigation</title></head>
<body>
<nav epub:type="toc"><ol>{}</ol></nav>
</body>
</html>"#,
        nav_links.join("")
    );

    zip.start_file("OEBPS/nav.xhtml", default_options).map_err(zip_error)?;
    zip.write_all(nav_xhtml.as_bytes()).map_err(zip_error)?;

    let layout = match opts.kind {
        EpubKind::FixedLayout => "pre-paginated",
        EpubKind::Reflowable => "reflowable",
    };

    let identifier = format!("urn:dtool:{}:{}", page_count, path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());

    let content_opf = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="pub-id">{}</dc:identifier>
    <dc:title>Exported Document</dc:title>
    <dc:language>en</dc:language>
    <meta property="rendition:layout">{}</meta>
    <meta property="rendition:orientation">auto</meta>
    <meta property="rendition:spread">none</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    {}
  </manifest>
  <spine>
    {}
  </spine>
</package>"#,
        escape_xml(&identifier),
        layout,
        manifest_items.join("\n    "),
        spine_items.join("\n    ")
    );

    zip.start_file("OEBPS/content.opf", default_options).map_err(zip_error)?;
    zip.write_all(content_opf.as_bytes()).map_err(zip_error)?;

    let css = match opts.kind {
        EpubKind::FixedLayout => format!(
            "body {{ margin: 0; padding: 0; width: {}px; height: {}px; overflow: hidden; position: relative; }}",
            opts.page_size[0], opts.page_size[1]
        ),
        EpubKind::Reflowable => "body { margin: 0; padding: 1em; } img { max-width: 100%; }".to_string(),
    };

    zip.start_file("OEBPS/style.css", default_options).map_err(zip_error)?;
    zip.write_all(css.as_bytes()).map_err(zip_error)?;

    zip.finish().map_err(zip_error)?;

    let metadata = std::fs::metadata(&path).map_err(zip_error)?;
    Ok(metadata.len())
}

fn generate_fixed_page(page: &dtool_core::document::Page, items: &[EpubItem], page_num: usize, opts: &EpubOptions) -> String {
    let w = opts.page_size[0];
    let h = opts.page_size[1];
    let scale_x = if page.size[0] > 0.0 && w > 0 { w as f32 / page.size[0] } else { 1.0 };
    let scale_y = if page.size[1] > 0.0 && h > 0 { h as f32 / page.size[1] } else { 1.0 };
    
    let mut body_content = String::new();

    for item in items {
        match item {
            EpubItem::Text { text, x, y, size } => {
                let font = size.map(|s| format!(" font-size: {}px;", s * scale_y)).unwrap_or_default();
                body_content.push_str(&format!(
                    "<div style=\"position: absolute; left: {}px; top: {}px;{}\">{}</div>",
                    x * scale_x, y * scale_y, font, escape_xml(text).replace('\n', "<br/>")
                ));
            }
            EpubItem::Image { href, x, y, w, h } => {
                body_content.push_str(&format!(
                    "<img style=\"position: absolute; left: {}px; top: {}px; width: {}px; height: {}px;\" src=\"{}\" alt=\"\"/>",
                    x * scale_x, y * scale_y, w * scale_x, h * scale_y, href
                ));
            }
        }
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Page {}</title><meta name="viewport" content="width={}, height={}"/><link rel="stylesheet" href="style.css"/></head>
<body>{}</body>
</html>"#,
        page_num, w, h, body_content
    )
}

fn generate_reflowable_page(page: &dtool_core::document::Page, items: &[EpubItem], page_num: usize, opts: &EpubOptions) -> String {
    let mut body_content = String::new();
    let _ = (page.size, opts.dpi);

    for item in items {
        match item {
            EpubItem::Text { text, .. } => {
                for paragraph in text.split('\n') {
                    body_content.push_str(&format!("<p>{}</p>", escape_xml(paragraph)));
                }
            }
            EpubItem::Image { href, .. } => {
                body_content.push_str(&format!("<img src=\"{}\" alt=\"\"/>", href));
            }
        }
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Page {}</title><link rel="stylesheet" href="style.css"/></head>
<body>{}</body>
</html>"#,
        page_num, body_content
    )
}
