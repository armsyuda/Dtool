use crate::CommandError;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum QrContent {
    Text { value: String },
    Url { value: String },
    Phone { number: String },
    Sms { number: String, #[serde(default)] message: String },
    Email { address: String, #[serde(default)] subject: String, #[serde(default)] message: String },
    Whatsapp { number: String, #[serde(default)] message: String },
    Facetime { recipient: String },
    Location { latitude: String, longitude: String },
    Wifi { ssid: String, security: WifiSecurity, #[serde(default)] password: String, #[serde(default)] hidden: bool },
    Vcard { given: String, family: String, #[serde(default)] number: String, #[serde(default)] address: String, #[serde(default)] organization: String },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiSecurity {
    #[default]
    Wpa,
    Wep,
    Open,
}

impl QrContent {
    pub fn from_payload(value: &str) -> Self {
        if is_web_url(value) { Self::Url { value: value.to_owned() } } else { Self::Text { value: value.to_owned() } }
    }

    pub fn payload(&self) -> Result<String, CommandError> {
        let invalid = CommandError::invalid_params;
        let result = match self {
            Self::Text { value } => value.clone(),
            Self::Url { value } => {
                let value = value.trim();
                if !is_web_url(value) { return Err(invalid()); }
                value.to_owned()
            }
            Self::Phone { number } => format!("tel:{}", phone(number).ok_or_else(invalid)?),
            Self::Sms { number, message } => {
                let to = phone(number).ok_or_else(invalid)?;
                with_query(format!("sms:{to}"), &[("body", message)])
            }
            Self::Email { address, subject, message } => {
                let to = email(address).ok_or_else(invalid)?;
                with_query(format!("mailto:{}", percent(&to, "@")), &[("subject", subject), ("body", message)])
            }
            Self::Whatsapp { number, message } => {
                let number = phone(number).filter(|number| number.starts_with('+')).ok_or_else(invalid)?;
                let digits = &number[1..];
                if digits.starts_with('0') { return Err(invalid()); }
                with_query(format!("https://wa.me/{digits}"), &[("text", message)])
            }
            Self::Facetime { recipient } => {
                let value = phone(recipient).or_else(|| email(recipient).map(|value| percent(&value, "@"))).ok_or_else(invalid)?;
                format!("facetime:{value}")
            }
            Self::Location { latitude, longitude } => {
                let lat = coordinate(latitude, 90.0).ok_or_else(invalid)?;
                let lon = coordinate(longitude, 180.0).ok_or_else(invalid)?;
                format!("geo:{lat},{lon}")
            }
            Self::Wifi { ssid, security, password, hidden } => {
                if ssid.is_empty() || ssid.chars().any(char::is_control) { return Err(invalid()); }
                let mode = match security {
                    WifiSecurity::Wpa => "WPA",
                    WifiSecurity::Wep => "WEP",
                    WifiSecurity::Open => "nopass",
                };
                let mut encoded = format!("WIFI:T:{mode};S:{};", wifi_escape(ssid));
                if *security != WifiSecurity::Open {
                    if password.is_empty() || password.chars().any(char::is_control) { return Err(invalid()); }
                    let _ = write!(encoded, "P:{};", wifi_escape(password));
                }
                if *hidden { encoded.push_str("H:true;"); }
                encoded.push(';');
                encoded
            }
            Self::Vcard { given, family, number, address, organization } => {
                let name = [given.trim(), family.trim()].into_iter().filter(|value| !value.is_empty()).collect::<Vec<_>>().join(" ");
                if name.is_empty() { return Err(invalid()); }
                let mut card = format!("BEGIN:VCARD\r\nVERSION:4.0\r\nFN:{}\r\nN:{};{};;;\r\n",
                    vcard_escape(&name), vcard_escape(family.trim()), vcard_escape(given.trim()));
                if !number.trim().is_empty() {
                    let _ = write!(card, "TEL;VALUE=uri:tel:{}\r\n", phone(number).ok_or_else(invalid)?);
                }
                if !address.trim().is_empty() {
                    let address = email(address).ok_or_else(invalid)?;
                    let _ = write!(card, "EMAIL:{}\r\n", vcard_escape(&address));
                }
                if !organization.is_empty() {
                    let _ = write!(card, "ORG:{}\r\n", vcard_escape(organization));
                }
                card.push_str("END:VCARD\r\n");
                card
            }
        };
        Ok(result)
    }
}

fn is_web_url(value: &str) -> bool {
    value.strip_prefix("https://").or_else(|| value.strip_prefix("http://"))
        .is_some_and(|rest| !rest.is_empty() && !rest.starts_with(['/', '?', '#'])
            && !rest.chars().any(|c| c.is_whitespace() || c.is_control()))
}

fn phone(value: &str) -> Option<String> {
    let number: String = value.trim().chars().filter(|c| !matches!(c, ' ' | '-' | '.' | '(' | ')')).collect();
    let digits = number.strip_prefix('+').unwrap_or(&number);
    (!digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit())).then_some(number)
}

fn email(value: &str) -> Option<String> {
    let value = value.trim();
    let (user, domain) = value.split_once('@')?;
    (!user.is_empty() && !domain.is_empty() && !domain.contains('@')
        && !value.chars().any(|c| c.is_whitespace() || c.is_control())).then(|| value.to_owned())
}

fn coordinate(value: &str, limit: f64) -> Option<f64> {
    let value: f64 = value.trim().parse().ok()?;
    (-limit..=limit).contains(&value).then_some(value)
}

fn percent(value: &str, extra: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) || extra.as_bytes().contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn with_query(mut url: String, fields: &[(&str, &str)]) -> String {
    let mut separator = '?';
    for &(name, value) in fields.iter().filter(|(_, value)| !value.is_empty()) {
        let _ = write!(url, "{separator}{name}={}", percent(value, ""));
        separator = '&';
    }
    url
}

fn wifi_escape(value: &str) -> String {
    let mut encoded = String::new();
    for c in value.chars() {
        if matches!(c, '\\' | ';' | ',' | ':' | '"') { encoded.push('\\'); }
        encoded.push(c);
    }
    encoded
}

fn vcard_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace("\r\n", "\n").replace('\r', "\n")
        .replace('\n', "\\n").replace(';', "\\;").replace(',', "\\,")
}
