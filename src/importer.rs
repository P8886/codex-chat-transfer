use crate::{
    bundle::{decode_records, digest, encode_records, load, Manifest},
    model::{desktop_path, path_key, read_json, scan, Project},
    rpc::Rpc,
};
use anyhow::{bail, ensure, Context, Result};
use chrono::Utc;
use fs2::FileExt;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOptions {
    pub home: PathBuf,
    pub package: PathBuf,
    pub project_id: Option<String>,
    pub target_dir: Option<PathBuf>,
    pub project_name: Option<String>,
    #[serde(default)]
    pub replace: bool,
    pub codex_binary: Option<PathBuf>,
    #[serde(default)]
    pub project_mappings: Option<Vec<ProjectMapping>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMapping {
    pub source_project_id: String,
    pub project_id: Option<String>,
    pub target_dir: Option<PathBuf>,
    pub project_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub status: String,
    pub imported: usize,
    pub project: Project,
    pub projects: Vec<Project>,
    pub backup: Option<String>,
    pub verified_message_count: usize,
    pub warnings: Vec<String>,
}

pub fn ensure_runtime_closed() -> Result<()> {
    crate::runtime::ensure_closed()
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("写入路径没有父目录")?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".chat-transfer-{}.tmp", Uuid::new_v4()));
    let result = (|| -> Result<()> {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[derive(Serialize, Deserialize)]
struct BackupEntry {
    path: String,
    file: Option<String>,
    database: bool,
}

struct Backup {
    directory: PathBuf,
    entries: Vec<BackupEntry>,
}

impl Backup {
    fn create(home: &Path, paths: &BTreeSet<PathBuf>) -> Result<Self> {
        let directory = home.join("chat-transfer-backups").join(format!(
            "{}-{}",
            Utc::now().format("%Y%m%d-%H%M%S"),
            Uuid::new_v4()
        ));
        fs::create_dir_all(&directory)?;
        let mut entries = Vec::new();
        for (index, path) in paths.iter().enumerate() {
            let database = path.extension().is_some_and(|e| e == "sqlite" || e == "db");
            let file = if path.exists() {
                let name = format!("snapshot-{index}");
                if database {
                    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                    db.backup("main", directory.join(&name), None)?;
                } else {
                    fs::copy(path, directory.join(&name))?;
                }
                Some(name)
            } else {
                None
            };
            entries.push(BackupEntry {
                path: path.to_string_lossy().into_owned(),
                file,
                database,
            });
        }
        atomic_write(
            &directory.join("backup.json"),
            &serde_json::to_vec_pretty(&entries)?,
        )?;
        Ok(Self { directory, entries })
    }

    fn restore(&self) -> Result<()> {
        for entry in &self.entries {
            let path = Path::new(&entry.path);
            if entry.database {
                for suffix in ["-wal", "-shm"] {
                    let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
                    if sidecar.exists() {
                        fs::remove_file(&sidecar)?;
                    }
                }
            }
            if let Some(file) = &entry.file {
                atomic_write(path, &fs::read(self.directory.join(file))?)?;
            } else if path.exists() {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }
}

fn object<'a>(state: &'a mut Value, key: &str) -> Result<&'a mut serde_json::Map<String, Value>> {
    if state.get(key).is_none() {
        state[key] = json!({});
    }
    state[key]
        .as_object_mut()
        .with_context(|| format!("Codex 配置字段 {key} 格式不支持"))
}

fn append_unique(state: &mut Value, key: &str, value: Value) -> Result<()> {
    if state.get(key).is_none() {
        state[key] = json!([]);
    }
    let list = state[key]
        .as_array_mut()
        .with_context(|| format!("Codex 配置字段 {key} 格式不支持"))?;
    if !list.contains(&value) {
        list.push(value);
    }
    Ok(())
}

pub fn desktop_membership(state: &mut Value, project: &Project, ids: &[String]) -> Result<()> {
    let now = Utc::now().timestamp_millis();
    object(state,"local-projects")?.entry(project.id.clone()).or_insert(json!({"id":project.id,"name":project.name,"rootPaths":project.roots,"createdAt":now,"updatedAt":now}));
    append_unique(state, "project-order", json!(project.id))?;
    for root in &project.roots {
        append_unique(state, "electron-saved-workspace-roots", json!(root))?;
    }
    for id in ids {
        object(state, "thread-project-assignments")?.insert(
            id.clone(),
            json!({"projectKind":"local","projectId":project.id}),
        );
        object(state, "thread-workspace-root-hints")?.insert(id.clone(), json!(project.roots[0]));
    }
    if let Some(value) = state.get_mut("projectless-thread-ids") {
        let values = value
            .as_array_mut()
            .context("projectless-thread-ids 格式不支持")?;
        values.retain(|v| !ids.iter().any(|id| v.as_str() == Some(id)));
    }
    if let Some(hosts) = state
        .get_mut("app-server-projects-migration-by-host")
        .and_then(Value::as_object_mut)
    {
        for status in hosts.values_mut() {
            if let Some(pending) = status
                .get_mut("pendingThreadAssignmentIds")
                .and_then(Value::as_array_mut)
            {
                pending.retain(|v| !ids.iter().any(|id| v.as_str() == Some(id)));
            }
        }
    }
    Ok(())
}

fn remap_settings(value: &mut Value, roots: &[String]) {
    if let Some(map) = value.as_object_mut() {
        if map.contains_key("cwd") {
            map.insert("cwd".into(), json!(roots[0]));
        }
        for key in [
            "runtime_workspace_roots",
            "writable_roots",
            "additional_dirs",
        ] {
            if map.contains_key(key) {
                map.insert(key.into(), json!(roots));
            }
        }
        for key in [
            "environments",
            "sandbox_policy",
            "permission_profile",
            "active_permission_profile",
        ] {
            if let Some(child) = map.get_mut(key) {
                if let Some(values) = child.as_array_mut() {
                    for value in values {
                        remap_settings(value, roots);
                    }
                } else {
                    remap_settings(child, roots);
                }
            }
        }
    }
}

fn remap_images(value: &mut Value, assets: &BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("local_image") {
                if let Some(path) = map.get("path").and_then(Value::as_str) {
                    if let Some(target) = assets.get(path) {
                        map.insert("path".into(), json!(target));
                    }
                }
            }
            for value in map.values_mut() {
                remap_images(value, assets);
            }
        }
        Value::Array(values) => {
            for value in values {
                remap_images(value, assets);
            }
        }
        _ => {}
    }
}

