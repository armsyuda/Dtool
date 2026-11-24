use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Accessibility {
    pub access_modes: Vec<String>,
    pub access_modes_sufficient: Vec<String>,
    pub features: Vec<String>,
    pub hazards: Vec<String>,
    pub summary: String,
    pub conformance: Option<String>,
    pub certified_by: String,
    pub certifier_credentials: String,
    pub certifier_report: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DocMetadata {
    pub title: String,
    pub author: String,
    pub subject: String,
    pub keywords: Vec<String>,
    pub copyright: String,
    pub description: String,
    pub accessibility: Accessibility,
    pub xmp_extra: Vec<(String, String)>,
}

impl DocMetadata {
    pub fn is_empty(&self) -> bool {
        self.title.is_empty()
            && self.author.is_empty()
            && self.subject.is_empty()
            && self.keywords.is_empty()
            && self.copyright.is_empty()
            && self.description.is_empty()
            && self.accessibility.summary.is_empty()
            && self.xmp_extra.is_empty()
    }
}

pub fn write_xmp(meta: &DocMetadata) -> String {
    let mut xmp = String::new();
    xmp.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xmp.push_str("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n");
    xmp.push_str("  <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n");
    xmp.push_str("    <rdf:Description xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmlns:a11y=\"http://www.w3.org/ns/a11y/\">\n");

    if !meta.title.is_empty() {
        xmp.push_str(&format!("      <dc:title>{}</dc:title>\n", escape_xml(&meta.title)));
    }
    if !meta.author.is_empty() {
        xmp.push_str(&format!("      <dc:creator>{}</dc:creator>\n", escape_xml(&meta.author)));
    }
    if !meta.subject.is_empty() {
        xmp.push_str(&format!("      <dc:description>{}</dc:description>\n", escape_xml(&meta.subject)));
    }
    if !meta.keywords.is_empty() {
        let kw = meta.keywords.join(", ");
        xmp.push_str(&format!("      <dc:subject>{}</dc:subject>\n", escape_xml(&kw)));
    }
    if !meta.copyright.is_empty() {
        xmp.push_str(&format!("      <dc:rights>{}</dc:rights>\n", escape_xml(&meta.copyright)));
    }
    if !meta.description.is_empty() {
        xmp.push_str(&format!("      <xmp:Description>{}</xmp:Description>\n", escape_xml(&meta.description)));
    }

    let acc = &meta.accessibility;
    if !acc.access_modes.is_empty() {
        xmp.push_str(&format!("      <a11y:accessModes>{}</a11y:accessModes>\n", escape_xml(&acc.access_modes.join(", "))));
    }
    if !acc.access_modes_sufficient.is_empty() {
        xmp.push_str(&format!("      <a11y:accessModesSufficient>{}</a11y:accessModesSufficient>\n", escape_xml(&acc.access_modes_sufficient.join(", "))));
    }
    if !acc.features.is_empty() {
        xmp.push_str(&format!("      <a11y:features>{}</a11y:features>\n", escape_xml(&acc.features.join(", "))));
    }
    if !acc.hazards.is_empty() {
        xmp.push_str(&format!("      <a11y:hazards>{}</a11y:hazards>\n", escape_xml(&acc.hazards.join(", "))));
    }
    if !acc.summary.is_empty() {
        xmp.push_str(&format!("      <a11y:summary>{}</a11y:summary>\n", escape_xml(&acc.summary)));
    }
    if let Some(ref conf) = acc.conformance {
        xmp.push_str(&format!("      <a11y:conformance>{}</a11y:conformance>\n", escape_xml(conf)));
    }
    if !acc.certified_by.is_empty() {
        xmp.push_str(&format!("      <a11y:certifiedBy>{}</a11y:certifiedBy>\n", escape_xml(&acc.certified_by)));
    }
    if !acc.certifier_credentials.is_empty() {
        xmp.push_str(&format!("      <a11y:certifierCredentials>{}</a11y:certifierCredentials>\n", escape_xml(&acc.certifier_credentials)));
    }
    if !acc.certifier_report.is_empty() {
        xmp.push_str(&format!("      <a11y:certifierReport>{}</a11y:certifierReport>\n", escape_xml(&acc.certifier_report)));
    }

    for (key, value) in &meta.xmp_extra {
        xmp.push_str(&format!("      <{}>{}</{}>\n", key, escape_xml(value), key));
    }

    xmp.push_str("    </rdf:Description>\n");
    xmp.push_str("  </rdf:RDF>\n");
    xmp.push_str("</x:xmpmeta>\n");
    xmp
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
