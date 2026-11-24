use dtool_core::workspace::Workspace;

fn main() {
    let mut workspace = Workspace::default();
    workspace.new_document("Untitled");
    let workspace = workspace.into_shared();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dtool_cli::run_cli_args(&workspace, &args) {
        Ok(value) if args.first().is_some_and(|value| value == "catalog") => {
            if let Some(commands) = value.get("commands").and_then(serde_json::Value::as_array) {
                for command in commands { if let Some(name) = command.as_str() { println!("{}", name); } }
            }
        }
        Ok(value) => println!("{}", value),
        Err(error) => {
            println!("{}", serde_json::json!({ "error": { "code": error.code, "key": error.message_key } }));
            std::process::exit(1);
        }
    }
}
