use dtool_core::metadata::{Accessibility, DocMetadata};

pub fn read_xmp(bytes: &[u8]) -> DocMetadata {
    let text = String::from_utf8_lossy(bytes);
    let mut meta = DocMetadata::default();

    for (key, value) in parse_xmp_pairs(&text) {
        match key.as_str() {
            "dc:title" => meta.title = value,
            "dc:creator" => meta.author = value,
            "dc:description" => meta.description = value,
            "dc:subject" => meta.keywords = value.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            "xmpRights:UsageTerms" => meta.copyright = value,
            _ => meta.xmp_extra.push((key, value)),
        }
    }

    meta
}

pub fn write_xmp(meta: &DocMetadata) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n");
    out.push_str("  <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n");
    out.push_str("    <rdf:Description\n");
    out.push_str("      xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n");
    out.push_str("      xmlns:xmpRights=\"http://ns.adobe.com/xap/1.0/rights/\">\n");

    if !meta.title.is_empty() {
        out.push_str(&format!("      <dc:title>{}</dc:title>\n", escape_xml(&meta.title)));
    }
    if !meta.author.is_empty() {
        out.push_str(&format!("      <dc:creator>{}</dc:creator>\n", escape_xml(&meta.author)));
    }
    if !meta.description.is_empty() {
        out.push_str(&format!("      <dc:description>{}</dc:description>\n", escape_xml(&meta.description)));
    }
    if !meta.keywords.is_empty() {
        let joined = meta.keywords.join("; ");
        out.push_str(&format!("      <dc:subject>{}</dc:subject>\n", escape_xml(&joined)));
    }
    if !meta.copyright.is_empty() {
        out.push_str(&format!("      <xmpRights:UsageTerms>{}</xmpRights:UsageTerms>\n", escape_xml(&meta.copyright)));
    }

    for (key, value) in &meta.xmp_extra {
        out.push_str(&format!("      <{}>{}</{}>\n", key, escape_xml(value), key));
    }

    out.push_str("    </rdf:Description>\n");
    out.push_str("  </rdf:RDF>\n");
    out.push_str("</x:xmpmeta>\n");
    out
}

fn parse_xmp_pairs(text: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            while let Some(&c) = chars.peek() {
                if c == '>' || c == '/' {
                    break;
                }
                tag.push(c);
                chars.next();
            }

            if tag.starts_with('/') {
                continue;
            }

            let mut value = String::new();
            while let Some(&c) = chars.peek() {
                if c == '<' {
                    break;
                }
                value.push(c);
                chars.next();
            }

            if !value.is_empty() {
                pairs.push((tag.trim().to_string(), unescape_xml(value.trim())));
            }
        }
    }

    pairs
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}
