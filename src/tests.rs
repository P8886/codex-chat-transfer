use crate::importer::ProjectMapping;
use crate::{
    bundle,
    importer::{self, ImportOptions},
    model,
    rpc::Rpc,
};
use anyhow::Result;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use uuid::Uuid;

fn fixture(home: &Path, id: &str, base: Option<Value>, message: &str) -> PathBuf {
    fs::create_dir_all(home.join("sessions/2026/10/01")).unwrap();
    let turn = Uuid::new_v4().to_string();
    let user = Uuid::new_v4().to_string();
    let assistant = Uuid::new_v4().to_string();
    let mut meta = json!({"type":"session_meta","timestamp":"2026-10-01T01:00:00Z","payload":{
        "id":id,"timestamp":"2026-10-01T01:00:00Z","cwd":r"D:\Company\renewlet",
        "runtime_workspace_roots":[r"D:\Company\renewlet"],"originator":"Codex Desktop","cli_version":"0.159.2",
        "source":"vscode","thread_source":"user","model_provider":"openai","history_mode":"paginated"}});
    if let Some(base) = base {
        meta["payload"]["history_base"] = base;
    }
    let mut rows = vec![
        meta,
        json!({"type":"event_msg","timestamp":"2026-10-01T01:00:01Z","payload":{"type":"task_started","turn_id":turn,"root_turn_id":turn,"started_at":1790816401,"collaboration_mode_kind":"default","model_context_window":128000}}),
        json!({"type":"event_msg","timestamp":"2026-10-01T01:00:02Z","payload":{"type":"item_completed","thread_id":id,"turn_id":turn,"started_at_ms":1790816402000i64,"completed_at_ms":1790816402000i64,"item":{"type":"UserMessage","id":user,"content":[{"type":"text","text":message,"text_elements":[]}]}}}),
        json!({"type":"event_msg","timestamp":"2026-10-01T01:00:03Z","payload":{"type":"item_completed","thread_id":id,"turn_id":turn,"started_at_ms":1790816403000i64,"completed_at_ms":1790816403000i64,"item":{"type":"AgentMessage","id":assistant,"content":[{"type":"Text","text":"Fixture reply"}],"phase":"final_answer"}}}),
        json!({"type":"event_msg","timestamp":"2026-10-01T01:00:04Z","payload":{"type":"task_complete","turn_id":turn,"started_at":1790816401,"completed_at":1790816404,"duration_ms":3000,"last_agent_message":"Fixture reply"}}),
    ];
    let start = rows[0]["payload"]["history_base"]["end_ordinal_exclusive"]
        .as_u64()
        .unwrap_or(0);
    for (index, row) in rows.iter_mut().enumerate() {
        row["ordinal"] = json!(start + index as u64);
    }
    let path = home.join(format!(
        "sessions/2026/10/01/rollout-2026-10-01T01-00-00-{id}.jsonl"
    ));
    fs::write(&path, bundle::encode_records(&rows).unwrap()).unwrap();
    path
}

fn bare_home() -> TempDir {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join(".codex-global-state.json"), "{}").unwrap();
    temp
}
fn allow_isolated_fixture() -> Result<()> {
    Ok(())
}

#[test]
fn exporter_includes_inherited_messages_and_removes_external_history() {
    let source = bare_home();
    let work = TempDir::new().unwrap();
    let base = Uuid::new_v4().to_string();
    let selected = Uuid::new_v4().to_string();
    let parent = fixture(source.path(), &base, None, "Inherited user message");
    fixture(
        source.path(),
        &selected,
        Some(
            json!({"thread_id":base,"end_ordinal_exclusive":5,"end_byte_offset":fs::metadata(&parent).unwrap().len()}),
        ),
        "Current user message",
    );
    let source_bytes = fs::read(&parent).unwrap();
    let output = work.path().join("package.zip");
    let manifest = bundle::export(source.path(), std::slice::from_ref(&selected), &output).unwrap();
    assert_eq!(manifest.threads[0].expected_message_ids.len(), 4);
    let packed = bundle::load(&output).unwrap();
    let records = bundle::decode_records(&packed.files[&manifest.threads[0].file]).unwrap();
    assert!(records[0]["payload"].get("history_base").is_none());
    assert!(records
        .iter()
        .any(|r| r.to_string().contains("Inherited user message")));
    assert_eq!(fs::read(&parent).unwrap(), source_bytes);
}

