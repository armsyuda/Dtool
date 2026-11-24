use dtool_core::Workspace;
use dtool_mcp::{serve_stdio, Config, Server, UI_MAX_REQUEST_BYTES};
use serde_json::{json, Value};
use std::io::{BufRead, Read};

struct EndlessLine {
    served: usize,
    cap: usize,
    chunk: Vec<u8>,
}

impl Read for EndlessLine {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let available = self.fill_buf()?;
        let n = available.len().min(out.len());
        out[..n].copy_from_slice(&available[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for EndlessLine {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.served >= self.cap {
            return Err(std::io::Error::other("reader drained past the request cap"));
        }
        let n = self.chunk.len().min(self.cap - self.served);
        Ok(&self.chunk[..n])
    }

    fn consume(&mut self, amount: usize) {
        self.served += amount;
    }
}

fn server() -> Server {
    let mut workspace = Workspace::default();
    workspace.new_document("c15-stdio-cap");
    let server = Server::new(Config::default(), workspace);
    server.gui_connection_handle().set_from_gui(true);
    server
}

fn replies(output: &[u8]) -> Vec<Value> {
    output.split(|b| *b == b'\n').filter(|line| !line.is_empty()).map(|line| serde_json::from_slice(line).unwrap()).collect()
}

#[test]
fn oversized_line_is_rejected_before_the_whole_line_is_read() {
    let mut server = server();
    let reader = EndlessLine { served: 0, cap: UI_MAX_REQUEST_BYTES * 2, chunk: vec![b'a'; 8192] };
    let mut output = Vec::new();
    serve_stdio(&mut server, reader, &mut output);
    let replies = replies(&output);
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(replies[0]["error"]["code"], "REQUEST_TOO_LARGE");
}

#[test]
fn request_after_an_oversized_line_is_still_served() {
    let mut server = server();
    let mut input = vec![b'x'; UI_MAX_REQUEST_BYTES + 10];
    input.push(b'\n');
    input.extend_from_slice(serde_json::to_string(&json!({"jsonrpc": "2.0", "id": 7, "method": "dtool.query", "params": {"name": "app.version"}})).unwrap().as_bytes());
    input.push(b'\n');
    let mut output = Vec::new();
    serve_stdio(&mut server, std::io::Cursor::new(input), &mut output);
    let replies = replies(&output);
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert_eq!(replies[0]["error"]["code"], "REQUEST_TOO_LARGE");
    assert_eq!(replies[1]["id"], 7);
    assert!(replies[1].get("error").is_none() || replies[1]["error"].is_null(), "{}", replies[1]);
}

#[test]
fn line_at_the_cap_is_parsed_and_crlf_is_accepted() {
    let mut server = server();
    let request = serde_json::to_string(&json!({"jsonrpc": "2.0", "id": 8, "method": "dtool.query", "params": {"name": "app.version"}})).unwrap();
    let mut input = request.clone().into_bytes();
    input.extend_from_slice(b"\r\n");
    let mut padded = request.into_bytes();
    padded.resize(UI_MAX_REQUEST_BYTES, b' ');
    padded.push(b'\n');
    input.extend_from_slice(&padded);
    let mut output = Vec::new();
    serve_stdio(&mut server, std::io::Cursor::new(input), &mut output);
    let replies = replies(&output);
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert!(replies.iter().all(|reply| reply["id"] == 8 && reply["error"].is_null()), "{replies:?}");
}