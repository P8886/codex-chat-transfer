use crate::web::LocalServer;
use anyhow::{Context, Result};
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    platform::run_return::EventLoopExtRunReturn,
    window::WindowBuilder,
};
use wry::{WebContext, WebViewBuilder};

pub fn run() -> Result<()> {
    let directory = std::env::current_exe()?
        .parent()
        .context("应用目录不可用")?
        .join("data/webview2");
    std::fs::create_dir_all(&directory).context("应用目录不可写，请将便携包解压到可写的文件夹")?;
    let server = LocalServer::start(0)?;
    let mut event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Codex Chat Transfer")
        .with_inner_size(LogicalSize::new(1180.0, 800.0))
        .with_min_inner_size(LogicalSize::new(800.0, 600.0))
        .build(&event_loop)?;
    let mut context = WebContext::new(Some(directory));
    let origin = server.origin.clone();
    let allowed_origin = origin.clone();
    let webview = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(&origin)
        .with_navigation_handler(move |url| {
            url == allowed_origin || url.starts_with(&(allowed_origin.clone() + "/"))
        })
        .build(&window)
        .context("无法创建独立窗口，请确认系统已安装 Microsoft Edge WebView2 Runtime")?;
    event_loop.run_return(|event, _, flow| {
        *flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            if server.is_busy() {
                rfd::MessageDialog::new()
                    .set_title("操作尚未完成")
                    .set_description("正在导入、导出或选择文件。请等待当前操作完成后再关闭窗口。")
                    .set_level(rfd::MessageLevel::Warning)
                    .show();
            } else {
                *flow = ControlFlow::Exit;
            }
        }
    });
    drop(webview);
    drop(context);
    drop(window);
    drop(server);
    Ok(())
}