#[test]
fn exporter_rejects_missing_or_cyclic_history_without_creating_package() {
    let source = bare_home();
    let work = TempDir::new().unwrap();
    let id = Uuid::new_v4().to_string();
    fixture(
        source.path(),
        &id,
        Some(json!({"thread_id":Uuid::new_v4().to_string(),"end_ordinal_exclusive":2})),
        "Current",
    );
    let output = work.path().join("package.zip");
    assert!(bundle::export(source.path(), std::slice::from_ref(&id), &output).is_err());
    assert!(!output.exists());
    fixture(
        source.path(),
        &id,
        Some(json!({"thread_id":id,"end_ordinal_exclusive":5})),
        "Current",
    );
    assert!(bundle::export(source.path(), &[id], &output).is_err());
    assert!(!output.exists());
}

#[test]
fn exporter_rejects_already_truncated_history() {
    let source = bare_home();
    let work = TempDir::new().unwrap();
    let id = Uuid::new_v4().to_string();
    let path = fixture(source.path(), &id, None, "Incomplete");
    let mut rows = bundle::decode_records(&fs::read(&path).unwrap()).unwrap();
    rows[0]["ordinal"] = json!(340);
    fs::write(&path, bundle::encode_records(&rows).unwrap()).unwrap();
    assert!(bundle::export(source.path(), &[id], &work.path().join("package.zip")).is_err());
}

#[test]
fn desktop_membership_clears_projectless_and_pending_assignment_flags() {
    let mut state = json!({"untouched":{"theme":"dark"},"projectless-thread-ids":["one","other"],
        "thread-project-assignments":{"other":{"projectId":"keep","projectKind":"local"}},
        "app-server-projects-migration-by-host":{"local:test":{"pendingThreadAssignmentIds":["one","other"]}}});
    let project = model::Project {
        id: "target".into(),
        name: "Todo".into(),
        roots: vec!["D:\\Personal\\todo".into()],
        native_id: Some("native".into()),
    };
    importer::desktop_membership(&mut state, &project, &["one".into()]).unwrap();
    assert_eq!(
        state["thread-project-assignments"]["one"]["projectId"],
        "target"
    );
    assert_eq!(state["projectless-thread-ids"], json!(["other"]));
    assert_eq!(
        state["app-server-projects-migration-by-host"]["local:test"]["pendingThreadAssignmentIds"],
        json!(["other"])
    );
    assert_eq!(state["untouched"]["theme"], "dark");
    assert_eq!(
        state["thread-project-assignments"]["other"]["projectId"],
        "keep"
    );
}

#[test]
fn remaps_every_runtime_context_but_preserves_historical_commands_and_text() {
    let rows = vec![
        json!({"type":"session_meta","payload":{"cwd":"D:\\A","runtime_workspace_roots":["D:\\A"]}}),
        json!({"type":"turn_context","payload":{"cwd":"D:\\A","environments":[{"cwd":"D:\\A"}],"sandbox_policy":{"writable_roots":["D:\\A"]}}}),
        json!({"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"cwd":"D:\\A","runtime_workspace_roots":["D:\\A"]}}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"CommandExecution","cwd":"D:\\A","command":["git","status"]}}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","content":[{"type":"text","text":"D:\\A"},{"type":"local_image","path":"D:\\A\\image.png"}]}}}),
    ];
    let roots = vec!["D:\\B\\todo".to_string()];
    let images = BTreeMap::from([("D:\\A\\image.png".into(), "D:\\B\\assets\\image.png".into())]);
    let result = bundle::decode_records(
        &importer::rewrite_rollout(&bundle::encode_records(&rows).unwrap(), &roots, &images)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result[0]["payload"]["cwd"], roots[0]);
    assert_eq!(result[1]["payload"]["environments"][0]["cwd"], roots[0]);
    assert_eq!(
        result[1]["payload"]["sandbox_policy"]["writable_roots"],
        json!(roots)
    );
    assert_eq!(result[2]["payload"]["thread_settings"]["cwd"], roots[0]);
    assert_eq!(result[3], rows[3]);
    assert_eq!(
        result[4]["payload"]["item"]["content"][0],
        rows[4]["payload"]["item"]["content"][0]
    );
    assert_eq!(
        result[4]["payload"]["item"]["content"][1]["path"],
        "D:\\B\\assets\\image.png"
    );
}

