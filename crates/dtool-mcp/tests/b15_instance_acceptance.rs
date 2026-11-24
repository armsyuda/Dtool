
mod codex_instance_acceptance {
    use dtool_core::Workspace;
    use dtool_mcp::{Config, Server};
    use serde_json::{json, Value};
    use std::net::TcpListener;
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

    fn server() -> Server {
        let mut workspace = Workspace::default();
        workspace.new_document("instance-fixture");
        Server::new(Config::default(), workspace)
    }

    #[test]
    fn occupied_local_port_is_reported_without_a_second_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut server = server();
        match serve_http(&mut server, HttpOptions { addr }) {
            Err(code) => assert_eq!(code, "PORT_IN_USE"),
            Ok(handle) => { drop(OwnedHttp(Some(handle))); panic!("occupied port was rebound"); }
        }
    }

    #[test]
    fn non_local_bind_is_refused_before_listening() {
        let mut server = server();
        match serve_http(&mut server, HttpOptions { addr: "0.0.0.0:0".parse().unwrap() }) {
            Err(code) => assert_eq!(code, "BIND_NOT_ALLOWED"),
            Ok(handle) => { drop(OwnedHttp(Some(handle))); panic!("non-local listener was accepted"); }
        }
    }

    #[test]
    fn instance_query_reports_bound_loopback_port_without_rebinding() {
        let mut server = server();
        let http = OwnedHttp::start(&mut server);
        let addr = http.handle().addr;
        for id in 1..=3 {
            let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": id, "method": "dtool.query", "params": { "name": "app.instance.query", "params": {} } }));
            assert!(reply.get("error").is_none());
            let result = reply["result"].to_string();
            assert!(result.contains("127.0.0.1"));
            assert!(result.contains(&addr.port().to_string()));
            assert!(TcpListener::bind(addr).is_err());
        }
    }

    #[test]
    fn five_local_installation_instances_do_not_share_workspaces() {
        let servers: Vec<_> = (0..5).map(|_| server()).collect();
        for (i, first) in servers.iter().enumerate() {
            for second in servers.iter().skip(i + 1) {
                assert!(!Arc::ptr_eq(&first.workspace(), &second.workspace()));
            }
        }
        let workspace = servers[0].workspace();
        workspace.lock().unwrap().new_document("only-first-installation");
        assert_eq!(workspace.lock().unwrap().contexts().len(), 2);
        for server in servers.iter().skip(1) { assert_eq!(server.workspace().lock().unwrap().contexts().len(), 1); }
    }
}
