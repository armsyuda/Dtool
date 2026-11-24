use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
use dtool_core::commands::qr::{self, QrContent, WifiSecurity};
use dtool_core::workspace::ContextId;
use dtool_core::{Command, ObjectId, Session};
use dtool_strings::text;
use serde_json::json;

#[derive(Clone)]
struct Draft {
    context: Option<ContextId>,
    object: ObjectId,
    content: QrContent,
    invalid: bool,
}

impl Default for Draft {
    fn default() -> Self {
        Self { context: None, object: ObjectId::default(), content: QrContent::Text { value: String::new() }, invalid: false }
    }
}

fn draft_key() -> egui::Id {
    egui::Id::new("dtool.qr.editor.draft")
}

pub fn context_button(ui: &mut egui::Ui, session: &Session) {
    let [id] = session.selection.ids.as_slice() else { return };
    let Some(object) = session.document.get_object(*id) else { return };
    let Some(payload) = qr::payload(object) else { return };
    let button = dtool_design::button_with(ui, text("qr.edit"), &dtool_design::ControlState::enabled(!object.locked));
    if button.clicked() {
        let draft = Draft {
            context: session.context,
            object: *id,
            content: object.qr_content.clone().unwrap_or_else(|| QrContent::from_payload(payload)),
            invalid: false,
        };
        ui.ctx().data_mut(|data| data.insert_temp(draft_key(), draft));
    }
}

pub fn show(ctx: &egui::Context, session: &Session) -> Option<Command> {
    let mut draft = ctx.data_mut(|data| data.remove_temp::<Draft>(draft_key()))?;
    if session.context != draft.context || !session.document.get_object(draft.object).is_some_and(|object| !object.locked && qr::payload(object).is_some()) {
        return None;
    }
    let (result, (ok, cancel)) = show_modal(ctx, "dtool.qr.editor", text("qr.edit"), ClosePolicy::CancelOnly, false, |ui| {
        ui.set_min_width(320.0);
        ui.horizontal(|ui| {
            ui.label(text("qr.type"));
            egui::ComboBox::from_id_salt("qr_type").selected_text(text(kind(&draft.content))).show_ui(ui, |ui| {
                for template in templates() {
                    let key = kind(&template);
                    let chosen = kind(&draft.content) == key;
                    if ui.selectable_label(chosen, text(key)).clicked() && !chosen {
                        draft.content = template;
                        draft.invalid = false;
                    }
                }
            });
        });
        let key = kind(&draft.content);
        if ui.push_id(key, |ui| fields(ui, &mut draft.content)).inner {
            draft.invalid = false;
        }
        if draft.invalid {
            ui.label(text("qr.invalid"));
        }
        ui.horizontal(|ui| {
            if ui.button(text("qr.clear")).clicked() {
                if let Some(template) = templates().into_iter().find(|template| kind(template) == key) {
                    draft.content = template;
                }
                draft.invalid = false;
            }
            let ok = ui.button(text("button.ok")).clicked();
            let cancel = ui.button(text("button.cancel")).clicked();
            (ok, cancel)
        }).inner
    });
    if cancel || result == DialogResult::Cancel {
        return None;
    }
    if ok {
        if draft.content.payload().and_then(|value| qr::encode(&value)).is_ok() {
            return Some(Command::with_params("qr.update", json!({"id": draft.object.0, "content": draft.content})));
        }
        draft.invalid = true;
    }
    ctx.data_mut(|data| data.insert_temp(draft_key(), draft));
    None
}

fn kind(content: &QrContent) -> &'static str {
    match content {
        QrContent::Text { .. } => "qr.text",
        QrContent::Url { .. } => "qr.url",
        QrContent::Phone { .. } => "qr.phone",
        QrContent::Sms { .. } => "qr.sms",
        QrContent::Email { .. } => "qr.email",
        QrContent::Whatsapp { .. } => "qr.whatsapp",
        QrContent::Facetime { .. } => "qr.facetime",
        QrContent::Location { .. } => "qr.location",
        QrContent::Wifi { .. } => "qr.wifi",
        QrContent::Vcard { .. } => "qr.vcard",
    }
}