#[test]
fn checksum_tampering_is_rejected() {
    let source = bare_home();
    let work = TempDir::new().unwrap();
    let id = Uuid::new_v4().to_string();
    fixture(source.path(), &id, None, "Original");
    let output = work.path().join("package.zip");
    bundle::export(source.path(), &[id], &output).unwrap();
    let packed = bundle::load(&output).unwrap();
    let corrupt = work.path().join("corrupt.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&corrupt).unwrap());
    use std::io::Write;
    zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&serde_json::to_vec(&packed.manifest).unwrap())
        .unwrap();
    for (name, mut bytes) in packed.files {
        bytes.push(b' ');
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap();
    assert!(bundle::load(&corrupt).is_err());
}

#[test]
fn collision_is_reported_before_any_existing_file_is_modified() {
    let source = bare_home();
    let target = bare_home();
    let work = TempDir::new().unwrap();
    let id = Uuid::new_v4().to_string();
    fixture(source.path(), &id, None, "Source");
    let old = fixture(target.path(), &id, None, "Existing target");
    let before = fs::read(&old).unwrap();
    let package = work.path().join("package.zip");
    bundle::export(source.path(), &[id], &package).unwrap();
    let dir = work.path().join("todo");
    fs::create_dir(&dir).unwrap();
    let options = ImportOptions {
        home: target.path().into(),
        package,
        project_id: None,
        target_dir: Some(dir),
        project_name: Some("Todo".into()),
        replace: false,
        codex_binary: None,
        project_mappings: None,
    };
    let error = importer::import_checked(&options, allow_isolated_fixture).unwrap_err();
    assert!(error.to_string().contains("同 ID"));
    assert_eq!(fs::read(&old).unwrap(), before);
}

#[test]
fn native_a_to_b_import_is_readable_visible_in_project_and_idempotent() {
    let Ok(binary) = std::env::var("CODEX_TRANSFER_TEST_CODEX_EXE") else {
        eprintln!("Native test skipped: set CODEX_TRANSFER_TEST_CODEX_EXE");
        return;
    };
    let source = bare_home();
    let target = bare_home();
    let work = TempDir::new().unwrap();
    let base = Uuid::new_v4().to_string();
    let id = Uuid::new_v4().to_string();
    let parent = fixture(source.path(), &base, None, "A inherited user message");
    let original = fixture(
        source.path(),
        &id,
        Some(
            json!({"thread_id":base,"end_ordinal_exclusive":5,"end_byte_offset":fs::metadata(&parent).unwrap().len()}),
        ),
        "A current user message",
    );
    let original_bytes = fs::read(&original).unwrap();
    let package = work.path().join("package.zip");
    bundle::export(source.path(), std::slice::from_ref(&id), &package).unwrap();
    let destination = work.path().join("B-todo");
    fs::create_dir(&destination).unwrap();
    fs::write(
        target.path().join(".codex-global-state.json"),
        serde_json::to_vec(&json!({"projectless-thread-ids":[id],"unrelated":"keep"})).unwrap(),
    )
    .unwrap();
    let options = ImportOptions {
        home: target.path().into(),
        package: package.clone(),
        project_id: None,
        target_dir: Some(destination.clone()),
        project_name: Some("B Todo".into()),
        replace: false,
        codex_binary: Some(binary.clone().into()),
        project_mappings: None,
    };
    let report = importer::import_checked(&options, allow_isolated_fixture).unwrap();
    assert_eq!(report.status, "verified");
    assert_eq!(report.imported, 1);
    assert_eq!(report.verified_message_count, 4);
    assert_eq!(fs::read(&original).unwrap(), original_bytes);
    let global = model::read_json(&target.path().join(".codex-global-state.json")).unwrap();
    assert_eq!(global["unrelated"], "keep");
    assert_eq!(global["projectless-thread-ids"], json!([]));
    assert_eq!(
        global["thread-project-assignments"][&id]["projectId"],
        report.project.id
    );
    let mut server = Rpc::start(Path::new(&binary), target.path()).unwrap();
    let list=server.call("thread/list",json!({"projectId":report.project.native_id,"sourceKinds":[],"modelProviders":[],"limit":100})).unwrap();
    assert!(list["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"] == id));
    let resumed = server
        .call("thread/resume", json!({"threadId":id,"excludeTurns":true}))
        .unwrap();
    assert_eq!(
        model::path_key(resumed["cwd"].as_str().unwrap()),
        model::path_key(&destination.to_string_lossy())
    );
    drop(server);
    let again = importer::import_checked(&options, allow_isolated_fixture).unwrap();
    assert_eq!(again.status, "alreadyImported");
    let catalog = model::scan(target.path()).unwrap();
    assert_eq!(catalog.threads.len(), 1);
    assert_eq!(
        catalog.threads[0].project_id.as_deref(),
        Some(report.project.id.as_str())
    );
}

