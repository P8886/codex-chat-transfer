use crate::model::{first_metadata, path_key, scan, Thread};
use anyhow::{ensure, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackedThread {
    pub id: String,
    pub title: String,
    pub source_cwd: String,
    #[serde(default)]
    pub source_project_id: Option<String>,
    pub archived: bool,
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
    pub record_count: usize,
    pub expected_message_ids: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub source_path: String,
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceProject {
    pub id: String,
    pub name: String,
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub package_id: String,
    pub exported_at: String,
    pub source_version: String,
    #[serde(default)]
    pub source_projects: Vec<SourceProject>,
    pub threads: Vec<PackedThread>,
    pub assets: Vec<Asset>,
    pub warnings: Vec<String>,
}

pub struct Package {
    pub manifest: Manifest,
    pub files: BTreeMap<String, Vec<u8>>,
    pub sha256: String,
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn digest_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn read_records(path: &Path) -> Result<Vec<(u64, usize, Value)>> {
    ensure!(
        fs::metadata(path)?.len() <= MAX_FILE_BYTES,
        "会话文件超过 256 MiB：{}",
        path.display()
    );
    let mut rows = Vec::new();
    let mut offset = 0usize;
    for (index, line) in BufReader::new(fs::File::open(path)?)
        .split(b'\n')
        .enumerate()
    {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let row: Value = serde_json::from_slice(&line)
            .with_context(|| format!("{} 第 {} 行 JSON 无效", path.display(), index + 1))?;
        let ordinal = row["ordinal"].as_u64().unwrap_or(index as u64);
        rows.push((ordinal, offset, row));
        offset += line.len() + 1;
    }
    Ok(rows)
}

// Flatten inherited prefixes before migration, so byte offsets never point into rewritten files.
fn flatten(
    thread: &Thread,
    catalog: &BTreeMap<String, Thread>,
    cutoff: Option<u64>,
    visiting: &mut BTreeSet<String>,
    snapshots: &mut BTreeMap<PathBuf, String>,
) -> Result<Vec<Value>> {
    if let Some(error) = &thread.export_error {
        anyhow::bail!("{error}");
    }
    ensure!(
        visiting.insert(thread.id.clone()),
        "会话历史依赖成环：{}",
        thread.id
    );
    ensure!(
        fs::metadata(&thread.path)?.len() <= MAX_FILE_BYTES,
        "会话文件超过 256 MiB"
    );
    snapshots.insert(thread.path.clone(), digest_file(&thread.path)?);
    let records = read_records(&thread.path)?;
    ensure!(!records.is_empty(), "会话文件为空：{}", thread.id);
    let metadata = &records[0].2;
    let base = &metadata["payload"]["history_base"];
    let mut result = Vec::new();
    if let Some(base_id) = base["thread_id"].as_str() {
        let source = catalog.get(base_id).with_context(|| {
            format!(
                "会话 {} 缺少前段历史 {}；请从 A 电脑完整导出",
                thread.title, base_id
            )
        })?;
        let end = base["end_ordinal_exclusive"]
            .as_u64()
            .context("history_base 缺少截止序号")?;
        let source_rows = read_records(&source.path)?;
        ensure!(
            source_rows
                .last()
                .is_some_and(|(ordinal, _, _)| ordinal + 1 >= end),
            "前段历史被截断：{base_id}"
        );
        if let Some(expected_offset) = base["end_byte_offset"].as_u64() {
            let actual = source_rows
                .iter()
                .find(|(ordinal, _, _)| *ordinal == end)
                .map(|(_, offset, _)| *offset as u64)
                .unwrap_or(fs::metadata(&source.path)?.len());
            ensure!(
                actual == expected_offset,
                "前段历史的字节边界不匹配：{base_id}；不能保证历史完整"
            );
        }
        result.extend(flatten(source, catalog, Some(end), visiting, snapshots)?);
    } else {
        ensure!(
            records[0].0 == 0,
            "会话 {} 从序号 {} 开始，前段历史缺失",
            thread.title,
            records[0].0
        );
    }
    for (ordinal, _, row) in records {
        if cutoff.is_some_and(|limit| ordinal >= limit) {
            break;
        }
        if row["type"] != "session_meta" {
            result.push(row);
        }
    }
    visiting.remove(&thread.id);
    Ok(result)
}

pub fn expected_message_ids(records: &[Value]) -> BTreeSet<String> {
    records
        .iter()
        .filter_map(|row| {
            let p = &row["payload"];
            let item = &p["item"];
            if row["type"] == "event_msg"
                && p["type"] == "item_completed"
                && matches!(item["type"].as_str(), Some("UserMessage" | "AgentMessage"))
            {
                item["id"].as_str().map(str::to_owned)
            } else {
                None
            }
        })
        .collect()
}

pub fn encode_records(records: &[Value]) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for row in records {
        serde_json::to_writer(&mut bytes, row)?;
        bytes.push(b'\n');
    }
    Ok(bytes)
}

pub fn decode_records(bytes: &[u8]) -> Result<Vec<Value>> {
    bytes
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_slice(l).map_err(Into::into))
        .collect()
}

fn collect_image_paths(value: &Value, paths: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("local_image") {
                if let Some(path) = map.get("path").and_then(Value::as_str) {
                    paths.insert(path.to_owned());
                }
            }
            for value in map.values() {
                collect_image_paths(value, paths);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_image_paths(value, paths);
            }
        }
        _ => {}
    }
}

