use crate::{
    importer::{atomic_write, Backup},
    model, runtime,
};
use anyhow::{bail, ensure, Context, Result};
use fs2::FileExt;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Deserialize)]
pub struct DeleteOptions {
    pub home: PathBuf,
    pub threads: Vec<String>,
    pub confirmed: bool,
}

#[derive(Debug, Serialize)]
pub struct DeleteReport {
    pub deleted: usize,
    pub backup: String,
}

pub fn delete(options: &DeleteOptions) -> Result<DeleteReport> {
    delete_checked(options, runtime::ensure_closed)
}

pub(crate) fn delete_checked(
    options: &DeleteOptions,
    guard: impl Fn() -> Result<()>,
) -> Result<DeleteReport> {
    ensure!(options.confirmed, "请先确认删除会话");
    ensure!(!options.threads.is_empty(), "未选择会话");
    guard()?;
    let home = options.home.canonicalize()?;
    let ids: BTreeSet<_> = options.threads.iter().cloned().collect();
    for id in &ids {
        uuid::Uuid::parse_str(id).context("无效的会话 ID")?;
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.join(".chat-transfer.lock"))?;
    lock.try_lock_exclusive()
        .context("另一个导入、删除或恢复正在运行")?;
    let catalog = model::scan(&home)?;
    ensure!(
        ids.iter()
            .all(|id| catalog.threads.iter().any(|t| &t.id == id)),
        "部分会话已不存在，请刷新列表"
    );
    let mut rollouts = BTreeSet::new();
    let mut found = BTreeSet::new();

    for folder in ["sessions", "archived_sessions"] {
        let root = home.join(folder);
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(&root).follow_links(false) {
            let entry = entry?;
            if !entry.file_type().is_file() || entry.path().extension().is_none_or(|e| e != "jsonl")
            {
                continue;
            }
            // Empty files contain no history references and must not block unrelated deletions.
            if entry.metadata()?.len() == 0 {
                continue;
            }
            let meta = model::first_metadata(entry.path())
                .with_context(|| format!("无法检查会话历史依赖：{}", entry.path().display()))?;
            let id = meta["payload"]["id"].as_str().context("会话缺少 ID")?;

            if !ids.contains(id)
                && meta["payload"]["history_base"]["thread_id"]
                    .as_str()
                    .is_some_and(|base| ids.contains(base))
            {
                let title = catalog
                    .threads
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| t.title.as_str())
                    .unwrap_or(id);
                bail!("会话 {title} 依赖待删除的前段历史，请一起选择或取消删除其前段会话");
            }
            if ids.contains(id) {
                found.insert(id.to_owned());
                let path = entry.path().canonicalize()?;
                ensure!(
                    path.starts_with(&home),
                    "会话文件指向数据目录外部，拒绝删除"
                );
                rollouts.insert(path);
            }
        }
    }

    ensure!(
        ids.is_subset(&found),
        "待删除会话文件发生变化或已为空，请刷新列表后重试"
    );
    let databases: Vec<_> = [
        "state_5.sqlite",
        "thread_history_1.sqlite",
        "sqlite/codex-dev.db",
    ]
    .into_iter()
    .map(|name| home.join(name))
    .filter(|p| p.exists())
    .collect();
    let mut paths = rollouts.clone();
    paths.extend(databases.iter().cloned());
    for name in [
        ".codex-global-state.json",
        ".codex-global-state.json.bak",
        "session_index.jsonl",
    ] {
        paths.insert(home.join(name));
    }
    for path in &paths {
        if path.exists() {
            ensure!(
                path.canonicalize()?.starts_with(&home),
                "删除目标指向数据目录外部"
            );
        }
        if let Some(parent) = path.parent() {
            ensure!(
                parent.canonicalize()?.starts_with(&home),
                "删除目标经过外部链接"
            );
        }
    }
    let backup = Backup::create(&home, &paths)?;
    guard()?;
    let result = (|| -> Result<()> {
        for path in &databases {
            guard()?;
            remove_database_rows(path, &ids)?;
        }
        for name in [".codex-global-state.json", ".codex-global-state.json.bak"] {
            let path = home.join(name);
            if !path.exists() {
                continue;
            }
            let mut state = model::read_json(&path)?;
            for key in [
                "thread-project-assignments",
                "thread-workspace-root-hints",
                "thread-titles",
                "thread-project-membership-host-ids",
                "thread-projectless-output-directories",
                "thread-writable-roots",
            ] {
                if let Some(map) = state[key].as_object_mut() {
                    map.retain(|id, _| !ids.contains(id));
                }
            }
            for key in [
                "projectless-thread-ids",
                "pending-thread-project-assignment-migration-thread-ids",
                "pinned-thread-ids",
            ] {
                if let Some(list) = state[key].as_array_mut() {
                    list.retain(|value| value.as_str().is_none_or(|id| !ids.contains(id)));
                }
            }
            if let Some(hosts) = state["app-server-projects-migration-by-host"].as_object_mut() {
                for status in hosts.values_mut() {
                    if let Some(list) = status["pendingThreadAssignmentIds"].as_array_mut() {
                        list.retain(|value| value.as_str().is_none_or(|id| !ids.contains(id)));
                    }
                }
            }
            if let Some(orders) = state["sidebar-project-thread-orders"].as_object_mut() {
                for (project_id, order) in orders {
                    if catalog
                        .projects
                        .iter()
                        .any(|project| &project.id == project_id)
                    {
                        if let Some(list) = order["threadIds"].as_array_mut() {
                            list.retain(|value| value.as_str().is_none_or(|id| !ids.contains(id)));
                        }
                    }
                }
            }
            guard()?;
            atomic_write(&path, &serde_json::to_vec(&state)?)?;
        }
        let index = home.join("session_index.jsonl");
        if index.exists() {
            let mut retained = Vec::new();
            for line in fs::read_to_string(&index)?
                .lines()
                .filter(|line| !line.is_empty())
            {
                let row: Value = serde_json::from_str(line)?;
                if row["id"].as_str().is_none_or(|id| !ids.contains(id)) {
                    retained.extend_from_slice(line.as_bytes());
                    retained.push(b'\n');
                }
            }
            guard()?;
            atomic_write(&index, &retained)?;
        }
        for path in &rollouts {
            guard()?;
            fs::remove_file(path)?;
        }
        guard()?;
        ensure!(
            model::scan(&home)?
                .threads
                .iter()
                .all(|t| !ids.contains(&t.id)),
            "删除后仍发现会话，操作未完成"
        );
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(runtime) = guard() {
            bail!(
                "删除中途检测到数据写入进程，已停止；退出后恢复备份 {}：{runtime:#}；{error:#}",
                backup.directory.display()
            );
        }
        backup.restore().with_context(|| {
            format!(
                "删除失败且回滚失败，请恢复备份 {}",
                backup.directory.display()
            )
        })?;
        bail!("删除失败，已恢复原数据：{error:#}");
    }
    Ok(DeleteReport {
        deleted: ids.len(),
        backup: backup.directory.to_string_lossy().into_owned(),
    })
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn remove_database_rows(path: &Path, ids: &BTreeSet<String>) -> Result<()> {
    let mut db = Connection::open(path)?;
    // Snapshot rollback covers all databases as one operation; remove child projections as well.
    db.pragma_update(None, "foreign_keys", false)?;
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let tx = db.transaction()?;
    for table in tables {
        let columns: BTreeSet<String> = tx
            .prepare(&format!("PRAGMA table_info({})", quoted(&table)))?
            .query_map([], |row| row.get(1))?
            .collect::<rusqlite::Result<_>>()?;
        let column = if columns.contains("thread_id") {
            Some("thread_id")
        } else if table == "threads" && columns.contains("id") {
            Some("id")
        } else {
            None
        };
        if let Some(column) = column {
            let host_filter = if columns.contains("host_id") {
                " AND host_id='local'"
            } else {
                ""
            };
            for id in ids {
                tx.execute(
                    &format!(
                        "DELETE FROM {} WHERE {}=?1{host_filter}",
                        quoted(&table),
                        quoted(column)
                    ),
                    [id],
                )?;
            }
        }
    }
    let invalid = tx.prepare("PRAGMA foreign_key_check")?.exists([])?;
    ensure!(!invalid, "删除后数据库仍有依赖引用，已取消删除");
    tx.commit()?;
    Ok(())
}
