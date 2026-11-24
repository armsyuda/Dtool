
#[path = "../../dtool-cli/src/harness.rs"]
#[allow(dead_code)]
mod actual_b15_profile_fixture;

mod codex_transports_acceptance {
    use super::actual_b15_profile_fixture::TestProfile;
    use dtool_core::Workspace;
    use dtool_mcp::{Config, Server, serve_stdio};
    use serde_json::{json, Value};
    use std::sync::Arc;

    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::time::{Duration, Instant};
    use dtool_mcp::http::{serve_http, HttpHandle, HttpOptions};

    struct OwnedHttp(Option<HttpHandle>);

    impl OwnedHttp {
        fn start(server: &mut Server) -> Self {
            Self(Some(serve_http(server, HttpOptions { addr: "127.0.0.1:0".parse().unwrap() }).unwrap()))
        }
        fn handle(&self) -> &HttpHandle { self.0.as_ref().unwrap() }
        fn request(&self, value: &Value) -> Value {
            let handle = self.handle();
            let mut stream = TcpStream::connect_timeout(&handle.addr, Duration::from_secs(2)).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
            let body = serde_json::to_vec(value).unwrap();
            write!(stream, "POST / HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", handle.token, body.len()).unwrap();
            stream.write_all(&body).unwrap();
            stream.shutdown(std::net::Shutdown::Write).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            assert!(response.starts_with("HTTP/1.1 200"));
            serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
        }
    }

    impl Drop for OwnedHttp {
        fn drop(&mut self) {
            let Some(handle) = self.0.take() else { return; };
            let addr = handle.addr;
            let worker = std::thread::spawn(move || drop(handle));
            let deadline = Instant::now() + Duration::from_secs(2);
            while !worker.is_finished() && Instant::now() < deadline {
                let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(20));
                std::thread::sleep(Duration::from_millis(1));
            }
            if worker.is_finished() { let _ = worker.join(); }
            else if !std::thread::panicking() { panic!("owned HTTP listener did not stop"); }
        }
    }

    fn server(profile: &TestProfile) -> Server {
        let mut workspace = Workspace::default();
        workspace.new_document("transport-fixture");
        Server::new(Config { allow_dirs: vec![profile.path().to_path_buf()], ..Config::default() }, workspace)
    }

    #[test]
    fn real_stdio_http_and_local_paths_share_one_workspace_and_reads_are_pure() {
        let profile = TestProfile::new();
        let mut server = server(&profile);
        let mut stdio_server = server.share();
        let workspace = server.workspace();
        assert!(Arc::ptr_eq(&workspace, &stdio_server.workspace()));
        let id = workspace.lock().unwrap().active().unwrap();
        let before = serde_json::to_value(&workspace.lock().unwrap().get(id).unwrap().session.document).unwrap();
        let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": "app.status", "params": {} } });
        let mut input = serde_json::to_vec(&request).unwrap(); input.push(b'\n');
        let mut output = Vec::new();
        serve_stdio(&mut stdio_server, std::io::Cursor::new(input), &mut output);
        let stdio: Value = serde_json::from_slice(&output).unwrap();
        let http = OwnedHttp::start(&mut server);
        let response = http.request(&request);
        assert!(stdio.get("error").is_none());
        assert!(response.get("error").is_none());
        assert!(stdio["result"].get("doc_version").is_some());
        assert!(response["result"].get("doc_version").is_some());
        let workspace = workspace.lock().unwrap();
        assert_eq!(workspace.active(), Some(id));
        assert_eq!(workspace.contexts().len(), 1);
        assert_eq!(serde_json::to_value(&workspace.get(id).unwrap().session.document).unwrap(), before);
    }

    fn pdf_fixture() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>",
            "<< /Length 16 >>\nstream\n10 20 30 40 re f\nendstream",
        ];
        let mut body = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(body.len());
            body.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
        }
        let xref = body.len();
        body.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in offsets { body.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes()); }
        body.extend_from_slice(format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        body
    }

    fn interchange_open_imports_real_objects(extension: &str, bytes: &[u8]) {
        let profile = TestProfile::new();
        let path = profile.path().join("early-interchange").with_extension(extension);
        std::fs::write(&path, bytes).unwrap();
        let mut server = server(&profile);
        let workspace = server.workspace();
        let count = workspace.lock().unwrap().contexts().len();
        let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 7, "method": "dtool.run", "params": { "name": "doc.open", "params": { "path": path } } }));
        assert!(reply.get("error").is_none(), "{reply}");
        let ws = workspace.lock().unwrap();
        assert_eq!(ws.contexts().len(), count + 1);
        let id = ws.active().unwrap();
        assert!(!ws.get(id).unwrap().session.document.objects.is_empty(), "{reply}");
    }

    #[test]
    fn early_pdf_lane_does_not_silently_create_an_empty_document() {
        interchange_open_imports_real_objects("pdf", &pdf_fixture());
    }

    #[test]
    fn early_svg_lane_does_not_claim_node_count_or_outlined_text_as_success() {
        interchange_open_imports_real_objects("svg", br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect x="10" y="20" width="30" height="40" fill="red"/></svg>"#);
    }

    #[test]
    fn early_pdf_compatible_ai_lane_is_not_renamed_format_success() {
        interchange_open_imports_real_objects("ai", &pdf_fixture());
    }
}