#[test]
fn failed_native_initialization_rolls_back_files_and_global_state() {
    let source = bare_home();
    let target = bare_home();
    let work = TempDir::new().unwrap();
    let id = Uuid::new_v4().to_string();
    fixture(source.path(), &id, None, "Rollback");
    let package = work.path().join("package.zip");
    bundle::export(source.path(), &[id], &package).unwrap();
    let destination = work.path().join("todo");
    fs::create_dir(&destination).unwrap();
    let global_before = fs::read(target.path().join(".codex-global-state.json")).unwrap();
    let options = ImportOptions {
        home: target.path().into(),
        package,
        project_id: None,
        target_dir: Some(destination),
        project_name: None,
        replace: false,
        codex_binary: Some(work.path().join("missing.exe")),
        project_mappings: None,
    };
    assert!(importer::import_checked(&options, allow_isolated_fixture).is_err());
    assert_eq!(
        fs::read(target.path().join(".codex-global-state.json")).unwrap(),
        global_before
    );
    assert!(model::scan(target.path()).unwrap().threads.is_empty());
    assert!(!target.path().join("state_5.sqlite").exists());
}

#[test]
fn path_normalization_handles_windows_verbatim_and_unc_paths() {
    assert_eq!(
        model::path_key(r"\\?\D:\Personal\Todo\"),
        model::path_key("d:/personal/todo")
    );
    assert_eq!(
        model::desktop_path(r"\\?\UNC\server\share\folder"),
        r"\\server\share\folder"
    );
    assert_ne!(
        model::path_key("/Projects/A"),
        model::path_key("/projects/a")
    );
}

#[test]
fn images_are_packed_with_checksums_and_missing_images_are_reported() {
    let source = bare_home();
    let work = TempDir::new().unwrap();
    let id = Uuid::new_v4().to_string();
    let image = work.path().join("sample.png");
    fs::write(&image, b"image fixture bytes").unwrap();
    let path = fixture(source.path(), &id, None, "Picture");
    let mut rows = bundle::decode_records(&fs::read(&path).unwrap()).unwrap();
    rows[2]["payload"]["item"]["content"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!({"type":"local_image","path":image}),
            json!({"type":"local_image","path":work.path().join("missing.png")}),
        ]);
    fs::write(&path, bundle::encode_records(&rows).unwrap()).unwrap();
    let output = work.path().join("images.zip");
    let manifest = bundle::export(source.path(), &[id], &output).unwrap();
    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(manifest.warnings.len(), 1);
    let packed = bundle::load(&output).unwrap();
    assert_eq!(
        packed.files[&manifest.assets[0].file],
        b"image fixture bytes"
    );
}

