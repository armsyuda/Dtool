use std::io;

use dtool_core::workspace::Workspace;
use dtool_mcp::{Config, Server, serve_stdio};

fn main() {
    let mut workspace = Workspace::default();
    workspace.new_document("Untitled");
    let mut server = Server::new(Config::default(), workspace);
    let stdin = io::stdin();
    let stdout = io::stdout();
    serve_stdio(&mut server, stdin.lock(), stdout);
}
