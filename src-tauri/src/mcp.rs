// Serves the Model Context Protocol over Streamable HTTP on the loopback
// interface. Messages are answered by the document window, which owns the
// documents and the editing UI. Each request waits for the window's reply,
// so responses are plain JSON and no event streams are offered.

use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, Url};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::MAIN;

const LOCAL_HOSTS: &[&str] = &["127.0.0.1", "localhost", "[::1]"];
const MAX_BODY_SIZE: u64 = 16 * 1024 * 1024;

#[derive(Deserialize, Clone, Copy, PartialEq)]
pub struct Config {
    enabled: bool,
    port: u16,
}

#[derive(Serialize, Clone, Default)]
pub struct Status {
    running: bool,
    url: Option<String>,
    error: Option<String>,
}

type Reply = mpsc::Sender<Result<Value, String>>;

#[derive(Default)]
struct ServerState {
    config: Option<Config>,
    server: Option<Arc<Server>>,
}

struct Inner {
    app: AppHandle,
    state: Mutex<ServerState>,
    status: Mutex<Status>,
    pending: Mutex<HashMap<u64, Reply>>,
    next_request_id: AtomicU64,
}

#[derive(Clone)]
pub struct Mcp(Arc<Inner>);

fn is_local(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(|host| LOCAL_HOSTS.contains(&host)))
        .unwrap_or(false)
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

fn respond(request: Request, status: u16, body: Option<Value>, headers: Vec<Header>) {
    let mut response = Response::from_data(body.as_ref().map(Value::to_string).unwrap_or_default())
        .with_status_code(status);
    if body.is_some() {
        response.add_header(header("Content-Type", "application/json"));
    }
    for header in headers {
        response.add_header(header);
    }
    // The client may have gone away in the meantime
    let _ = request.respond(response);
}

impl Mcp {
    pub fn new(app: AppHandle) -> Self {
        Mcp(Arc::new(Inner {
            app,
            state: Mutex::default(),
            status: Mutex::default(),
            pending: Mutex::default(),
            next_request_id: AtomicU64::new(1),
        }))
    }

    pub fn status(&self) -> Status {
        self.0.status.lock().unwrap().clone()
    }

    fn set_status(&self, status: Status) {
        *self.0.status.lock().unwrap() = status.clone();
        self.0.app.emit("mcp-status", [status]).unwrap();
    }

    pub fn configure(&self, config: Config) {
        let mut state = self.0.state.lock().unwrap();
        // A server that failed to start, like on a busy port, retries on reconfiguration
        if state.config == Some(config) && (state.server.is_some() || !config.enabled) {
            return;
        }
        state.config = Some(config);
        self.stop(&mut state);
        if config.enabled {
            self.start(&mut state, config.port);
        } else {
            self.set_status(Status::default());
        }
    }

    fn start(&self, state: &mut ServerState, port: u16) {
        let server = match Server::http(("127.0.0.1", port)) {
            Ok(server) => Arc::new(server),
            Err(error) => {
                let in_use = error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == ErrorKind::AddrInUse);
                return self.set_status(Status {
                    running: false,
                    url: None,
                    error: Some(if in_use {
                        format!("Port {port} is already in use")
                    } else {
                        error.to_string()
                    }),
                });
            }
        };
        state.server = Some(server.clone());
        self.set_status(Status {
            running: true,
            url: Some(format!("http://127.0.0.1:{port}/mcp")),
            error: None,
        });

        let mcp = self.clone();
        thread::spawn(move || {
            for request in server.incoming_requests() {
                let mcp = mcp.clone();
                thread::spawn(move || mcp.handle(request));
            }
        });
    }

    fn stop(&self, state: &mut ServerState) {
        let Some(server) = state.server.take() else {
            return;
        };
        server.unblock();
        self.reject_pending("The MCP server was stopped");
    }

    fn forward(&self, message: Value) -> Result<Value, String> {
        if self.0.app.get_webview_window(MAIN).is_none() {
            return Err("ProtoSolid has no open document window".to_string());
        }
        let id = self.0.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (reply, response) = mpsc::channel();
        self.0.pending.lock().unwrap().insert(id, reply);
        self.0
            .app
            .emit_to(MAIN, "mcp-request", (id, message))
            .map_err(|error| error.to_string())?;
        response
            .recv()
            .map_err(|_| "The document window did not answer".to_string())?
    }

    // The window answers every forwarded request, unless it reloads or closes
    pub fn receive(&self, id: u64, response: Value) {
        if let Some(reply) = self.0.pending.lock().unwrap().remove(&id) {
            let _ = reply.send(Ok(response));
        }
    }

    pub fn reject_pending(&self, reason: &str) {
        for (_, reply) in self.0.pending.lock().unwrap().drain() {
            let _ = reply.send(Err(reason.to_string()));
        }
    }

    fn handle(&self, mut request: Request) {
        let header_value = |request: &Request, name: &'static str| {
            request
                .headers()
                .iter()
                .find(|header| header.field.equiv(name))
                .map(|header| header.value.to_string())
        };

        // Browsers attach an origin. Refusing foreign ones and foreign host names
        // keeps web pages from reaching the server, including by DNS rebinding.
        if header_value(&request, "Origin").is_some_and(|origin| !is_local(&origin)) {
            return respond(
                request,
                403,
                Some(rpc_error(Value::Null, -32000, "Forbidden origin")),
                vec![],
            );
        }
        let host = header_value(&request, "Host").unwrap_or_default();
        if !is_local(&format!("http://{host}")) {
            return respond(
                request,
                403,
                Some(rpc_error(Value::Null, -32000, "Forbidden host")),
                vec![],
            );
        }
        if request.url().split('?').next() != Some("/mcp") {
            let error = rpc_error(Value::Null, -32000, "Not found, use the /mcp endpoint");
            return respond(request, 404, Some(error), vec![]);
        }
        if *request.method() != Method::Post {
            return respond(request, 405, None, vec![header("Allow", "POST")]);
        }

        let mut body = Vec::new();
        let read = request
            .as_reader()
            .take(MAX_BODY_SIZE + 1)
            .read_to_end(&mut body)
            .map_err(|error| error.to_string())
            .and_then(|size| {
                if size as u64 > MAX_BODY_SIZE {
                    Err("Request body is too large".to_string())
                } else {
                    Ok(())
                }
            });
        let message = match read
            .and_then(|_| serde_json::from_slice::<Value>(&body).map_err(|error| error.to_string()))
        {
            Ok(message) => message,
            Err(error) => {
                let error = rpc_error(Value::Null, -32700, &format!("Parse error: {error}"));
                return respond(request, 400, Some(error), vec![]);
            }
        };
        if !message.is_object() {
            let error = rpc_error(Value::Null, -32600, "Expected a single JSON-RPC message");
            return respond(request, 400, Some(error), vec![]);
        }

        // Notifications and responses need no answer
        let (Some(id), Some(method)) = (message.get("id").cloned(), message.get("method").cloned())
        else {
            return respond(request, 202, None, vec![]);
        };

        match self.forward(message) {
            Ok(response) => {
                let headers = if method == "initialize" {
                    vec![header("Mcp-Session-Id", &uuid::Uuid::new_v4().to_string())]
                } else {
                    vec![]
                };
                respond(request, 200, Some(response), headers)
            }
            Err(error) => respond(request, 503, Some(rpc_error(id, -32000, &error)), vec![]),
        }
    }
}