pub fn export(home: &Path, ids: &[String], output: &Path) -> Result<Manifest> {
    ensure!(!ids.is_empty(), "请选择要导出的聊天");
    ensure!(
        !output.exists(),
        "导出文件已存在，请使用新的文件名：{}",
        output.display()
    );
    let catalog = scan(home)?;
    let source_projects = catalog.projects.clone();
    let catalog: BTreeMap<_, _> = catalog
        .threads
        .into_iter()
        .map(|t| (t.id.clone(), t))
        .collect();
    let mut manifest = Manifest {
        format: "codex-chat-transfer".into(),
        version: 2,
        package_id: Uuid::new_v4().to_string(),
        exported_at: Utc::now().to_rfc3339(),
        source_version: "codex-jsonl-v1".into(),
        source_projects: Vec::new(),
        threads: Vec::new(),
        assets: Vec::new(),
        warnings: Vec::new(),
    };
    let mut files = BTreeMap::new();
    let mut snapshots = BTreeMap::new();
    let mut image_paths = BTreeSet::new();
    for id in ids.iter().collect::<BTreeSet<_>>() {
        let thread = catalog
            .get(id)
            .with_context(|| format!("找不到会话 {id}"))?;
        let source_id = thread
            .project_id
            .clone()
            .unwrap_or_else(|| "__unassigned__".into());
        if !manifest.source_projects.iter().any(|p| p.id == source_id) {
            let original = source_projects.iter().find(|p| p.id == source_id);
            manifest.source_projects.push(SourceProject {
                id: source_id.clone(),
                name: original.map(|p| p.name.clone()).unwrap_or_else(|| {
                    if thread.project_id.is_none() {
                        "未分组聊天".into()
                    } else {
                        folder_name(&thread.cwd)
                    }
                }),
                roots: original
                    .map(|p| p.roots.clone())
                    .unwrap_or_else(|| vec![thread.cwd.clone()]),
            });
        }
        let mut meta = first_metadata(&thread.path)?;
        meta["payload"]
            .as_object_mut()
            .context("session_meta 无效")?
            .remove("history_base");
        let mut records = vec![meta];
        records.extend(flatten(
            thread,
            &catalog,
            None,
            &mut BTreeSet::new(),
            &mut snapshots,
        )?);
        for (ordinal, row) in records.iter_mut().enumerate() {
            row["ordinal"] = Value::from(ordinal);
            if row["type"] == "event_msg" && row["payload"].get("thread_id").is_some() {
                row["payload"]["thread_id"] = Value::from(id.as_str());
            }
            collect_image_paths(row, &mut image_paths);
        }
        let bytes = encode_records(&records)?;
        let file = format!("rollouts/{id}.jsonl");
        manifest.threads.push(PackedThread {
            id: id.clone(),
            title: thread.title.clone(),
            source_cwd: thread.cwd.clone(),
            source_project_id: Some(source_id),
            archived: thread.archived,
            sha256: digest(&bytes),
            bytes: bytes.len() as u64,
            record_count: records.len(),
            expected_message_ids: expected_message_ids(&records),
            file: file.clone(),
        });
        files.insert(file, bytes);
    }
    for source_path in image_paths {
        let path = Path::new(&source_path);
        if !path.is_file() {
            manifest
                .warnings
                .push(format!("原始图片不存在：{source_path}"));
            continue;
        }
        let bytes = fs::read(path)?;
        ensure!(
            bytes.len() as u64 <= MAX_FILE_BYTES,
            "图片超过 256 MiB：{source_path}"
        );
        let sha256 = digest(&bytes);
        snapshots.insert(path.to_path_buf(), sha256.clone());
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .filter(|s| s.len() < 12 && s.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or("bin");
        let file = format!("assets/{sha256}.{extension}");
        manifest.assets.push(Asset {
            source_path,
            file: file.clone(),
            sha256,
            bytes: bytes.len() as u64,
        });
        files.insert(file, bytes);
    }
    ensure!(
        files.values().map(|b| b.len() as u64).sum::<u64>() <= MAX_PACKAGE_BYTES,
        "导出包超过 2 GiB"
    );
    for (path, expected) in &snapshots {
        ensure!(
            digest_file(path)? == *expected,
            "导出期间文件发生变化，请等待聊天结束后重新导出：{}",
            path.display()
        );
    }
    let parent = output.parent().context("导出路径无效")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".chat-transfer-{}.partial", Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut zip = ZipWriter::new(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?,
        );
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("manifest.json", options)?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        for (name, bytes) in &files {
            zip.start_file(name, options)?;
            zip.write_all(bytes)?;
        }
        zip.finish()?.sync_all()?;
        ensure!(!output.exists(), "导出文件已存在");
        fs::rename(&temporary, output)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(manifest)
}

fn folder_name(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .rfind(|p| !p.is_empty())
        .unwrap_or("未分组聊天")
        .to_owned()
}

pub fn normalize_groups(manifest: &mut Manifest) -> Result<()> {
    if manifest.version == 1 {
        manifest.source_projects.clear();
        for thread in &mut manifest.threads {
            let id = format!(
                "legacy-cwd:{}",
                digest(path_key(&thread.source_cwd).as_bytes())
            );
            thread.source_project_id = Some(id.clone());
            if !manifest.source_projects.iter().any(|p| p.id == id) {
                manifest.source_projects.push(SourceProject {
                    id,
                    name: folder_name(&thread.source_cwd),
                    roots: vec![thread.source_cwd.clone()],
                });
            }
        }
    }
    let mut ids = BTreeSet::new();
    for project in &manifest.source_projects {
        ensure!(
            !project.id.trim().is_empty() && ids.insert(&project.id),
            "导出包存在无效或重复的项目分组 ID"
        );
        ensure!(
            !project.name.trim().is_empty() && !project.roots.is_empty(),
            "项目分组名称或源目录缺失"
        );
        ensure!(
            manifest
                .threads
                .iter()
                .any(|t| t.source_project_id.as_deref() == Some(&project.id)),
            "导出包有空项目分组"
        );
    }
    ensure!(
        manifest.threads.iter().all(|t| t
            .source_project_id
            .as_ref()
            .is_some_and(|id| ids.contains(id))),
        "会话所属项目分组缺失"
    );
    Ok(())
}

pub fn load(path: &Path) -> Result<Package> {
    let mut zip = ZipArchive::new(fs::File::open(path)?)?;
    let mut manifest_bytes = Vec::new();
    zip.by_name("manifest.json")?
        .take(8 * 1024 * 1024)
        .read_to_end(&mut manifest_bytes)?;
    let mut manifest: Manifest =
        serde_json::from_slice(&manifest_bytes).context("不是有效的 Chat Transfer 导出包")?;
    ensure!(
        manifest.format == "codex-chat-transfer" && matches!(manifest.version, 1 | 2),
        "不支持此导出包格式版本"
    );
    Uuid::parse_str(&manifest.package_id)?;
    ensure!(
        !manifest.threads.is_empty() && manifest.threads.len() <= 10000,
        "导出包的会话数量无效"
    );
    normalize_groups(&mut manifest)?;
    let mut expected = BTreeMap::new();
    let mut thread_ids = BTreeSet::new();
    for thread in &manifest.threads {
        Uuid::parse_str(&thread.id)?;
        ensure!(thread_ids.insert(&thread.id), "导出包有重复会话 ID");
        ensure!(
            thread.file == format!("rollouts/{}.jsonl", thread.id),
            "导出包会话路径无效"
        );
        expected.insert(thread.file.clone(), (thread.sha256.clone(), thread.bytes));
    }
    for asset in &manifest.assets {
        ensure!(
            asset.file.starts_with("assets/")
                && asset.file.split('/').count() == 2
                && !asset.file.contains('\\')
                && !asset.file.contains(".."),
            "导出包图片路径无效"
        );
        if let Some(old) = expected.insert(asset.file.clone(), (asset.sha256.clone(), asset.bytes))
        {
            ensure!(
                old == (asset.sha256.clone(), asset.bytes),
                "图片校验声明冲突"
            );
        }
    }
    let mut names = BTreeSet::new();
    for index in 0..zip.len() {
        let file = zip.by_index(index)?;
        let name = file.name().to_owned();
        ensure!(names.insert(name.clone()), "ZIP 内存在重复文件名");
        ensure!(
            name == "manifest.json" || expected.contains_key(&name),
            "ZIP 含有未声明的文件：{name}"
        );
    }
    ensure!(zip.len() == expected.len() + 1, "导出包文件数量不匹配");
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    for (name, (hash, size)) in expected {
        let file = zip.by_name(&name)?;
        ensure!(
            file.size() == size && size <= MAX_FILE_BYTES,
            "导出包文件大小无效：{name}"
        );
        total = total.checked_add(size).context("导出包过大")?;
        ensure!(total <= MAX_PACKAGE_BYTES, "导出包解压后超过 2 GiB");
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 == size && digest(&bytes) == hash,
            "文件校验失败：{name}"
        );
        files.insert(name, bytes);
    }
    for thread in &manifest.threads {
        let records = decode_records(&files[&thread.file])?;
        ensure!(records.len()==thread.record_count && records.first().is_some_and(|r|r["type"]=="session_meta" && r["payload"]["id"]==thread.id),"会话元数据不匹配：{}",thread.id);
        ensure!(
            records[0]["payload"].get("history_base").is_none(),
            "导出包仍包含外部历史依赖"
        );
        ensure!(
            expected_message_ids(&records) == thread.expected_message_ids,
            "会话正文清单不匹配"
        );
        for (ordinal, row) in records.iter().enumerate() {
            ensure!(
                row["ordinal"].as_u64() == Some(ordinal as u64),
                "会话序号不连续"
            );
        }
    }
    let sha256 = digest_file(path)?;
    Ok(Package {
        manifest,
        files,
        sha256,
    })
}
