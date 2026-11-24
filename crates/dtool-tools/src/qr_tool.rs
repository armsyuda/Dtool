use dtool_core::commands::qr::DEFAULT_PAYLOAD;
use dtool_core::Command;
use dtool_core::serde_json::json;

pub fn create(pos: [f32; 2], size: [f32; 2]) -> Command {
    Command::with_params("qr.create", json!({
        "payload": DEFAULT_PAYLOAD,
        "pos": pos,
        "size": size
    }))
}
