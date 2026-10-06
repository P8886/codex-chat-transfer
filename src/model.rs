use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub roots: Vec<String>,
    pub native_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub project_id: Option<String>,
    pub archived: bool,
    pub updated_at: i64,
    #[serde(default)]
    pub export_error: Option<String>,
    #[serde(skip)]
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub home: String,
    pub projects: Vec<Project>,
    pub threads: Vec<Thread>,
    pub warnings: Vec<String>,
}

pub fn default_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".codex"))
}

pub fn read_json(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    serde_json::from_slice(&fs::read(path)?).with_context(|| format!("无法读取 {}", path.display()))
}

pub fn desktop_path(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    if lower.starts_with(r"\\?\unc\") {
        format!(r"\\{}", &path[8..])
    } else if lower.starts_with(r"\\?\") {
        path[4..].to_string()
    } else {
        path.to_string()
    }
}

pub fn path_key(path: &str) -> String {
    let value = desktop_path(path).replace('\\', "/");
    let value = value.trim_end_matches('/');
    if value.as_bytes().get(1) == Some(&b':') || value.starts_with("//") {
        value.to_lowercase()
    } else {
        value.to_string()
    }
}

fn is_under(path: &str, root: &str) -> bool {
    let path = path_key(path);
    let root = path_key(root);
    path == root || path.starts_with(&(root + "/"))
}

pub fn first_metadata(path: &Path) -> Result<Value> {
    let file = fs::File::open(path)?;
    let first = BufReader::new(file)
        .lines()
        .next()
        .context("会话文件为空")??;
    let row: Value = serde_json::from_str(&first)?;
    if row["type"] != "session_meta" {
        bail!("{} 缺少 session_meta", path.display());
    }
    Ok(row)
}

pub fn scan(home: &Path) -> Result<Catalog> {
    if !home.is_dir() {
        bail!("Codex 数据目录不存在：{}", home.display());
    }
    let state = read_json(&home.join(".codex-global-state.json"))?;
    let mappings = state["app-server-project-id-by-legacy-project-id-by-host"].as_object();
    let mut projects = Vec::new();
    if let Some(local) = state["local-projects"].as_object() {
        for (id, value) in local {
            let roots: Vec<String> = value["rootPaths"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(desktop_path))
                .collect();
            if roots.is_empty() || id.starts_with("g-p-") {
                continue;
            }
            let native_id = mappings
                .into_iter()
                .flat_map(|m| m.values())
                .find_map(|m| m[id].as_str().map(str::to_owned));
            projects.push(Project {
                id: id.clone(),
                name: value["name"].as_str().unwrap_or(id).to_owned(),
                roots,
                native_id,
            });
        }
    }
    let mut titles = BTreeMap::new();
    if home.join("session_index.jsonl").exists() {
        for line in BufReader::new(fs::File::open(home.join("session_index.jsonl"))?).lines() {
            let value: Value = serde_json::from_str(&line?)?;
            if let (Some(id), Some(name)) = (value["id"].as_str(), value["thread_name"].as_str()) {
                titles.insert(id.to_owned(), name.to_owned());
            }
        }
    }
    let mut preferred_paths = BTreeMap::new();
    let mut db_titles = BTreeMap::new();
    let mut db_projects = BTreeMap::new();
    let mut db_times = BTreeMap::new();
    if home.join("state_5.sqlite").exists() {
        let db = Connection::open_with_flags(
            home.join("state_5.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let columns: BTreeSet<String> = db
            .prepare("PRAGMA table_info(threads)")?
            .query_map([], |r| r.get(1))?
            .collect::<rusqlite::Result<_>>()?;
        let name = if columns.contains("name") {
            "COALESCE(name,title)"
        } else {
            "title"
        };
        let project = if columns.contains("project_id") {
            "project_id"
        } else {
            "NULL"
        };
        let mut statement = db.prepare(&format!(
            "SELECT id,rollout_path,{name},{project},updated_at FROM threads"
        ))?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        for row in rows {
            let (id, path, name, project, updated) = row?;
            preferred_paths.insert(id.clone(), PathBuf::from(path));
            db_titles.insert(id.clone(), name);
            db_projects.insert(id.clone(), project);
            db_times.insert(id, updated);
        }
    }
    let mut candidates: BTreeMap<String, Vec<Thread>> = BTreeMap::new();
    let mut warnings = Vec::new();
    for folder in ["sessions", "archived_sessions"] {
        let root = home.join(folder);
        if !root.exists() {
            continue;
        }
        for item in WalkDir::new(&root).follow_links(false) {
            let item = item?;
            if !item.file_type().is_file()
                || item.path().extension().and_then(|v| v.to_str()) != Some("jsonl")
            {
                continue;
            }
            let metadata = match first_metadata(item.path()) {
                Ok(v) => v,
                Err(e) => {
                    warnings.push(e.to_string());
                    continue;
                }
            };
            let payload = &metadata["payload"];
            let Some(id) = payload["id"].as_str() else {
                warnings.push(format!("{} 缺少会话 ID", item.path().display()));
                continue;
            };
            if uuid::Uuid::parse_str(id).is_err() {
                warnings.push(format!("无效会话 ID：{id}"));
                continue;
            }
            let cwd = desktop_path(payload["cwd"].as_str().unwrap_or(""));
            let assignment = &state["thread-project-assignments"][id];
            let explicitly_projectless = state["projectless-thread-ids"]
                .as_array()
                .is_some_and(|v| v.iter().any(|v| v.as_str() == Some(id)));
            let project_id = assignment["projectId"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| {
                    if explicitly_projectless {
                        return None;
                    }
                    let db_project = db_projects.get(id).and_then(|v| v.as_deref());
                    projects
                        .iter()
                        .find(|p| p.native_id.as_deref() == db_project && db_project.is_some())
                        .or_else(|| {
                            projects
                                .iter()
                                .filter(|p| p.roots.iter().any(|r| is_under(&cwd, r)))
                                .max_by_key(|p| p.roots.iter().map(String::len).max().unwrap_or(0))
                        })
                        .map(|p| p.id.clone())
                });
            let title = titles
                .get(id)
                .or_else(|| db_titles.get(id))
                .cloned()
                .unwrap_or_else(|| id.to_owned());
            candidates.entry(id.to_owned()).or_default().push(Thread {
                id: id.to_owned(),
                title,
                cwd,
                project_id,
                archived: folder == "archived_sessions",
                updated_at: db_times.get(id).copied().unwrap_or(0),
                export_error: None,
                path: item.path().to_path_buf(),
            });
        }
    }
    let mut threads = Vec::new();
    for (id, mut choices) in candidates {
        let preferred = preferred_paths.get(&id).and_then(|path| {
            choices.iter().position(|t| {
                path_key(&t.path.to_string_lossy()) == path_key(&path.to_string_lossy())
            })
        });
        if let Some(index) = preferred {
            threads.push(choices.swap_remove(index));
        } else {
            let mut thread = choices.remove(0);
            if !choices.is_empty() {
                let error = format!("同一会话 ID 对应多个文件且无法确定当前文件：{id}");
                warnings.push(error.clone());
                thread.export_error = Some(error);
            }
            threads.push(thread);
        }
    }
    threads.sort_by_key(|t| std::cmp::Reverse(t.updated_at));
    projects.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Catalog {
        home: desktop_path(&home.to_string_lossy()),
        projects,
        threads,
        warnings,
    })
}
