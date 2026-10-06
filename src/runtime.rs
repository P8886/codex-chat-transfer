use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProcess {
    pub pid: u32,
    pub name: String,
    pub source: String,
    pub reason: String,
    pub executable: Option<String>,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReport {
    pub blocked: bool,
    pub blockers: Vec<RuntimeProcess>,
}

fn classify(
    name: &str,
    path: Option<&str>,
    arguments: &[String],
    parent: Option<&str>,
) -> Option<(String, String)> {
    let name = name.to_ascii_lowercase();
    let path = path.unwrap_or("").to_ascii_lowercase().replace('\\', "/");
    let args: Vec<_> = arguments.iter().map(|s| s.to_ascii_lowercase()).collect();
    if matches!(name.as_str(), "chatgpt.exe" | "chatgpt") {
        // Electron children cannot persist desktop state once their main process is gone.
        if args.iter().any(|a| a.starts_with("--type=")) {
            return None;
        }
        if !path.is_empty()
            && !path.contains("openai.codex")
            && !path.contains("/codex/")
            && !path.contains("/codex.app/")
        {
            return None;
        }
        return Some((
            "Codex 桌面主进程".into(),
            if path.is_empty() {
                "无法读取主进程路径，不能确认数据目录已释放".into()
            } else {
                "桌面主进程尚未退出，可能覆盖项目配置".into()
            },
        ));
    }
    if !matches!(name.as_str(), "codex.exe" | "codex") {
        return None;
    }
    let args: Vec<_> = args
        .iter()
        .skip(1)
        .take_while(|a| a.as_str() != "--")
        .map(String::as_str)
        .collect();
    if args
        .iter()
        .any(|a| matches!(*a, "--version" | "-v" | "--help" | "-h"))
    {
        return None;
    }
    let mut position = 0;
    while position < args.len() {
        if matches!(
            args[position],
            "-c" | "--config"
                | "--enable"
                | "--disable"
                | "--cd"
                | "-m"
                | "--model"
                | "--profile"
                | "-p"
        ) {
            position += 2;
        } else if args[position].starts_with('-') {
            if args[position].contains('=')
                || matches!(
                    args[position],
                    "--oss"
                        | "--search"
                        | "--no-alt-screen"
                        | "--yolo"
                        | "--dangerously-bypass-approvals-and-sandbox"
                )
            {
                position += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    let command = args.get(position).copied();
    if matches!(command, Some("exec-server" | "completion")) {
        return None;
    }
    if command == Some("app-server")
        && args
            .get(position + 1)
            .is_some_and(|a| matches!(*a, "generate-json-schema" | "generate-ts" | "proxy"))
    {
        return None;
    }
    let parent = parent.unwrap_or("").to_ascii_lowercase();
    let extension = path.contains("/.vscode/")
        || path.contains("/.cursor/")
        || matches!(parent.as_str(), "code.exe" | "cursor.exe" | "windsurf.exe");
    if args.contains(&"app-server") {
        Some((
            if extension {
                "编辑器的 Codex 扩展后台"
            } else {
                "Codex app-server"
            }
            .into(),
            "会话服务仍在运行，可能写入聊天数据库".into(),
        ))
    } else if arguments.is_empty() {
        Some((
            "Codex CLI（信息不可读取）".into(),
            "无法读取命令行，不能确认该进程是否会写入聊天数据".into(),
        ))
    } else {
        Some((
            "Codex CLI 会话".into(),
            "CLI 会话或命令尚未退出，可能写入聊天数据".into(),
        ))
    }
}

pub fn inspect() -> RuntimeReport {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_exe(UpdateKind::Always),
    );
    let mut blockers = Vec::new();
    for process in system.processes().values() {
        let name = process.name().to_string_lossy().into_owned();
        let executable = process.exe().map(|p| p.to_string_lossy().into_owned());
        let parent = process
            .parent()
            .and_then(|pid| system.process(pid))
            .map(|p| p.name().to_string_lossy().into_owned());
        let arguments: Vec<_> = process
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        if let Some((source, reason)) =
            classify(&name, executable.as_deref(), &arguments, parent.as_deref())
        {
            blockers.push(RuntimeProcess {
                pid: process.pid().as_u32(),
                name,
                source,
                reason,
                executable,
                parent,
            });
        }
    }
    blockers.sort_by_key(|p| p.pid);
    RuntimeReport {
        blocked: !blockers.is_empty(),
        blockers,
    }
}

pub fn ensure_closed() -> Result<()> {
    let report = inspect();
    ensure!(!report.blocked,"检测到可修改 Codex 数据的进程：{}。请查看 CCT 的进程列表并退出对应程序；本工具不会关闭或重启它们。",
        report.blockers.iter().map(|p|format!("{}（{}，PID {}）",p.source,p.name,p.pid)).collect::<Vec<_>>().join("；"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn remote_and_read_only_helpers_do_not_block_import() {
        for command in [
            args(&[
                "codex.exe",
                "exec-server",
                "--remote",
                "https://example.test",
            ]),
            args(&["codex.exe", "--version"]),
            args(&["codex.exe", "app-server", "generate-json-schema"]),
            args(&["codex.exe", "app-server", "proxy"]),
        ] {
            assert!(classify("codex.exe", Some("C:/Codex/codex.exe"), &command, None).is_none());
        }
    }
    #[test]
    fn orphan_electron_children_are_not_desktop_writers() {
        for role in ["--type=renderer", "--type=gpu-process", "--type=utility"] {
            assert!(classify("ChatGPT.exe", None, &args(&["ChatGPT.exe", role]), None).is_none());
        }
        assert!(classify(
            "ChatGPT.exe",
            Some("C:/WindowsApps/OpenAI.Codex/app/ChatGPT.exe"),
            &args(&["ChatGPT.exe"]),
            None
        )
        .is_some());
    }
    #[test]
    fn editor_app_servers_remain_blocked_with_a_clear_source() {
        let result = classify(
            "codex.exe",
            Some("C:/Users/test/.vscode/extensions/openai.chatgpt/bin/codex.exe"),
            &args(&[
                "codex.exe",
                "-c",
                "features.code_mode_host=true",
                "app-server",
            ]),
            Some("Code.exe"),
        )
        .unwrap();
        assert_eq!(result.0, "编辑器的 Codex 扩展后台");
    }
    #[test]
    fn unknown_cli_commands_are_not_silently_allowed() {
        assert!(classify("codex.exe", None, &[], None).is_some());
        assert!(classify(
            "codex.exe",
            None,
            &args(&["codex.exe", "exec", "--", "--help"]),
            None
        )
        .is_some());
        assert!(classify("codex-chat-transfer.exe", None, &[], None).is_none());
        assert!(classify("codex-windows-sandbox-service.exe", None, &[], None).is_none());
        assert!(classify(
            "codex.exe",
            None,
            &args(&["codex.exe", "exec", "exec-server"]),
            None
        )
        .is_some());
        assert!(classify(
            "codex.exe",
            None,
            &args(&["codex.exe", "exec", "generate-json-schema"]),
            None
        )
        .is_some());
    }
}
