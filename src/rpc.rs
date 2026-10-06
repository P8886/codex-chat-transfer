use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

pub fn detect_binary() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CODEX_TRANSFER_CODEX_EXE") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        bail!("CODEX_TRANSFER_CODEX_EXE 指向不存在的文件");
    }
    if let Some(root) = dirs::data_local_dir() {
        let cache = root.join("OpenAI/Codex/bin");
        let mut found: Vec<_> = std::fs::read_dir(cache)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("codex.exe"))
            .filter(|path| path.is_file())
            .collect();
        found.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
        if let Some(path) = found.pop() {
            return Ok(path);
        }
    }
    let filename = if cfg!(windows) { "codex.exe" } else { "codex" };
    if let Some(paths) = std::env::var_os("PATH") {
        for path in std::env::split_paths(&paths) {
            let path = path.join(filename);
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    bail!("找不到 Codex app-server；请在设置中指定 codex 可执行文件")
}

pub struct Rpc {
    child: Child,
    input: BufWriter<ChildStdin>,
    responses: mpsc::Receiver<Value>,
    next_id: u64,
}

impl Rpc {
    pub fn start(binary: &Path, home: &Path) -> Result<Self> {
        let mut command = Command::new(binary);
        command
            .arg("app-server")
            .env("CODEX_HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().context("无法启动 Codex app-server")?;
        let input = BufWriter::new(child.stdin.take().context("app-server stdin 不可用")?);
        let stdout = child.stdout.take().context("app-server stdout 不可用")?;
        let (send, responses) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(value) = serde_json::from_str(&line) {
                    if send.send(value).is_err() {
                        break;
                    }
                }
            }
        });
        let mut rpc = Self {
            child,
            input,
            responses,
            next_id: 0,
        };
        rpc.call("initialize",json!({"clientInfo":{"name":"codex_chat_transfer","version":"0.1.0"},"capabilities":{"experimentalApi":true}}))?;
        writeln!(rpc.input, "{}", json!({"method":"initialized"}))?;
        rpc.input.flush()?;
        Ok(rpc)
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        writeln!(
            self.input,
            "{}",
            json!({"id":id,"method":method,"params":params})
        )?;
        self.input.flush()?;
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let response = self
                .responses
                .recv_timeout(remaining)
                .with_context(|| format!("Codex {method} 超时或进程退出"))?;
            if response["id"].as_u64() != Some(id) {
                continue;
            }
            if response.get("error").is_some() {
                bail!(
                    "Codex {method} 失败：{}",
                    response["error"]["message"].as_str().unwrap_or("未知错误")
                );
            }
            return response
                .get("result")
                .cloned()
                .context("app-server 返回值缺失");
        }
    }
}

impl Drop for Rpc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