fn templates() -> [QrContent; 10] {
    let blank = String::new;
    [
        QrContent::Text { value: blank() },
        QrContent::Url { value: blank() },
        QrContent::Phone { number: blank() },
        QrContent::Sms { number: blank(), message: blank() },
        QrContent::Email { address: blank(), subject: blank(), message: blank() },
        QrContent::Whatsapp { number: blank(), message: blank() },
        QrContent::Facetime { recipient: blank() },
        QrContent::Location { latitude: blank(), longitude: blank() },
        QrContent::Wifi { ssid: blank(), security: WifiSecurity::default(), password: blank(), hidden: false },
        QrContent::Vcard { given: blank(), family: blank(), number: blank(), address: blank(), organization: blank() },
    ]
}

fn fields(ui: &mut egui::Ui, content: &mut QrContent) -> bool {
    match content {
        QrContent::Text { value } => multiline(ui, "qr.payload", value, 4),
        QrContent::Url { value } => single(ui, "qr.url", value),
        QrContent::Phone { number } => single(ui, "qr.phone", number),
        QrContent::Sms { number, message } => single(ui, "qr.phone", number) | body(ui, message),
        QrContent::Email { address, subject, message } => {
            single(ui, "qr.address", address) | single(ui, "qr.subject", subject) | body(ui, message)
        }
        QrContent::Whatsapp { number, message } => single(ui, "qr.international_phone", number) | body(ui, message),
        QrContent::Facetime { recipient } => single(ui, "qr.recipient", recipient),
        QrContent::Location { latitude, longitude } => single(ui, "qr.latitude", latitude) | single(ui, "qr.longitude", longitude),
        QrContent::Wifi { ssid, security, password, hidden } => {
            let mut changed = single(ui, "qr.ssid", ssid);
            ui.horizontal(|ui| {
                ui.label(text("qr.security"));
                let options = [(WifiSecurity::Wpa, "WPA/WPA2"), (WifiSecurity::Wep, "WEP"), (WifiSecurity::Open, text("qr.open_network"))];
                let current = options.iter().find(|(mode, _)| *mode == *security).map_or("", |(_, label)| *label);
                egui::ComboBox::from_id_salt("wifi_security").selected_text(current).show_ui(ui, |ui| {
                    for (mode, label) in options {
                        changed |= ui.selectable_value(security, mode, label).changed();
                    }
                });
            });
            if *security != WifiSecurity::Open {
                changed |= edit(ui, "qr.password", egui::TextEdit::singleline(password).password(true)).changed();
            } else {
                password.clear();
            }
            changed | ui.checkbox(hidden, text("qr.hidden")).changed()
        }
        QrContent::Vcard { given, family, number, address, organization } => {
            single(ui, "qr.given", given) | single(ui, "qr.family", family)
                | single(ui, "qr.phone", number) | single(ui, "qr.address", address)
                | single(ui, "qr.organization", organization)
        }
    }
}

fn body(ui: &mut egui::Ui, value: &mut String) -> bool {
    multiline(ui, "qr.message", value, 3)
}

fn multiline(ui: &mut egui::Ui, key: &str, value: &mut String, rows: usize) -> bool {
    let response = edit(ui, key, egui::TextEdit::multiline(value).desired_rows(rows));
    dtool_design::mark_multiline(ui, &response);
    response.changed()
}

fn single(ui: &mut egui::Ui, label: &str, value: &mut String) -> bool {
    edit(ui, label, egui::TextEdit::singleline(value)).changed()
}

fn edit(ui: &mut egui::Ui, key: &str, input: egui::TextEdit<'_>) -> egui::Response {
    let label = ui.label(text(key)).id;
    ui.add(input.id_salt(key).desired_width(320.0)).labelled_by(label)
}