#[test]
fn native_existing_project_replacement_and_restore_keep_other_data() {
    let Ok(binary) = std::env::var("CODEX_TRANSFER_TEST_CODEX_EXE") else {
        eprintln!("Native test skipped: set CODEX_TRANSFER_TEST_CODEX_EXE");
        return;
    };
    let source = bare_home();
    let target = bare_home();
    let work = TempDir::new().unwrap();
    let id = Uuid::new_v4().to_string();
    fixture(source.path(), &id, None, "Original A message");
    let first = work.path().join("first.zip");
    bundle::export(source.path(), std::slice::from_ref(&id), &first).unwrap();
    let destination = work.path().join("Existing-todo");
    fs::create_dir(&destination).unwrap();
    let mut server = Rpc::start(Path::new(&binary), target.path()).unwrap();
    let project=server.call("project/create",json!({"name":"Existing Todo","roots":[{"path":destination}],"idempotencyKey":"existing-test"})).unwrap();
    let native = project["project"]["id"].as_str().unwrap().to_owned();
    drop(server);
    let home_key = format!(
        "local:{}",
        model::desktop_path(&target.path().to_string_lossy())
    );
    let state = json!({"local-projects":{"existing":{"id":"existing","name":"Existing Todo","rootPaths":[destination,work.path().join("deleted-auxiliary-folder")]}},
        "app-server-project-id-by-legacy-project-id-by-host":{home_key:{"existing":native}},"projectless-thread-ids":[id],"unrelated":"keep"});
    fs::write(
        target.path().join(".codex-global-state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let mut options = ImportOptions {
        home: target.path().into(),
        package: first,
        project_id: Some("existing".into()),
        target_dir: None,
        project_name: None,
        replace: false,
        codex_binary: Some(binary.into()),
        project_mappings: None,
    };
    let original = importer::import_checked(&options, allow_isolated_fixture).unwrap();
    assert_eq!(original.project.id, "existing");
    assert_eq!(original.project.roots.len(), 1);
    assert!(original
        .warnings
        .iter()
        .any(|w| w.contains("deleted-auxiliary-folder")));
    assert_eq!(
        model::read_json(&target.path().join(".codex-global-state.json")).unwrap()
            ["local-projects"]["existing"]["rootPaths"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    fixture(source.path(), &id, None, "Replacement A message");
    let next = work.path().join("next.zip");
    bundle::export(source.path(), std::slice::from_ref(&id), &next).unwrap();
    options.package = next;
    options.replace = true;
    let replaced = importer::import_checked(&options, allow_isolated_fixture).unwrap();
    assert_eq!(replaced.project.id, "existing");
    let catalog = model::scan(target.path()).unwrap();
    let rollout = &catalog.threads[0].path;
    assert!(fs::read_to_string(rollout)
        .unwrap()
        .contains("Replacement A message"));
    importer::restore_backup_checked(target.path(), Path::new(replaced.backup.as_ref().unwrap()))
        .unwrap();
    assert!(fs::read_to_string(rollout)
        .unwrap()
        .contains("Original A message"));
    assert!(!fs::read_to_string(rollout)
        .unwrap()
        .contains("Replacement A message"));
    assert_eq!(
        model::read_json(&target.path().join(".codex-global-state.json")).unwrap()["unrelated"],
        "keep"
    );
}

#[test]
fn malicious_backup_cannot_write_outside_the_selected_home() {
    let home = bare_home();
    let work = TempDir::new().unwrap();
    let victim = work.path().join("untouched.txt");
    fs::write(&victim, b"untouched").unwrap();
    let backup = home.path().join("chat-transfer-backups/invalid");
    fs::create_dir_all(&backup).unwrap();
    fs::write(backup.join("snapshot-0"), b"wrong").unwrap();
    fs::write(
        backup.join("backup.json"),
        serde_json::to_vec(&json!([{"path":victim,"file":"snapshot-0","database":false}])).unwrap(),
    )
    .unwrap();
    assert!(importer::restore_backup_checked(home.path(), &backup).is_err());
    assert_eq!(fs::read(&victim).unwrap(), b"untouched");
}

#[test]
fn catalog_resolves_duplicates_after_scanning_all_candidates() {
    let home = bare_home();
    let id = Uuid::new_v4().to_string();
    let preferred = fixture(home.path(), &id, None, "Preferred file");
    for suffix in ["a", "b"] {
        fs::copy(
            &preferred,
            preferred.with_file_name(format!("duplicate-{suffix}.jsonl")),
        )
        .unwrap();
    }
    let db = rusqlite::Connection::open(home.path().join("state_5.sqlite")).unwrap();
    db.execute_batch(
        "CREATE TABLE threads(id TEXT,rollout_path TEXT,title TEXT,updated_at INTEGER)",
    )
    .unwrap();
    db.execute(
        "INSERT INTO threads VALUES(?1,?2,'Preferred',0)",
        rusqlite::params![id, preferred.to_string_lossy()],
    )
    .unwrap();
    let catalog = model::scan(home.path()).unwrap();
    assert_eq!(catalog.threads.len(), 1);
    assert_eq!(catalog.threads[0].path, preferred);
    assert!(catalog.threads[0].export_error.is_none());
    drop(db);
    fs::remove_file(home.path().join("state_5.sqlite")).unwrap();
    let ambiguous = model::scan(home.path()).unwrap();
    assert_eq!(ambiguous.threads.len(), 1);
    assert!(ambiguous.threads[0].export_error.is_some());
    let work = TempDir::new().unwrap();
    assert!(bundle::export(home.path(), &[id], &work.path().join("invalid.zip")).is_err());
}

#[test]
fn a_missing_auxiliary_folder_does_not_block_a_valid_primary_folder() {
    let work = TempDir::new().unwrap();
    let primary = work.path().join("primary");
    fs::create_dir(&primary).unwrap();
    let missing = work.path().join("deleted-helper");
    let original = model::Project {
        id: "target".into(),
        name: "提醒项目".into(),
        roots: vec![
            primary.to_string_lossy().into_owned(),
            missing.to_string_lossy().into_owned(),
        ],
        native_id: Some("native".into()),
    };
    let effective = importer::validate_project_roots(original.clone()).unwrap();
    assert_eq!(effective.roots.len(), 1);
    assert_eq!(effective.id, original.id);
    assert_eq!(original.roots.len(), 2);
    let mut invalid = original;
    invalid.roots.swap(0, 1);
    let error = importer::validate_project_roots(invalid)
        .unwrap_err()
        .to_string();
    assert!(error.contains("主文件夹"));
    assert!(error.contains("deleted-helper"));
}

fn grouped_source() -> (TempDir, TempDir, PathBuf, Vec<String>) {
    let source = bare_home();
    let work = TempDir::new().unwrap();
    let ids = vec![Uuid::new_v4().to_string(), Uuid::new_v4().to_string()];
    let roots = [r"D:\Company\todo", r"D:\Company\notes"];
    let names = ["待办项目", "笔记项目"];
    let groups = ["source-todo", "source-notes"];
    let mut state = json!({"local-projects":{},"thread-project-assignments":{}});
    for index in 0..2 {
        let file = fixture(source.path(), &ids[index], None, names[index]);
        let mut records = bundle::decode_records(&fs::read(&file).unwrap()).unwrap();
        records[0]["payload"]["cwd"] = json!(roots[index]);
        records[0]["payload"]["runtime_workspace_roots"] = json!([roots[index]]);
        fs::write(file, bundle::encode_records(&records).unwrap()).unwrap();
        state["local-projects"][groups[index]] =
            json!({"id":groups[index],"name":names[index],"rootPaths":[roots[index]]});
        state["thread-project-assignments"][&ids[index]] =
            json!({"projectKind":"local","projectId":groups[index]});
    }
    fs::write(
        source.path().join(".codex-global-state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let package = work.path().join("grouped.zip");
    bundle::export(source.path(), &ids, &package).unwrap();
    (source, work, package, ids)
}

#[test]
fn project_names_ids_and_legacy_directory_groups_survive_export() {
    let (source, _work, package, ids) = grouped_source();
    let packed = bundle::load(&package).unwrap();
    assert_eq!(packed.manifest.version, 2);
    assert_eq!(packed.manifest.source_projects.len(), 2);
    let todo = packed
        .manifest
        .threads
        .iter()
        .find(|t| t.id == ids[0])
        .unwrap();
    assert_eq!(todo.source_project_id.as_deref(), Some("source-todo"));
    assert!(packed
        .manifest
        .source_projects
        .iter()
        .any(|p| p.id == "source-todo" && p.name == "待办项目"));
    let mut legacy = packed.manifest.clone();
    legacy.version = 1;
    legacy.source_projects.clear();
    for thread in &mut legacy.threads {
        thread.source_project_id = None;
    }
    bundle::normalize_groups(&mut legacy).unwrap();
    assert_eq!(legacy.source_projects.len(), 2);
    assert!(legacy.source_projects.iter().any(|p| p.name == "todo"));
    let mut state = model::read_json(&source.path().join(".codex-global-state.json")).unwrap();
    state["local-projects"]["source-todo"]["name"] = json!("同名项目");
    state["local-projects"]["source-notes"]["name"] = json!("同名项目");
    fs::write(
        source.path().join(".codex-global-state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let duplicate_names = package.with_file_name("same-names.zip");
    let manifest = bundle::export(source.path(), &ids, &duplicate_names).unwrap();
    assert_eq!(manifest.source_projects.len(), 2);
    assert!(manifest
        .source_projects
        .iter()
        .all(|p| p.name == "同名项目"));
    assert_ne!(
        manifest.source_projects[0].id,
        manifest.source_projects[1].id
    );
}

#[test]
fn incomplete_or_collapsed_project_mappings_are_rejected_before_writes() {
    let (_source, work, package, _ids) = grouped_source();
    let target = bare_home();
    let directory = work.path().join("B-todo");
    fs::create_dir(&directory).unwrap();
    let mapping = ProjectMapping {
        source_project_id: "source-todo".into(),
        project_id: None,
        target_dir: Some(directory.clone()),
        project_name: None,
    };
    let mut options = ImportOptions {
        home: target.path().into(),
        package,
        project_id: None,
        target_dir: None,
        project_name: None,
        replace: false,
        codex_binary: None,
        project_mappings: Some(vec![mapping.clone()]),
    };
    assert!(importer::import_checked(&options, allow_isolated_fixture)
        .unwrap_err()
        .to_string()
        .contains("映射未完整"));
    let mut second = mapping;
    second.source_project_id = "source-notes".into();
    options.project_mappings.as_mut().unwrap().push(second);
    assert!(importer::import_checked(&options, allow_isolated_fixture)
        .unwrap_err()
        .to_string()
        .contains("同一个项目或主文件夹"));
    assert!(!target.path().join("state_5.sqlite").exists());
    assert!(!target.path().join("chat-transfer-backups").exists());
}

#[test]
fn native_grouped_import_keeps_each_project_separate_and_is_idempotent() {
    let Ok(binary) = std::env::var("CODEX_TRANSFER_TEST_CODEX_EXE") else {
        return;
    };
    let (_source, work, package, ids) = grouped_source();
    let target = bare_home();
    let todo = work.path().join("B-todo");
    let notes = work.path().join("B-notes");
    fs::create_dir(&todo).unwrap();
    fs::create_dir(&notes).unwrap();
    let mut server = Rpc::start(Path::new(&binary), target.path()).unwrap();
    let native = server
        .call(
            "project/create",
            json!({"name":"待办项目","roots":[{"path":todo}],"idempotencyKey":"group-existing"}),
        )
        .unwrap()["project"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    drop(server);
    let host = format!(
        "local:{}",
        model::desktop_path(&target.path().to_string_lossy())
    );
    let state = json!({"local-projects":{"existing":{"id":"existing","name":"待办项目","rootPaths":[todo]}},"app-server-project-id-by-legacy-project-id-by-host":{host:{"existing":native}},"projectless-thread-ids":ids});
    fs::write(
        target.path().join(".codex-global-state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let options = ImportOptions {
        home: target.path().into(),
        package,
        project_id: None,
        target_dir: None,
        project_name: None,
        replace: false,
        codex_binary: Some(binary.clone().into()),
        project_mappings: Some(vec![
            ProjectMapping {
                source_project_id: "source-todo".into(),
                project_id: Some("existing".into()),
                target_dir: None,
                project_name: None,
            },
            ProjectMapping {
                source_project_id: "source-notes".into(),
                project_id: None,
                target_dir: Some(notes.clone()),
                project_name: None,
            },
        ]),
    };
    let report = importer::import_checked(&options, allow_isolated_fixture).unwrap();
    assert_eq!(report.projects.len(), 2);
    assert_eq!(report.imported, 2);
    assert_eq!(report.verified_message_count, 4);
    let catalog = model::scan(target.path()).unwrap();
    assert_eq!(catalog.projects.len(), 2);
    assert_eq!(
        catalog
            .threads
            .iter()
            .find(|t| t.id == ids[0])
            .unwrap()
            .project_id
            .as_deref(),
        Some("existing")
    );
    let note_project = report
        .projects
        .iter()
        .find(|p| p.name == "笔记项目")
        .unwrap();
    assert_eq!(
        catalog
            .threads
            .iter()
            .find(|t| t.id == ids[1])
            .unwrap()
            .project_id
            .as_deref(),
        Some(note_project.id.as_str())
    );
    let mut server = Rpc::start(Path::new(&binary), target.path()).unwrap();
    for project in &report.projects {
        let list=server.call("thread/list",json!({"projectId":project.native_id,"sourceKinds":[],"modelProviders":[],"limit":100})).unwrap();
        let rows = list["data"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            model::path_key(rows[0]["cwd"].as_str().unwrap()),
            model::path_key(&project.roots[0])
        );
    }
    drop(server);
    assert_eq!(
        importer::import_checked(&options, allow_isolated_fixture)
            .unwrap()
            .status,
        "alreadyImported"
    );
}

#[test]
fn a_failure_in_one_project_rolls_back_the_whole_grouped_package() {
    let Ok(binary) = std::env::var("CODEX_TRANSFER_TEST_CODEX_EXE") else {
        return;
    };
    let (source, work, _package, ids) = grouped_source();
    let target = bare_home();
    let broken = model::scan(source.path())
        .unwrap()
        .threads
        .into_iter()
        .find(|t| t.id == ids[1])
        .unwrap()
        .path;
    let records = bundle::decode_records(&fs::read(&broken).unwrap()).unwrap();
    fs::write(broken, bundle::encode_records(&records[..1]).unwrap()).unwrap();
    let package = work.path().join("broken-group.zip");
    bundle::export(source.path(), &ids, &package).unwrap();
    let todo = work.path().join("B-todo");
    let notes = work.path().join("B-notes");
    fs::create_dir(&todo).unwrap();
    fs::create_dir(&notes).unwrap();
    let before = fs::read(target.path().join(".codex-global-state.json")).unwrap();
    let options = ImportOptions {
        home: target.path().into(),
        package,
        project_id: None,
        target_dir: None,
        project_name: None,
        replace: false,
        codex_binary: Some(binary.into()),
        project_mappings: Some(vec![
            ProjectMapping {
                source_project_id: "source-todo".into(),
                project_id: None,
                target_dir: Some(todo),
                project_name: None,
            },
            ProjectMapping {
                source_project_id: "source-notes".into(),
                project_id: None,
                target_dir: Some(notes),
                project_name: None,
            },
        ]),
    };
    let error = importer::import_checked(&options, allow_isolated_fixture).unwrap_err();
    assert!(error.to_string().contains("已恢复原数据"));
    assert_eq!(
        fs::read(target.path().join(".codex-global-state.json")).unwrap(),
        before
    );
    assert!(!target.path().join("state_5.sqlite").exists());
    assert!(model::scan(target.path()).unwrap().threads.is_empty());
}