pub fn rewrite_rollout(
    bytes: &[u8],
    roots: &[String],
    assets: &BTreeMap<String, String>,
) -> Result<Vec<u8>> {
    let mut records = decode_records(bytes)?;
    for row in &mut records {
        match row["type"].as_str() {
            Some("session_meta" | "turn_context") => remap_settings(&mut row["payload"], roots),
            Some("event_msg") if row["payload"]["type"] == "thread_settings_applied" => {
                remap_settings(&mut row["payload"]["thread_settings"], roots)
            }
            _ => {}
        }
        remap_images(row, assets);
    }
    encode_records(&records)
}

fn select_project(options: &ImportOptions) -> Result<Project> {
    let catalog = scan(&options.home)?;
    if let Some(id) = &options.project_id {
        let project = catalog
            .projects
            .into_iter()
            .find(|p| p.id == *id)
            .context("目标项目不存在，请刷新项目列表")?;
        return validate_project_roots(project);
    }
    let root = options
        .target_dir
        .as_ref()
        .context("请选择 B 电脑上的目标项目或项目文件夹")?;
    ensure!(
        root.is_absolute() && root.is_dir(),
        "目标文件夹必须是已存在的绝对路径"
    );
    let root = desktop_path(&root.to_string_lossy());
    if let Some(project) = catalog
        .projects
        .into_iter()
        .find(|p| p.roots.iter().any(|r| path_key(r) == path_key(&root)))
    {
        return validate_project_roots(project);
    }
    Ok(Project {
        id: Uuid::new_v4().to_string(),
        name: options
            .project_name
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                options
                    .target_dir
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            }),
        roots: vec![root],
        native_id: None,
    })
}

