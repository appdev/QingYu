use super::*;

#[test]
fn write_lock_excludes_other_handles_and_releases_on_close() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("write.lock");
    let first = fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path).unwrap();
    let second = fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
    try_lock(&first).unwrap();
    assert!(matches!(try_lock(&second), Err(fs::TryLockError::WouldBlock)));
    drop(first);
    try_lock(&second).unwrap();
}

fn open(root: &Path) -> DiskLibrary {
    DiskLibrary::open(root.into(), None).unwrap()
}
fn draft(title: Option<&str>, body: &str) -> Note {
    Note {
        id: id(),
        folder_id: "default".into(),
        source: body.into(),
        created_at: now(),
        updated_at: now(),
        title: title.map(str::to_owned),
        pinned: false,
        trashed: false,
        conflict_of: None,
        path: String::new(),
        read_only: false,
        filename: None,
    }
}
fn create(store: &mut DiskLibrary, title: Option<&str>, body: &str) -> Note {
    let note = draft(title, body);
    let mut next = store.data.clone();
    next.active_id = note.id.clone();
    next.notes.push(note);
    store.commit(next, "create").unwrap();
    store.data.notes.last().unwrap().clone()
}

#[test]
fn matches_executed_swift_namehelper_for_unicode_slashes_and_numeric_suffixes() {
    #[derive(Deserialize)]
    struct Case {
        input: String,
        file: String,
    }
    let cases: Vec<Case> = serde_json::from_str(include_str!("namehelper-reference.json")).unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    for case in cases {
        assert_eq!(
            create(&mut store, Some(&case.input), "body").path,
            case.file,
            "Swift NameHelper input: {}",
            case.input
        );
    }
}
#[test]
fn creates_uuid_file_and_names_from_filename_not_markdown() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = open(temp.path());
    let note = create(&mut store, None, "# 正文标题\r\n\r\n保留正文\r\n");
    assert!(Uuid::parse_str(Path::new(&note.path).file_stem().unwrap().to_str().unwrap()).is_ok());
    assert_eq!(note.title.as_deref(), Some("无标题"));
    assert!(note.path.ends_with(".md"));
    assert_eq!(
        fs::read(temp.path().join(&note.path)).unwrap(),
        note.source.as_bytes()
    );
    let index = fs::read_to_string(temp.path().join(INDEX)).unwrap();
    assert!(!index.contains("正文标题"));
    assert_eq!(open(temp.path()).data.notes[0].source, note.source);
}
#[test]
fn namehelper_conflicts_and_character_mapping_match_miaoyan() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = open(temp.path());
    assert_eq!(
        create(&mut store, Some("  A:B/C  "), "one").path,
        "A-B:C.md"
    );
    for (title, file) in [
        ("同名", "同名.md"),
        ("同名", "同名 1.md"),
        ("同名 1", "同名 2.md"),
        ("同名 9", "同名 9.md"),
        ("同名 9", "同名 3.md"),
    ] {
        assert_eq!(create(&mut store, Some(title), "body").path, file);
    }
    assert_eq!(
        fs::read_to_string(temp.path().join("同名.md")).unwrap(),
        "body"
    );
}
#[test]
fn rename_rejects_collision_preserves_extension_bytes_and_date_allows_case_only() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("Note.markdown"), b"# title\r\n").unwrap();
    fs::write(temp.path().join("other.markdown"), b"other").unwrap();
    let mut store = open(temp.path());
    let position = store
        .data
        .notes
        .iter()
        .position(|n| n.path == "Note.markdown")
        .unwrap();
    let old = store.data.notes[position].clone();
    let mut next = store.data.clone();
    next.notes[position].title = Some("other".into());
    assert!(store.commit(next, "edit").is_err());
    assert_eq!(
        fs::read(temp.path().join(&old.path)).unwrap(),
        b"# title\r\n"
    );
    let mut next = store.data.clone();
    next.notes[position].title = Some("note".into());
    store.commit(next, "edit").unwrap();
    assert_eq!(store.data.notes[position].path, "note.markdown");
    assert_eq!(store.data.notes[position].updated_at, old.updated_at);
    let mut next = store.data.clone();
    next.notes[position].title = Some(" :/ ".into());
    store.commit(next, "edit").unwrap();
    assert_eq!(store.data.notes[position].path, "-:.markdown");
    assert_eq!(
        fs::read(temp.path().join("-:.markdown")).unwrap(),
        b"# title\r\n"
    );
}
#[test]
fn imports_files_and_directories_flattened_deduplicated_with_copy_conflicts_and_original_bytes() {
    let root = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    for dir in ["sub/deep", "i", "files", ".Trash", ".hidden"] {
        fs::create_dir_all(source.path().join(dir)).unwrap();
    }
    for file in [
        "one.MD",
        "sub/deep/two.markdown",
        "sub/three.txt",
        "i/skip.md",
        "files/skip.md",
        ".Trash/skip.md",
        ".hidden/skip.md",
        "skip.png",
    ] {
        fs::write(source.path().join(file), b"# imported\r\n\r\n").unwrap();
    }
    assert_eq!(
        collect_imports(&[source.path().into(), source.path().join("one.MD")])
            .unwrap()
            .len(),
        3
    );
    let mut store = open(root.path());
    assert_eq!(store.import(&[source.path().into()]).unwrap(), 3);
    assert_eq!(store.data.notes.len(), 3);
    assert!(store.data.notes.iter().all(|n| !n.path.contains('/')));
    store.import(&[source.path().join("one.MD")]).unwrap();
    assert!(root.path().join("one Copy.MD").exists());
    store.import(&[source.path().join("one.MD")]).unwrap();
    assert!(root.path().join("one Copy 2.MD").exists());
    assert_eq!(
        fs::read(root.path().join("one.MD")).unwrap(),
        fs::read(source.path().join("one.MD")).unwrap()
    );
    let count = store.data.notes.len();
    assert_eq!(store.import(&[root.path().join("one.MD")]).unwrap(), 0);
    assert_eq!(store.data.notes.len(), count);
}
#[test]
fn saves_fail_closed_for_external_changes_deletion_or_damaged_text() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = open(temp.path());
    let note = create(&mut store, Some("笔记"), "old");
    fs::write(temp.path().join(&note.path), b"external").unwrap();
    let mut next = store.data.clone();
    next.notes[0].source = "local".into();
    assert!(store.commit(next, "edit").is_err());
    assert_eq!(fs::read(temp.path().join(&note.path)).unwrap(), b"external");
    if temp.path().join(JOURNAL).exists() {
        fs::remove_file(temp.path().join(JOURNAL)).unwrap();
    }
    fs::remove_file(temp.path().join(&note.path)).unwrap();
    let mut next = store.data.clone();
    next.notes[0].source = "resurrect".into();
    assert!(store.commit(next, "edit").is_err());
    assert!(!temp.path().join(&note.path).exists());
    let damaged = tempfile::tempdir().unwrap();
    let mut bytes = "# 中文正文\n".as_bytes().to_vec();
    bytes.push(0xff);
    fs::write(damaged.path().join("bad.md"), &bytes).unwrap();
    let mut store = open(damaged.path());
    assert!(store.data.notes[0].read_only);
    let mut next = store.data.clone();
    next.notes[0].source = "bad write".into();
    assert!(store.commit(next, "edit").is_err());
    assert_eq!(fs::read(damaged.path().join("bad.md")).unwrap(), bytes);
}
#[test]
fn move_and_trash_keep_real_files_and_restore_uses_conflict_rule() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = open(temp.path());
    let note = create(&mut store, Some("保留"), "exact");
    let mut next = store.data.clone();
    next.folders.push(Folder {
        id: "travel".into(),
        name: "旅行".into(),
        path: String::new(),
    });
    store.commit(next, "edit").unwrap();
    let mut next = store.data.clone();
    next.notes[0].folder_id = "travel".into();
    store.commit(next, "edit").unwrap();
    assert!(temp.path().join("旅行/保留.md").exists());
    let mut next = store.data.clone();
    next.notes[0].trashed = true;
    store.commit(next, "edit").unwrap();
    assert!(temp.path().join("Trash/保留.md").exists());
    fs::write(temp.path().join("旅行/保留.md"), b"another").unwrap();
    let mut next = store.data.clone();
    next.notes[0].trashed = false;
    store.commit(next, "edit").unwrap();
    assert!(temp.path().join("旅行/保留 1.md").exists());
    assert_eq!(store.data.notes[0].id, note.id);
}
#[test]
fn migration_preserves_sources_and_does_not_overwrite_existing_files() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("迁移.md"), b"existing").unwrap();
    let seed = Library {
        version: 1,
        folders: vec![Folder {
            id: "default".into(),
            name: "备忘录".into(),
            path: String::new(),
        }],
        notes: vec![draft(Some("迁移"), "# exact\r\n")],
        active_id: String::new(),
        sync: None,
    };
    let store = DiskLibrary::open(temp.path().into(), Some(seed)).unwrap();
    assert_eq!(fs::read(temp.path().join("迁移.md")).unwrap(), b"existing");
    assert_eq!(
        fs::read(temp.path().join("迁移 1.md")).unwrap(),
        b"# exact\r\n"
    );
    assert_eq!(store.data.notes.len(), 2);
    assert!(temp
        .path()
        .join(".miaoyan/localStorage-backup.json")
        .exists());
    assert_eq!(
        DiskLibrary::open(temp.path().into(), Some(store.data.clone()))
            .unwrap()
            .data
            .notes
            .len(),
        2
    );
}
#[test]
fn interrupted_creation_rolls_back_only_its_own_file() {
    let temp = tempfile::tempdir().unwrap();
    let store = open(temp.path());
    let stage_name = format!(".miaoyan/staged-{}", id());
    let staged = stage_name.as_str();
    fs::write(temp.path().join(staged), b"pending").unwrap();
    let operation = Operation::Create {
        path: "pending.md".into(),
        bytes: b"pending".to_vec(),
        staged: staged.into(),
        modified: now(),
        identity: file_identity(&temp.path().join(staged)),
    };
    operation.apply(temp.path()).unwrap();
    // Android publishes by rename, so recovery cannot rely on a remaining hard link.
    if temp.path().join(staged).exists() { fs::remove_file(temp.path().join(staged)).unwrap(); }
    atomic(
        &temp.path().join(JOURNAL),
        &serde_json::to_vec(&Transaction {
            generation: store.revision + 1,
            operations: vec![operation],
            asset_sync: false,
        })
        .unwrap(),
        false,
    )
    .unwrap();
    let store = open(temp.path());
    assert!(store.data.notes.is_empty());
    assert!(!temp.path().join("pending.md").exists());
    let staged = ".miaoyan/foreign-stage";
    fs::write(temp.path().join(staged), b"same").unwrap();
    fs::write(temp.path().join("foreign.md"), b"same").unwrap();
    let operation = Operation::Create {
        path: "foreign.md".into(),
        bytes: b"same".to_vec(),
        staged: staged.into(),
        modified: now(),
        identity: file_identity(&temp.path().join(staged)),
    };
    operation.undo(temp.path()).unwrap();
    assert!(temp.path().join("foreign.md").exists());
}
#[test]
fn discovers_empty_folders_and_does_not_block_reload_after_external_edit() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("空文件夹")).unwrap();
    let mut store = open(temp.path());
    assert!(store.data.folders.iter().any(|f| f.path == "空文件夹"));
    let note = create(&mut store, Some("外部修改"), "old");
    fs::write(temp.path().join(&note.path), b"external").unwrap();
    let mut next = store.data.clone();
    next.notes[0].source = "local".into();
    assert!(store.commit(next, "edit").is_err());
    assert!(!temp.path().join(JOURNAL).exists());
    assert_eq!(open(temp.path()).data.notes[0].source, "external");
}
#[test]
fn duplicate_conflicts_include_files_created_by_another_editor() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = open(temp.path());
    let original = create(&mut store, Some("原文"), "latest");
    fs::write(temp.path().join("原文 Copy.md"), b"foreign").unwrap();
    let mut next = store.data.clone();
    next.notes.push(draft(Some("原文 Copy"), "latest"));
    store
        .commit(next, &format!("duplicate:{}", original.id))
        .unwrap();
    assert_eq!(store.data.notes.last().unwrap().path, "原文 Copy 2.md");
    assert_eq!(
        fs::read(temp.path().join("原文 Copy.md")).unwrap(),
        b"foreign"
    );
}
#[test]
fn combined_rename_and_body_save_is_one_transaction_and_file_extension_survives() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = open(temp.path());
    create(&mut store, Some("before"), "old");
    let mut next = store.data.clone();
    next.notes[0].title = Some("after".into());
    next.notes[0].source = "new".into();
    next.notes[0].updated_at = now();
    store.commit(next, "sync").unwrap();
    assert!(!temp.path().join("before.md").exists());
    assert_eq!(fs::read(temp.path().join("after.md")).unwrap(), b"new");
}
#[test]
fn import_keeps_utf16_and_big5_bytes_and_modified_existing_file_is_not_recreated() {
    let root = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let text = "# 中文測試\r\n";
    let mut utf16 = vec![0xff, 0xfe];
    for c in text.encode_utf16() {
        utf16.extend(c.to_le_bytes());
    }
    fs::write(source.path().join("unicode.txt"), &utf16).unwrap();
    let (big5, _, _) = encoding_rs::BIG5.encode(text);
    fs::write(source.path().join("big5.txt"), big5.as_ref()).unwrap();
    let mut store = open(root.path());
    store.import(&[source.path().into()]).unwrap();
    assert_eq!(fs::read(root.path().join("unicode.txt")).unwrap(), utf16);
    assert_eq!(
        fs::read(root.path().join("big5.txt")).unwrap(),
        big5.as_ref()
    );
    assert_eq!(
        store
            .data
            .notes
            .iter()
            .find(|n| n.path == "unicode.txt")
            .unwrap()
            .source,
        text
    );
}
#[test]
fn migration_reuses_existing_directory_without_overwriting_its_notes() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("旅行")).unwrap();
    fs::write(root.path().join("旅行/路线.md"), b"existing").unwrap();
    let mut seed_note = draft(Some("路线"), "legacy");
    seed_note.folder_id = "old-travel".into();
    let seed = Library {
        version: 1,
        active_id: seed_note.id.clone(),
        notes: vec![seed_note],
        folders: vec![
            Folder {
                id: "default".into(),
                name: "备忘录".into(),
                path: String::new(),
            },
            Folder {
                id: "old-travel".into(),
                name: "旅行".into(),
                path: String::new(),
            },
        ],
        sync: None,
    };
    let store = DiskLibrary::open(root.path().into(), Some(seed)).unwrap();
    assert_eq!(store.data.notes.len(), 2);
    assert_eq!(
        fs::read(root.path().join("旅行/路线.md")).unwrap(),
        b"existing"
    );
    assert_eq!(
        fs::read(root.path().join("旅行/路线 1.md")).unwrap(),
        b"legacy"
    );
}
#[test]
fn unreadable_note_does_not_block_other_documents_and_invalid_metadata_is_not_rewritten() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    create(&mut store, Some("不可读"), "keep");
    store.bytes.remove(&store.data.notes[0].id);
    store.data.notes[0].read_only = true;
    create(&mut store, Some("仍可创建"), "new");
    assert!(root.path().join("仍可创建.md").exists());
    let mut index: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join(INDEX)).unwrap()).unwrap();
    index["library"]["version"] = 99.into();
    let damaged = serde_json::to_vec(&index).unwrap();
    fs::write(root.path().join(INDEX), &damaged).unwrap();
    assert!(DiskLibrary::open(root.path().into(), None).is_err());
    assert_eq!(fs::read(root.path().join(INDEX)).unwrap(), damaged);
}
#[test]
fn import_and_edit_preserve_file_metadata_and_special_names_move_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let source_path = source.path().join("attrs.md");
    fs::write(&source_path, b"body").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&source_path, fs::Permissions::from_mode(0o640)).unwrap();
    }
    let before = fs::metadata(&source_path).unwrap();
    let mut store = open(root.path());
    store.import(&[source_path]).unwrap();
    let imported = fs::metadata(root.path().join("attrs.md")).unwrap();
    #[cfg(target_vendor = "apple")]
    assert_eq!(imported.created().unwrap(), before.created().unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(imported.permissions().mode() & 0o777, 0o640);
    }
    let mut next = store.data.clone();
    next.notes[0].source = "edited".into();
    store.commit(next, "edit").unwrap();
    #[cfg(target_vendor = "apple")]
    assert_eq!(
        fs::metadata(root.path().join("attrs.md"))
            .unwrap()
            .created()
            .unwrap(),
        before.created().unwrap()
    );
    let special = create(&mut store, Some("A/B"), "special");
    let mut next = store.data.clone();
    next.folders.push(Folder {
        id: "folder".into(),
        name: "目标".into(),
        path: String::new(),
    });
    next.notes
        .iter_mut()
        .find(|n| n.id == special.id)
        .unwrap()
        .folder_id = "folder".into();
    store.commit(next, "edit").unwrap();
    assert!(root.path().join("目标/A:B.md").exists());
}
#[test]
fn import_keeps_legacy_bytes_and_symlink_scan_is_loop_free() {
    let root = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let (bytes, _, _) = encoding_rs::GBK.encode("# 中文测试\r\n");
    fs::write(source.path().join("legacy.txt"), bytes.as_ref()).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(source.path(), source.path().join("loop")).unwrap();
    let mut store = open(root.path());
    store.import(&[source.path().into()]).unwrap();
    assert_eq!(
        fs::read(root.path().join("legacy.txt")).unwrap(),
        bytes.as_ref()
    );
}
