#![cfg_attr(windows, windows_subsystem = "windows")]

use anyhow::{Context, Result};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path, process::Command};

static APPLICATION: &[u8] = include_bytes!(env!("CCT_GUI_PAYLOAD"));
static LOADER: &[u8] = include_bytes!(env!("CCT_LOADER_PAYLOAD"));

fn extract_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.is_file() && fs::read(path).is_ok_and(|existing| existing == bytes) {
        return Ok(());
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = fs::File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path).with_context(|| format!("无法释放内置组件：{}", path.display()))
}

fn run() -> Result<i32> {
    let mut hash = Sha256::new();
    hash.update(APPLICATION);
    hash.update(LOADER);
    let fingerprint = format!("{:x}", hash.finalize());
    let directory = dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("CodexChatTransfer/single-file")
        .join(&fingerprint[..32]);
    fs::create_dir_all(&directory)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("extract.lock"))?;
    lock.lock_exclusive().context("无法锁定组件缓存")?;
    let application = directory.join("codex-chat-transfer.exe");
    extract_file(&application, APPLICATION)?;
    extract_file(&directory.join("WebView2Loader.dll"), LOADER)?;
    FileExt::unlock(&lock)?;
    drop(lock);
    let status = Command::new(&application)
        .current_dir(&directory)
        .status()
        .context("无法启动内置独立窗口")?;
    Ok(status.code().unwrap_or(1))
}

fn main() {
    let code = match run() {
        Ok(code) => code,
        Err(error) => {
            #[cfg(windows)]
            {
                let message: Vec<u16> = format!("启动失败：{error:#}")
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let title: Vec<u16> = "Codex Chat Transfer"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                        std::ptr::null_mut(),
                        message.as_ptr(),
                        title.as_ptr(),
                        0x10,
                    );
                }
            }
            #[cfg(not(windows))]
            eprintln!("{error:#}");
            1
        }
    };
    std::process::exit(code);
}