pub(crate) fn validate_project_roots(mut project: Project) -> Result<Project> {
    let primary = project.roots.first().context("目标项目没有主文件夹")?;
    ensure!(
        Path::new(primary).is_absolute() && Path::new(primary).is_dir(),
        "目标项目“{}”的主文件夹不存在或不可访问：{}。请选择正确的目标项目或修正主文件夹路径。",
        project.name,
        primary
    );
    project
        .roots
        .retain(|root| Path::new(root).is_absolute() && Path::new(root).is_dir());
    Ok(project)
}

fn write_index(home: &Path, manifest: &Manifest) -> Result<()> {
    let path = home.join("session_index.jsonl");
    let mut entries: BTreeMap<String, Value> = BTreeMap::new();
    if path.exists() {
        for line in fs::read_to_string(&path)?.lines().filter(|l| !l.is_empty()) {
            let value: Value = serde_json::from_str(line)?;
            let id = value["id"]
                .as_str()
                .context("session_index 缺少 id")?
                .to_owned();
            entries.insert(id, value);
        }
    }
    for thread in &manifest.threads {
        entries.insert(
            thread.id.clone(),
            json!({"id":thread.id,"thread_name":thread.title,"updated_at":Utc::now().to_rfc3339()}),
        );
    }
    atomic_write(
        &path,
        &encode_records(&entries.into_values().collect::<Vec<_>>())?,
    )
}

