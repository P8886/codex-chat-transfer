# Codex Chat Transfer

独立的 Codex 聊天迁移工具。Rust 处理导出、导入、备份与校验，TypeScript/React 提供本地界面。界面打包进可执行文件，不依赖 Cockpit Tools。

## 使用

单文件版只需复制 `codex-chat-transfer-single-v0.6.2.exe`，双击即打开独立窗口。EXE 内置应用与加载器，首次运行自动释放到当前用户的 `%LOCALAPPDATA%\CodexChatTransfer\single-file` 缓存，不需要手动携带 DLL、解压 ZIP 或安装应用。仍使用系统 WebView2 Runtime。关闭窗口后，启动器和内部服务一并退出。单文件版是图形入口，命令行功能使用下面的普通便携包。

桌面窗口中，聊天列表与左侧项目列表独立滚动，导出操作栏、搜索框及聊天表头保持固定；导入页仅滚动右侧内容区域。

右上角月亮/太阳按钮切换夜间或日间模式，覆盖列表、输入框、进程面板及预览/确认窗口，图片保持原色。首次启动默认跟随系统，手动选择会保存到 `%LOCALAPPDATA%\CodexChatTransfer\preferences.json`，重启、切换端口或升级便携版本后仍保留，不写入 Codex 聊天数据目录。

Windows 图形界面采用单实例：当前登录会话中再次启动单 EXE、便携 GUI 或 CLI 的 `desktop` 入口，只会唤起已有窗口，不再创建新窗口或内部服务。最小化窗口会恢复，已最大化的窗口保持大小；关闭或异常退出后可重新启动。命令行 `serve`、导出和其他 CLI 操作不受图形界面单实例限制。升级时请先关闭旧版本的所有窗口。

预览采用左右消息布局：用户消息在右侧浅色气泡内，助手消息在左侧；时间和附件随消息对齐，长文本自动换行。

历史解析独立索引原生 rollout 文件名的内部 ID，不再只依赖按聊天 ID 去重后的可见列表；同一聊天的旧历史文件也会参与查找。同一聊天 ID 引用另一个原生文件时按文件路径检测循环，不误判成自引用。空行保留其实际字节长度参与边界校验。找不到内部历史时显示解析失败，而不是断言原聊天丢失；字节边界错误会显示会话名称、实际历史文件路径以及预期/实际偏移。导出仍执行完整性校验，不会静默丢弃前段消息。

删除时的历史依赖检查跳过无关的零字节会话文件，不删除这些空文件。非空文件无法检查时仍会停止删除，并明确显示出错文件路径；真正的历史依赖保护保持不变。

界面导出只需选择文件夹，自动生成 `codex-chats.cct.zip`。同名文件存在时依次尝试 `codex-chats-1.cct.zip`、`codex-chats-2.cct.zip`，不会覆盖现有包；结果显示实际生成的完整路径。命令行的 `--out` 仍使用指定文件名。

聊天列表中的眼睛按钮可只读预览用户与助手正文；导出包校验后也可在导入前逐条预览，不需要关闭 Codex，不会导入或修改聊天。每页 50 条消息，支持前后翻页。正文按纯文本显示，图片附件显示缩略图并支持点击放大，工具执行记录不在正文预览中显示。单条超长消息显示前 131072 个字符并明确提示；导出数据不因此截断。

图片按需读取，支持 PNG、JPEG、GIF、WebP 及对应的内嵌 base64 图片，单张预览上限 20 MiB。本机读取会话中记录的原图；包内预览使用已校验的附件，不依赖 A 电脑原路径，导入后读取 B 电脑的新路径。缺失、无法解码或不支持的图片单独显示原因，不影响正文。远程图片链接不会自动联网加载；上述预览限制不改变导出包中的原始附件。

本机会话前段历史缺失时，预览仍显示当前文件内的现有内容，并提示历史不完整；完整导出仍严格检查前段依赖。导出路径、导出按钮和多选删除按钮位于聊天列表上方。列表末尾红色垃圾桶可删除单条会话；勾选多条后可批量删除，均需确认。删除要求退出 Codex 和扩展后台，会先备份会话文件、数据库与索引，再同步清理本机记录与项目归属，不删除项目代码或共享附件。若其他未选会话依赖待删历史，会拒绝删除；需要恢复时使用备份路径运行 `restore`。

