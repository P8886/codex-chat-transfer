use anyhow::{bail, Context, Result};
use codex_chat_transfer::{
    bundle,
    importer::{self, ImportOptions},
    model, rpc, runtime,
};
use include_dir::{include_dir, Dir};
use serde_json::{json, Value};
use std::{
    io::Read,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::Duration,
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use uuid::Uuid;

static UI: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ui-dist");

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name, value).unwrap()
}

fn api(request: &mut Request, token: &str, origin: &str) -> Result<Value> {
    if request.method() == &Method::Get && request.url() == "/api/bootstrap" {
        let report = runtime::inspect();
        return Ok(
            json!({"home":model::default_home(),"codexBinary":rpc::detect_binary().ok(),"token":token,
            "runtimeClosed":!report.blocked,"runtimeError":if report.blocked {Some("请退出列表中的数据写入进程")}else{None},"blockingProcesses":report.blockers,"version":env!("CARGO_PKG_VERSION")}),
        );
    }
    if request.method() != &Method::Post {
        bail!("不支持的请求方法");
    }
    let supplied = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("X-Transfer-Token"))
        .map(|h| h.value.as_str());
    if supplied != Some(token) {
        bail!("本地会话验证失败，请刷新页面");
    }
    if request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Origin"))
        .is_some_and(|h| h.value.as_str() != origin)
    {
        bail!("请求来源不匹配");
    }
    let mut bytes = Vec::new();
    request
        .as_reader()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        bail!("请求内容过大");
    }
    let body: Value = serde_json::from_slice(&bytes)?;
    let path = |key: &str| -> Result<PathBuf> {
        Ok(PathBuf::from(
            body[key].as_str().with_context(|| format!("缺少 {key}"))?,
        ))
    };
    match request.url() {
        "/api/catalog" => Ok(serde_json::to_value(model::scan(&path("home")?)?)?),
        "/api/dialog" => {
            let dialog = rfd::FileDialog::new().set_title("Codex Chat Transfer");
            let selected = match body["kind"].as_str() {
                Some("folder") => dialog.pick_folder(),
                Some("package") => dialog
                    .add_filter("Chat Transfer package", &["zip"])
                    .pick_file(),
                Some("binary") => dialog.pick_file(),
                Some("save") => dialog
                    .add_filter("Chat Transfer package", &["zip"])
                    .set_file_name("codex-chats.cct.zip")
                    .save_file(),
                _ => bail!("无效的文件选择类型"),
            };
            Ok(json!({"path":selected}))
        }
        "/api/export" => {
            let ids: Vec<String> = serde_json::from_value(body["threads"].clone())?;
            let output = path("output")?;
            let manifest = bundle::export(&path("home")?, &ids, &output)?;
            Ok(json!({"output":output,"manifest":manifest}))
        }
        "/api/preview" => {
            let package = bundle::load(&path("package")?)?;
            let catalog = model::scan(&path("home")?)?;
            let conflicts: Vec<_> = package
                .manifest
                .threads
                .iter()
                .filter(|t| catalog.threads.iter().any(|old| old.id == t.id))
                .map(|t| t.id.clone())
                .collect();
            Ok(json!({"manifest":package.manifest,"conflicts":conflicts}))
        }
        "/api/import" => {
            let options: ImportOptions = serde_json::from_value(body)?;
            Ok(serde_json::to_value(importer::import(&options)?)?)
        }
        _ => bail!("未知接口"),
    }
}

pub struct LocalServer {
    pub origin: String,
    server: Arc<Server>,
    busy: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LocalServer {
    pub fn start(port: u16) -> Result<Self> {
        let server = Server::http(("127.0.0.1", port))
            .map_err(|e| anyhow::anyhow!("无法启动本地界面：{e}"))?;
        let actual_port = server
            .server_addr()
            .to_ip()
            .context("服务器地址不可用")?
            .port();
        let origin = format!("http://127.0.0.1:{actual_port}");
        let token = Uuid::new_v4().to_string();
        let server = Arc::new(server);
        let busy = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (worker_server, worker_busy, worker_stop, worker_origin) =
            (server.clone(), busy.clone(), stop.clone(), origin.clone());
        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match worker_server.recv_timeout(Duration::from_millis(200)) {
                    Ok(Some(request)) => respond(request, &token, &worker_origin, &worker_busy),
                    Ok(None) => {}
                    Err(error) => {
                        eprintln!("本地服务错误：{error}");
                        break;
                    }
                }
            }
        });
        Ok(Self {
            origin,
            server,
            busy,
            stop,
            worker: Some(worker),
        })
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    pub fn wait(mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.server.unblock();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn serve(port: u16, no_open: bool) -> Result<()> {
    let server = LocalServer::start(port)?;
    let origin = &server.origin;
    println!("Codex Chat Transfer: {origin}");
    if !no_open {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("cmd")
                .args(["/c", "start", "", origin])
                .creation_flags(0x08000000)
                .spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&origin).spawn();
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let _ = std::process::Command::new("xdg-open").arg(&origin).spawn();
        }
    }
    server.wait();
    Ok(())
}

fn respond(mut request: Request, token: &str, origin: &str, busy: &AtomicBool) {
    let operation = request.method() == &Method::Post
        && matches!(request.url(), "/api/import" | "/api/export" | "/api/dialog");
    if operation {
        busy.store(true, Ordering::Release);
    }
    if request.url().starts_with("/api/") {
        let (status, value) = match api(&mut request, token, origin) {
            Ok(value) => (200, value),
            Err(error) => (400, json!({"error":format!("{error:#}")})),
        };
        let response = Response::from_data(value.to_string().into_bytes())
            .with_status_code(StatusCode(status))
            .with_header(header("Content-Type", "application/json; charset=utf-8"))
            .with_header(header("Cache-Control", "no-store"));
        let _ = request.respond(response);
    } else {
        let name = request
            .url()
            .split('?')
            .next()
            .unwrap_or("/")
            .trim_start_matches('/');
        let name = if name.is_empty() { "index.html" } else { name };
        if let Some(file) = UI.get_file(name) {
            let mime = mime_guess::from_path(name).first_or_octet_stream();
            let response = Response::from_data(file.contents().to_vec())
                .with_header(header("Content-Type", mime.as_ref()))
                .with_header(header("X-Content-Type-Options", "nosniff"));
            let _ = request.respond(response);
        } else {
            let _ = request
                .respond(Response::from_string("Not found").with_status_code(StatusCode(404)));
        }
    }
    if operation {
        busy.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    #[test]
    fn closing_local_server_releases_its_port() {
        let server = LocalServer::start(0).unwrap();
        let address = server.origin.strip_prefix("http://").unwrap().to_string();
        assert!(!server.is_busy());
        assert!(TcpStream::connect(&address).is_ok());
        drop(server);
        assert!(TcpStream::connect(&address).is_err());
    }
}
