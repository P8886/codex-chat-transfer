#![cfg_attr(windows, windows_subsystem = "windows")]

mod commands;
#[cfg(windows)]
mod desktop;
mod web;

fn main() {
    if let Err(error) = commands::run(true) {
        rfd::MessageDialog::new()
            .set_title("Codex Chat Transfer")
            .set_description(format!("启动失败：{error:#}"))
            .set_level(rfd::MessageLevel::Error)
            .show();
        std::process::exit(1);
    }
}