解压便携包后，双击 `codex-chat-transfer.exe`，直接打开独立 Windows 窗口，不打开外部浏览器或命令行窗口。A 电脑选择项目和聊天，导出 `.cct.zip`；B 电脑选择导出包和本机目标项目，校验后导入。A/B 项目路径可以不同。

便携应用无须安装 Node.js、Rust 或 Cockpit Tools。需要系统已有 Microsoft Edge WebView2 Runtime；本机已验证该组件。缺少时会显示启动错误，不会自动安装。应用的 WebView 缓存保存在 EXE 旁的 `data/webview2`，请将便携包解压到可写文件夹。无需注册服务或计划任务。

包内的 `WebView2Loader.dll` 必须与 EXE 放在同一目录，不要只复制 EXE。

关闭独立窗口会退出内部本地服务，不保留后台程序。导入、导出或文件选择尚未完成时，关闭操作会被阻止，避免中断数据写入。原命令行功能仍然保留，显式使用 `serve` 才会运行浏览器模式。

图形入口为 `codex-chat-transfer.exe`；命令行使用 `codex-chat-transfer-cli.exe`，二者共用同一套迁移逻辑。

导入前须完全退出 Codex，包括后台的 app-server。工具会检查进程并拒绝运行中的导入，不会自动关闭或重启 Codex。导出时可以运行 Codex，但若源记录在打包期间变化，会终止导出。

右上角状态可展开或收起进程诊断，导出和导入页面共用同一列表。可点击“结束阻塞进程”，确认清单与风险后强制结束身份可验证的进程；结束前核对 PID、创建时间与程序路径。不会结束整个 VS Code / Cursor，不会自动执行导入或删除，扩展重启的后台仍会显示。路径或创建时间无法验证的进程需手动退出。请先保存工作、停止任务，强制结束可能中断数据写入。置灰的删除、导出和导入按钮可悬浮或键盘聚焦查看不可用原因。

VS Code / Cursor 的 Codex 扩展也可能启动 app-server，即使 Codex 桌面窗口已关闭仍会占用数据。新版列出 PID、来源和路径，忽略 `exec-server` 远程辅助进程、只读命令及孤立的 Electron 子进程；不会跳过真正的会话服务。命令行 `diagnose` 可查看相同的进程诊断，不输出完整命令行或令牌。

目标项目的主文件夹必须存在。失效的辅助文件夹不会再阻止导入，会从本次会话运行目录中排除并显示完整路径提醒；不会擅自修改原项目的源文件夹配置。主文件夹失效时会明确报出它的路径。

现有同 ID 会话默认拒绝覆盖。显式勾选替换后，原会话、相关数据库、全局配置与索引会先保存到目标数据目录的 `chat-transfer-backups`。失败时自动恢复。完全相同的包再次导入到同一项目会再次校验归属，不会重复写入。

## 迁移内容

新版导入默认保留 A 电脑的项目分组。导出包记录项目名称、源文件夹列表和每条聊天的所属项目；在 B 电脑上为每个分组选择现有项目或指定新项目的实际文件夹。新项目默认沿用 A 的项目名称，同名项目按不同 ID 区分，不会自动合并。映射缺失或多个源分组映射到同一项目/主目录时，保留分组模式会拒绝导入。整包共用一次备份与回滚。

旧格式包仍可导入，但只能按源目录推导分组，不能恢复包里没有记录的原项目名称。要完整保留原名称，请用新版在 A 电脑重新导出。“未分组聊天”作为单独映射项，由你指定目标项目。也可显式选择“合并到一个项目”。这些分组是聊天的项目归属，不包含项目代码文件的复制。

- 合并 `history_base` 引用的前段历史，重新编号，生成独立会话文件。缺失、截断、循环依赖或无效字节边界均拒绝导出。
- 保留用户/助手正文及工具记录；改写元数据、所有后续会话设置、运行目录与工作空间根目录，原始历史命令中的旧路径不改动。
- 包含能找到的 `local_image` 图片，并在 B 上重定位；已丢失的原图会显示警告，不能恢复不存在的内容。
- 创建或关联本机项目，保存原生 `projectId` 与桌面 `thread-project-assignments`，解除 `projectless-thread-ids`，更新标题索引。
- 用本机 Codex app-server 重建会话历史，逐页核对已保存的用户/助手消息 ID，并再次核对项目 ID、目录、标题和桌面归属后才返回成功。
- 包内每个会话和图片均校验 SHA-256。不导出账号凭证、API Key、Codex 配置或整个工程文件。