fn clear_projection(home: &Path, ids: &[String]) -> Result<()> {
    let path = home.join("thread_history_1.sqlite");
    if !path.exists() {
        return Ok(());
    }
    let mut db = Connection::open(&path)?;
    let tables: BTreeSet<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let tx = db.transaction()?;
    for table in [
        "thread_turns",
        "thread_items",
        "thread_history_projection_state",
        "thread_realtime_items",
    ] {
        if tables.contains(table) {
            for id in ids {
                tx.execute(&format!("DELETE FROM {table} WHERE thread_id=?1"), [id])?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}

fn verify_disk(
    home: &Path,
    projects: &BTreeMap<String, Project>,
    targets: &BTreeMap<String, String>,
    manifest: &Manifest,
) -> Result<()> {
    let state = read_json(&home.join(".codex-global-state.json"))?;
    for thread in &manifest.threads {
        let project = &projects[&targets[&thread.id]];
        ensure!(
            state["thread-project-assignments"][&thread.id]["projectId"] == project.id,
            "桌面项目归属未保存"
        );
        ensure!(
            !state["projectless-thread-ids"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == &thread.id)),
            "会话仍被标记为无项目"
        );
    }
    let db = Connection::open_with_flags(
        home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    for thread in &manifest.threads {
        let project = &projects[&targets[&thread.id]];
        let (project_id, cwd, name): (Option<String>, String, Option<String>) = db.query_row(
            "SELECT project_id,cwd,name FROM threads WHERE id=?1",
            [&thread.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        ensure!(
            project_id == project.native_id
                && path_key(&cwd) == path_key(&project.roots[0])
                && name.as_deref() == Some(&thread.title),
            "原生会话归属、目录或标题校验失败：{}",
            thread.id
        );
    }
    Ok(())
}

fn select_targets(
    options: &ImportOptions,
    manifest: &Manifest,
) -> Result<(BTreeMap<String, Project>, BTreeMap<String, String>)> {
    let mut projects = BTreeMap::new();
    let mut targets = BTreeMap::new();
    if let Some(mappings) = &options.project_mappings {
        ensure!(
            mappings.len() == manifest.source_projects.len(),
            "项目映射未完整填写，不能保留全部分组"
        );
        let mut source_ids = BTreeSet::new();
        let mut roots = BTreeSet::new();
        let mut native_ids = BTreeSet::new();
        for mapping in mappings {
            let source = manifest
                .source_projects
                .iter()
                .find(|p| p.id == mapping.source_project_id)
                .context("映射引用不存在的源项目")?;
            ensure!(source_ids.insert(&source.id), "源项目映射重复");
            let mut request = options.clone();
            request.project_id = mapping.project_id.clone();
            request.target_dir = mapping.target_dir.clone();
            request.project_name = mapping
                .project_name
                .clone()
                .or_else(|| Some(source.name.clone()));
            let project = select_project(&request)?;
            if mapping.project_id.is_none() {
                ensure!(
                    request.project_name.as_deref() == Some(project.name.as_str()),
                    "所选文件夹已属于本机项目 {}；请直接选择该项目，或指定另一个文件夹",
                    project.name
                );
            }
            ensure!(
                !projects.contains_key(&project.id) && roots.insert(path_key(&project.roots[0])),
                "保留分组时，多个源项目不能映射到同一个项目或主文件夹"
            );
            if let Some(id) = &project.native_id {
                ensure!(
                    native_ids.insert(id.clone()),
                    "多个源项目不能映射到同一个原生项目"
                );
            }
            for thread in &manifest.threads {
                if thread.source_project_id.as_deref() == Some(&source.id) {
                    targets.insert(thread.id.clone(), project.id.clone());
                }
            }
            projects.insert(project.id.clone(), project);
        }
    } else {
        let project = select_project(options)?;
        for thread in &manifest.threads {
            targets.insert(thread.id.clone(), project.id.clone());
        }
        projects.insert(project.id.clone(), project);
    }
    ensure!(
        targets.len() == manifest.threads.len(),
        "会话项目映射不完整"
    );
    Ok((projects, targets))
}

pub fn import(options: &ImportOptions) -> Result<ImportReport> {
    ensure_runtime_closed()?;
    import_checked(options, ensure_runtime_closed)
}

pub(crate) fn import_checked(
    options: &ImportOptions,
    guard: fn() -> Result<()>,
) -> Result<ImportReport> {
    guard()?;
    ensure!(
        options.home.is_absolute() && options.home.is_dir(),
        "目标 Codex 数据目录必须是已存在的绝对路径"
    );
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(options.home.join(".chat-transfer.lock"))?;
    lock.try_lock_exclusive()
        .context("另一个 Chat Transfer 导入正在进行")?;
    let package = load(&options.package)?;
    let (mut projects, targets) = select_targets(options, &package.manifest)?;
    let catalog = scan(&options.home)?;
    let mut warnings = package.manifest.warnings.clone();
    for project in projects.values() {
        if let Some(original) = catalog.projects.iter().find(|p| p.id == project.id) {
            for root in original
                .roots
                .iter()
                .filter(|root| !project.roots.contains(root))
            {
                warnings.push(format!(
                    "目标项目“{}”的辅助文件夹不存在或不可访问，未加入本次运行目录：{}",
                    project.name, root
                ));
            }
        }
    }
    let ids: Vec<_> = package
        .manifest
        .threads
        .iter()
        .map(|t| t.id.clone())
        .collect();
    let journal = options
        .home
        .join("chat-transfer-imports")
        .join(format!("{}.json", package.manifest.package_id));
    if journal.exists() {
        let previous = read_json(&journal)?;
        let same_targets = previous["threadProjects"] == json!(targets)
            || (previous["projectId"].is_string()
                && targets
                    .values()
                    .all(|id| previous["projectId"].as_str() == Some(id)));
        if previous["sha256"] == package.sha256 && same_targets {
            verify_disk(&options.home, &projects, &targets, &package.manifest)?;
            let fingerprints = previous["fingerprints"]
                .as_array()
                .context("此前导入缺少正文指纹，不能确认完整性")?;
            for fingerprint in fingerprints {
                use std::io::Read;
                let path = PathBuf::from(fingerprint["path"].as_str().context("导入指纹路径无效")?);
                let length = fingerprint["bytes"].as_u64().context("导入指纹长度无效")?;
                let mut bytes = Vec::new();
                fs::File::open(path)?.take(length).read_to_end(&mut bytes)?;
                ensure!(
                    bytes.len() as u64 == length && digest(&bytes) == fingerprint["sha256"],
                    "已导入的正文或附件发生变化，不能确认此包完整；原数据未改动"
                );
            }
            return Ok(ImportReport {
                status: "alreadyImported".into(),
                imported: 0,
                project: projects.values().next().context("目标项目为空")?.clone(),
                projects: projects.values().cloned().collect(),
                backup: None,
                verified_message_count: 0,
                warnings,
            });
        }
    }
    let collisions: Vec<_> = catalog
        .threads
        .iter()
        .filter(|t| ids.contains(&t.id))
        .collect();
    ensure!(
        collisions.iter().all(|t| t.export_error.is_none()),
        "目标数据目录存在同 ID 文件冲突，无法确定要替换的会话；请先修复该记录"
    );
    ensure!(
        collisions.is_empty() || options.replace,
        "B 电脑已存在 {} 条同 ID 会话；请显式选择替换，原记录会先备份",
        collisions.len()
    );
    if !collisions.is_empty() {
        for thread in &catalog.threads {
            if ids.contains(&thread.id) {
                continue;
            }
            let meta = crate::model::first_metadata(&thread.path)?;
            if meta["payload"]["history_base"]["thread_id"]
                .as_str()
                .is_some_and(|id| ids.iter().any(|i| i == id))
            {
                bail!(
                    "{} 依赖即将被替换的历史；必须一起迁移或使用新的目标数据目录",
                    thread.title
                );
            }
        }
    }
    let binary = options
        .codex_binary
        .clone()
        .map(Ok)
        .unwrap_or_else(crate::rpc::detect_binary)?;
    let mut paths = BTreeSet::new();
    for file in [
        ".codex-global-state.json",
        ".codex-global-state.json.bak",
        "session_index.jsonl",
        "state_5.sqlite",
        "thread_history_1.sqlite",
        "sqlite/codex-dev.db",
    ] {
        paths.insert(options.home.join(file));
    }
    paths.insert(journal.clone());
    let mut rollouts = BTreeMap::new();
    for thread in &package.manifest.threads {
        let records = decode_records(&package.files[&thread.file])?;
        let timestamp = records[0]["payload"]["timestamp"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);
        let path = options
            .home
            .join("sessions")
            .join(timestamp.format("%Y/%m/%d").to_string())
            .join(format!(
                "rollout-{}-{}.jsonl",
                timestamp.format("%Y-%m-%dT%H-%M-%S"),
                thread.id
            ));
        paths.insert(path.clone());
        rollouts.insert(thread.id.clone(), path);
    }
    for thread in &collisions {
        paths.insert(thread.path.clone());
    }
    let mut asset_map = BTreeMap::new();
    for asset in &package.manifest.assets {
        let name = asset.file.strip_prefix("assets/").context("图片路径无效")?;
        let path = options
            .home
            .join("imported-assets")
            .join(&package.manifest.package_id)
            .join(name);
        paths.insert(path.clone());
        asset_map.insert(
            asset.source_path.clone(),
            desktop_path(&path.to_string_lossy()),
        );
    }
    let backup = Backup::create(&options.home, &paths)?;
    guard()?;
    let result = (|| -> Result<usize> {
        for thread in &collisions {
            if thread.path.exists() {
                fs::remove_file(&thread.path)?;
            }
        }
        for thread in &package.manifest.threads {
            let project = &projects[&targets[&thread.id]];
            let bytes = rewrite_rollout(&package.files[&thread.file], &project.roots, &asset_map)?;
            atomic_write(&rollouts[&thread.id], &bytes)?;
        }
        for asset in &package.manifest.assets {
            atomic_write(
                Path::new(&asset_map[&asset.source_path]),
                &package.files[&asset.file],
            )?;
        }
        clear_projection(&options.home, &ids)?;
        if options.home.join("state_5.sqlite").exists() {
            let db = Connection::open(options.home.join("state_5.sqlite"))?;
            for thread in &package.manifest.threads {
                let project = &projects[&targets[&thread.id]];
                db.execute(
                    "UPDATE threads SET rollout_path=?1,cwd=?2,archived=0 WHERE id=?3",
                    rusqlite::params![
                        rollouts[&thread.id].to_string_lossy(),
                        project.roots[0],
                        thread.id
                    ],
                )?;
            }
        }
        let mut rpc = Rpc::start(&binary, &options.home)?;
        for project in projects.values_mut() {
            if project.native_id.is_none() {
                let response=rpc.call("project/create",json!({"name":project.name,"roots":project.roots.iter().map(|p|json!({"path":p})).collect::<Vec<_>>(),"idempotencyKey":format!("chat-transfer-project-{}",project.id)}))?;
                project.native_id = Some(
                    response["project"]["id"]
                        .as_str()
                        .context("创建项目没有返回 ID")?
                        .to_owned(),
                );
            } else {
                rpc.call("project/read", json!({"projectId":project.native_id}))?;
            }
        }
        let mut verified = 0;
        for thread in &package.manifest.threads {
            let project = &projects[&targets[&thread.id]];
            rpc.call(
                "thread/read",
                json!({"threadId":thread.id,"includeTurns":false}),
            )?;
            rpc.call("thread/resume",json!({"threadId":thread.id,"cwd":project.roots[0],"runtimeWorkspaceRoots":project.roots,"excludeTurns":true}))?;
            rpc.call(
                "thread/metadata/update",
                json!({"threadId":thread.id,"projectId":project.native_id}),
            )?;
            rpc.call(
                "thread/name/set",
                json!({"threadId":thread.id,"name":thread.title}),
            )?;
            let mut cursor = Value::Null;
            let mut actual_ids = BTreeSet::new();
            loop {
                let page = rpc.call(
                    "thread/items/list",
                    json!({"threadId":thread.id,"limit":100,"cursor":cursor}),
                )?;
                for item in page["data"].as_array().context("历史分页返回格式不支持")? {
                    let item = item.get("item").unwrap_or(item);
                    if matches!(item["type"].as_str(), Some("userMessage" | "agentMessage")) {
                        if let Some(id) = item["id"].as_str() {
                            actual_ids.insert(id.to_owned());
                        }
                    }
                }
                cursor = page["nextCursor"].clone();
                if cursor.is_null() {
                    break;
                }
            }
            ensure!(
                thread.expected_message_ids.is_subset(&actual_ids),
                "聊天正文未完整进入 Codex 索引：{}",
                thread.title
            );
            if thread.expected_message_ids.is_empty() {
                ensure!(
                    !actual_ids.is_empty(),
                    "会话没有可读取的聊天正文：{}",
                    thread.title
                );
            }
            verified += actual_ids.len();
        }
        for thread in &package.manifest.threads {
            let project = &projects[&targets[&thread.id]];
            let read = rpc.call(
                "thread/read",
                json!({"threadId":thread.id,"includeTurns":false}),
            )?;
            ensure!(
                read["thread"]["projectId"].as_str() == project.native_id.as_deref(),
                "原生项目列表归属校验失败"
            );
        }
        drop(rpc);
        guard()?;
        let mut state = read_json(&options.home.join(".codex-global-state.json"))?;
        ensure!(state.is_object(), "Codex 全局配置格式不支持");
        for project in projects.values() {
            let group_ids: Vec<_> = targets
                .iter()
                .filter(|(_, id)| *id == &project.id)
                .map(|(id, _)| id.clone())
                .collect();
            desktop_membership(&mut state, project, &group_ids)?;
        }
        let key = format!("local:{}", desktop_path(&options.home.to_string_lossy()));
        object(
            &mut state,
            "app-server-project-id-by-legacy-project-id-by-host",
        )?
        .entry(key.clone())
        .or_insert(json!({}));
        for project in projects.values() {
            state["app-server-project-id-by-legacy-project-id-by-host"][&key][&project.id] =
                json!(project.native_id);
        }
        let bytes = serde_json::to_vec(&state)?;
        atomic_write(&options.home.join(".codex-global-state.json"), &bytes)?;
        atomic_write(&options.home.join(".codex-global-state.json.bak"), &bytes)?;
        write_index(&options.home, &package.manifest)?;
        let desktop_db = options.home.join("sqlite/codex-dev.db");
        if desktop_db.exists() {
            let db = Connection::open(&desktop_db)?;
            let table: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='local_thread_catalog')",
                [],
                |r| r.get(0),
            )?;
            if table {
                for thread in &package.manifest.threads {
                    let project = &projects[&targets[&thread.id]];
                    db.execute("UPDATE local_thread_catalog SET project_id=?1,cwd=?2,display_title=?3 WHERE host_id='local' AND thread_id=?4",rusqlite::params![project.id,project.roots[0],thread.title,thread.id])?;
                }
            }
        }
        verify_disk(&options.home, &projects, &targets, &package.manifest)?;
        let mut fingerprints = Vec::new();
        for path in rollouts.values().chain(
            asset_map
                .values()
                .map(PathBuf::from)
                .collect::<Vec<_>>()
                .iter(),
        ) {
            let bytes = fs::read(path)?;
            fingerprints.push(json!({"path":path,"bytes":bytes.len(),"sha256":digest(&bytes)}));
        }
        atomic_write(
            &journal,
            &serde_json::to_vec_pretty(
                &json!({"sha256":package.sha256,"threadProjects":targets,"importedAt":Utc::now().to_rfc3339(),"threadIds":ids,"verifiedMessageCount":verified,"fingerprints":fingerprints}),
            )?,
        )?;
        Ok(verified)
    })();
    match result {
        Ok(verified_message_count) => Ok(ImportReport {
            status: "verified".into(),
            imported: ids.len(),
            project: projects.values().next().context("目标项目为空")?.clone(),
            projects: projects.values().cloned().collect(),
            backup: Some(backup.directory.to_string_lossy().into_owned()),
            verified_message_count,
            warnings,
        }),
        Err(error) => {
            if let Err(runtime) = guard() {
                bail!("导入中途检测到 Codex 启动，已停止且没有报告成功：{runtime:#}；请退出 Codex 后使用 restore 恢复备份 {}；原错误：{error:#}",backup.directory.display());
            }
            if let Err(restore) = backup.restore() {
                bail!(
                    "导入失败：{error:#}；自动恢复也失败：{restore:#}；备份：{}",
                    backup.directory.display()
                );
            }
            bail!(
                "导入失败，已恢复原数据：{error:#}；备份：{}",
                backup.directory.display()
            )
        }
    }
}

pub fn restore_backup(home: &Path, directory: &Path) -> Result<()> {
    ensure_runtime_closed()?;
    restore_backup_checked(home, directory)
}

pub(crate) fn restore_backup_checked(home: &Path, directory: &Path) -> Result<()> {
    use std::path::Component;
    let home = home.canonicalize()?;
    let directory = directory.canonicalize()?;
    ensure!(
        directory
            .parent()
            .is_some_and(|p| p == home.join("chat-transfer-backups")),
        "备份必须来自所选 Codex 数据目录"
    );
    let entries: Vec<BackupEntry> =
        serde_json::from_slice(&fs::read(directory.join("backup.json"))?)?;
    for entry in &entries {
        let path = Path::new(&entry.path);
        ensure!(
            path.is_absolute() && !path.components().any(|p| p == Component::ParentDir),
            "备份目标路径无效"
        );
        ensure!(
            path_key(&entry.path).starts_with(&(path_key(&home.to_string_lossy()) + "/")),
            "备份路径超出 Codex 数据目录"
        );
        if let Some(file) = &entry.file {
            ensure!(
                file.starts_with("snapshot-") && file[9..].chars().all(|c| c.is_ascii_digit()),
                "备份文件名无效"
            );
            ensure!(directory.join(file).is_file(), "备份快照缺失");
        }
        for ancestor in path.ancestors().skip(1).filter(|p| p.exists()) {
            ensure!(
                ancestor.canonicalize()?.starts_with(&home)
                    || home.starts_with(ancestor.canonicalize()?),
                "备份目标经过外部链接"
            );
        }
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.join(".chat-transfer.lock"))?;
    lock.try_lock_exclusive()
        .context("另一个导入或恢复正在运行")?;
    Backup { directory, entries }.restore()
}
