
#[path = "../../dtool-cli/src/harness.rs"]
#[allow(dead_code)]
mod actual_shared_fixture;

mod codex_shared_workspace {

    use super::actual_shared_fixture::{HeadlessHarness, TestProfile, run_cli_args};
    use dtool_core::{Command, Scope};
    use dtool_core::workspace::{run_shared, SharedWorkspace, Workspace, CloseAnswer};
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

    fn fixture() -> (TestProfile, HeadlessHarness, SharedWorkspace, Server) {
        let profile = TestProfile::new();
        let mut harness = HeadlessHarness::new(&profile);
        let shared = harness.shared();
        let server = Server::with_workspace(Config { allow_dirs: vec![profile.path().to_path_buf()], ..Config::default() }, Arc::clone(&shared));
        server.gui_connection_handle().set_from_gui(true);
        (profile, harness, shared, server)
    }
    fn status_request() -> Value { json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": "app.status", "params": {} } }) }
    fn stdio(server: &mut Server, request: &Value) -> Value {
        let mut bytes = serde_json::to_vec(request).unwrap(); bytes.push(b'\n');
        let mut output = Vec::new();
        serve_stdio(server, std::io::Cursor::new(bytes), &mut output);
        serde_json::from_slice(&output).unwrap()
    }

    #[test]
    fn shared_handles_do_not_construct_independent_workspaces() {
        let (_profile, mut harness, shared, server) = fixture();
        assert!(Arc::ptr_eq(&shared, &server.workspace()));
        assert!(Arc::ptr_eq(&shared, &harness.shared()));
        let mut first = super::actual_shared_fixture::InProcessHarness::from_shared(Arc::clone(&shared));
        let mut second = super::actual_shared_fixture::InProcessHarness::from_shared(Arc::clone(&shared));
        assert_eq!(first.route(), "in_process");
        assert_eq!(second.kind(), "headless");
        assert!(Arc::ptr_eq(&shared, &first.shared()));
        assert!(Arc::ptr_eq(&shared, &second.shared()));
    }

    #[test]
    fn a_context_created_in_cli_runtime_is_visible_over_actual_stdio_http_and_local_api() {
        let (_profile, mut harness, shared, mut server) = fixture();
        let created = harness.with_workspace_mut(|workspace| workspace.new_document("shared-created-context")).unwrap();
        let (target, local) = run_shared(&shared, Scope::active(), Command::new("app.status"));
        assert_eq!(target, Some(created));
        assert_eq!(local.unwrap()["context_id"], created.0);
        let cli = run_cli_args(&shared, &["app.status".into(), "{}".into()]).unwrap();
        assert_eq!(cli["context_id"], created.0);
        let wire = stdio(&mut server, &status_request());
        assert_eq!(wire["result"]["context_id"], created.0);
        let http = OwnedHttp::start(&mut server);
        assert_eq!(http.request(&status_request())["result"]["context_id"], created.0);
        assert_eq!(shared.lock().unwrap().contexts().len(), 2);
    }

    #[test]
    fn closing_the_shared_context_is_visible_to_every_real_transport() {
        let (_profile, mut harness, shared, mut server) = fixture();
        let id = shared.lock().unwrap().active().unwrap();
        let http = OwnedHttp::start(&mut server);
        assert!(harness.with_workspace_mut(|workspace| workspace.close_with(id, CloseAnswer::No)).unwrap().unwrap());
        let (target, local) = run_shared(&shared, Scope::of(id), Command::new("app.status"));
        assert_eq!(target, Some(id));
        assert_eq!(local.unwrap_err().code, "CONTEXT_NOT_FOUND");
        assert_eq!(run_cli_args(&shared, &["app.status".into()]).unwrap_err().code, "NO_DOCUMENT");
        assert_eq!(stdio(&mut server, &status_request())["error"]["code"], "NO_DOCUMENT");
        assert_eq!(http.request(&status_request())["error"]["code"], "NO_DOCUMENT");
        assert!(shared.lock().unwrap().contexts().is_empty());
    }
}