导入后的会话作为普通聊天显示；选中导出的归档记录也会恢复为普通聊天。源数据不会被删除。涉及同 ID 覆盖且被其他未选聊天引用的历史，拒绝替换以免破坏依赖。

## 范围

已验证 Codex 0.159.2 和 0.159.0-alpha.12.1 的 app-server 协议及对应项目配置结构。依赖本机安装的 Codex 可执行文件；可在设置中手动指定。未来结构变化导致校验失败时会回滚，不能把失败视为成功。

支持本地 Codex 对话，不迁移 ChatGPT 云端对话。聊天内容可能含敏感项目资料，导出包本身不加密。

## 命令行

```powershell
codex-chat-transfer-cli.exe list --home C:\Users\you\.codex
codex-chat-transfer-cli.exe diagnose
codex-chat-transfer-cli.exe export --home C:\Users\you\.codex --threads <会话ID> --out D:\transfer\chats.cct.zip
codex-chat-transfer-cli.exe inspect D:\transfer\chats.cct.zip
codex-chat-transfer-cli.exe import D:\transfer\chats.cct.zip --home C:\Users\you\.codex --project-id <本机项目ID>
codex-chat-transfer-cli.exe import D:\transfer\chats.cct.zip --target-dir D:\projects\todo --project-name todo
codex-chat-transfer-cli.exe import D:\transfer\chats.cct.zip --mappings D:\transfer\projects.json
codex-chat-transfer-cli.exe restore C:\Users\you\.codex\chat-transfer-backups\<备份目录> --home C:\Users\you\.codex
```

项目 ID 取自 `list` 的 `projects[].id`。`--replace` 允许覆盖同 ID 会话；`--codex` 指定 Codex 可执行文件。`serve --no-open --port 47831` 启动本地界面，不自动打开浏览器。仅绑定 `127.0.0.1`。

`--mappings` 读取逐项目的 JSON 数组，`sourceProjectId` 来自导出包 `sourceProjects[].id`。未传此选项的命令行导入保留原来的单项目行为。

```json
[
  { "sourceProjectId": "A-todo-id", "projectId": "B-existing-todo-id" },
  { "sourceProjectId": "A-notes-id", "targetDir": "D:\\projects\\notes", "projectName": "笔记项目" }
]
```

## 构建与验证

应用图标源文件为 `assets/app-icon.svg`，图标已内置在两种 EXE、窗口/任务栏和界面标题栏中。修改 SVG 后运行 `npm run icon`（需要本机 Chrome）重新生成多尺寸 ICO 与 PNG，再正常打包。Windows 构建使用资源编译器：GNU 工具链需 MinGW 的 `windres` 在 PATH，MSVC 需 Windows SDK。

Windows 打包需要 Node.js/npm 和 Rust Cargo（Windows x64 工具链；MSVC 需安装 Visual Studio C++ Build Tools）。双击根目录 `package.bat`，选择 `1` 单 EXE、`2` ZIP 便携版、`3` 两种都打包。产物保存在 `dist`，失败会显示错误，不继续打包。也可以在终端执行：

```bat
package.bat 1
package.bat 2
package.bat 3
```

Cargo 不在 PATH 时，可先设置 `CCT_CARGO` 为 `cargo.exe` 的完整路径。两种产物均使用系统 WebView2 Runtime。

```powershell
npm ci
npm run build
cargo test
cargo build --release
.\scripts\package-portable.ps1 -SkipBuild
```

Rust 原生 A/B 导入测试需要设置 `CODEX_TRANSFER_TEST_CODEX_EXE` 为 Codex 可执行文件的绝对路径。测试在临时目录中运行，不写入真实 `CODEX_HOME`。未设置时，该测试会注明跳过。

```powershell
$env:CODEX_TRANSFER_TEST_CODEX_EXE = 'C:\path\to\codex.exe'
cargo test native_a_to_b -- --nocapture
npm run test:ui
```

界面测试使用模拟接口验证流程，不把界面模拟测试当作真实 Codex 桌面验证。

## 协议

MIT，见 [LICENSE](LICENSE)。第三方依赖遵循各自协议。
