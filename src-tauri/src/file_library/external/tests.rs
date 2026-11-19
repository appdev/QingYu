use super::*;

fn fixture() -> (tempfile::TempDir, DiskLibrary) {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("a.md"), "# original\r\n").unwrap();
    let store = DiskLibrary::open(temp.path().into(), None).unwrap();
    (temp, store)
}
fn buffer(store: &DiskLibrary, source: &str) -> Option<EditorBuffer> {
    Some(EditorBuffer {
        note_id: store.data.active_id.clone(),
        source: source.into(),
        expected_source: None,
    })
}

#[test]
fn source_contract_external_edit_atomic_replacement_rename_and_delete_never_recreate_old_paths() {
    let (temp, mut store) = fixture();
    let original_id = store.data.active_id.clone();
    fs::write(temp.path().join("replacement.tmp"), "# atomic replacement").unwrap();
    fs::rename(
        temp.path().join("replacement.tmp"),
        temp.path().join("a.md"),
    )
    .unwrap();
    let result = store.reconcile(None).unwrap();
    assert!(result.snapshot.is_some());
    assert_eq!(store.data.active_id, original_id);
    assert_eq!(store.data.notes[0].source, "# atomic replacement");
    fs::rename(
        temp.path().join("a.md"),
        temp.path().join("new-name.MARKDOWN"),
    )
    .unwrap();
    store.reconcile(None).unwrap();
    assert_eq!(store.data.notes.len(), 1);
    assert_eq!(store.data.notes[0].path, "new-name.MARKDOWN");
    assert!(!store.data.notes.iter().any(|n| n.id == original_id));
    fs::remove_file(temp.path().join("new-name.MARKDOWN")).unwrap();
    store.reconcile(None).unwrap();
    assert!(store.data.notes.is_empty());
    assert_eq!(store.data.active_id, "");
    assert!(!temp.path().join("a.md").exists());
    assert!(!temp.path().join("new-name.MARKDOWN").exists());
}

#[test]
fn source_contract_back_up_only_the_owned_active_buffer_and_keep_backup_out_of_note_list() {
    let (temp, mut store) = fixture();
    fs::write(temp.path().join("a.md"), "# external").unwrap();
    let draft = buffer(&store, "# unsaved Chinese 中文 🌸\n");
    let result = store.reconcile(draft).unwrap();
    assert_eq!(result.backups.len(), 1);
    assert_eq!(
        fs::read_to_string(temp.path().join(&result.backups[0])).unwrap(),
        "# unsaved Chinese 中文 🌸\n"
    );
    assert_eq!(store.data.notes.len(), 1);
    assert_eq!(store.data.notes[0].source, "# external");
    assert!(store
        .reconcile(buffer(&store, "# external"))
        .unwrap()
        .snapshot
        .is_none());
    fs::write(temp.path().join("a.md"), "# external again").unwrap();
    let stale_other_owner = Some(EditorBuffer {
        note_id: "another-note".into(),
        source: "wrong buffer".into(),
        expected_source: None,
    });
    assert!(store
        .reconcile(stale_other_owner)
        .unwrap()
        .backups
        .is_empty());
}

#[test]
fn deleted_unsaved_buffer_is_backed_up_and_failure_to_backup_leaves_memory_untouched() {
    let (temp, mut store) = fixture();
    let before = store.data.clone();
    fs::write(temp.path().join(".miaoyan-conflicts"), "occupied").unwrap();
    fs::write(temp.path().join("a.md"), "# external").unwrap();
    assert!(store.reconcile(buffer(&store, "local draft")).is_err());
    assert_eq!(store.data, before);
    fs::remove_file(temp.path().join(".miaoyan-conflicts")).unwrap();
    fs::remove_file(temp.path().join("a.md")).unwrap();
    let result = store.reconcile(buffer(&store, "local draft")).unwrap();
    assert_eq!(
        fs::read_to_string(temp.path().join(&result.backups[0])).unwrap(),
        "local draft"
    );
    assert!(store.data.notes.is_empty());
    assert!(!temp.path().join("a.md").exists());
}

#[test]
fn another_window_backs_up_only_a_dirty_draft_against_its_own_baseline() {
    let (temp, mut store) = fixture();
    fs::write(temp.path().join("b.md"), "另一篇初稿").unwrap();
    store.reconcile(None).unwrap();
    let note = store
        .data
        .notes
        .iter()
        .find(|note| note.path == "b.md")
        .unwrap()
        .clone();
    assert_ne!(store.data.active_id, note.id);
    let root = store.snapshot().root;
    store
        .save_note_body(
            &root,
            &note.id,
            &note.source,
            "另一窗口已保存".into(),
            "edit",
        )
        .unwrap();
    let buffer = |source: &str| {
        Some(EditorBuffer {
            note_id: note.id.clone(),
            source: source.into(),
            expected_source: Some(note.source.clone()),
        })
    };
    assert!(store
        .reconcile(buffer(&note.source))
        .unwrap()
        .backups
        .is_empty());
    let conflict = store.reconcile(buffer("独立窗口待保存草稿")).unwrap();
    assert_eq!(conflict.backups.len(), 1);
    assert_eq!(
        fs::read_to_string(temp.path().join(&conflict.backups[0])).unwrap(),
        "独立窗口待保存草稿"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join(&note.path)).unwrap(),
        "另一窗口已保存"
    );
}

#[test]
fn own_save_notifications_and_new_files_do_not_reload_unchanged_active_content_or_reset_metadata() {
    let (temp, mut store) = fixture();
    let mut next = store.data.clone();
    next.notes[0].source = "our own edit".into();
    next.notes[0].pinned = true;
    store.commit(next, "edit").unwrap();
    let index = fs::read(temp.path().join(INDEX)).unwrap();
    assert!(store
        .reconcile(buffer(&store, "our own edit"))
        .unwrap()
        .snapshot
        .is_none());
    assert_eq!(fs::read(temp.path().join(INDEX)).unwrap(), index);
    fs::write(temp.path().join("b.txt"), "new outside note").unwrap();
    let result = store.reconcile(buffer(&store, "our own edit")).unwrap();
    assert!(result.backups.is_empty());
    assert!(result.snapshot.is_some());
    assert!(store.data.notes.iter().any(|n| n.pinned));
    assert_eq!(store.data.notes.len(), 2);
}

#[test]
fn hidden_conflict_and_metadata_paths_are_ignored_but_missing_markdown_paths_are_observed() {
    let root = Path::new("/notes");
    for path in [
        ".miaoyan/library.json",
        ".miaoyan-conflicts/backup.md",
        ".hidden/n.md",
    ] {
        assert!(!relevant(root, &root.join(path)));
    }
    for path in ["removed.md", "renamed.txt", "new-folder/n.markdown"] {
        assert!(relevant(root, &root.join(path)));
    }
    assert!(!relevant(root, Path::new("/another-library/a.md")));
}

#[cfg(desktop)]
#[test]
fn real_recursive_watcher_observes_create_edit_rename_delete_and_stops_on_drop() {
    use std::{sync::mpsc, time::Duration};
    let (temp, mut store) = fixture();
    let (tx, rx) = mpsc::channel();
    let watch = Watch::start(store.root.clone(), true, move |result| {
        tx.send(result).unwrap();
    })
    .unwrap();
    let wait = |store: &mut DiskLibrary, expected: &str, source: Option<&str>| {
        let limit = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            rx.recv_timeout(limit.saturating_duration_since(std::time::Instant::now()))
                .expect("native filesystem event")
                .unwrap();
            store.reconcile(None).unwrap();
            if source.map_or_else(
                || !store.data.notes.iter().any(|n| n.path == expected),
                |text| {
                    store
                        .data
                        .notes
                        .iter()
                        .any(|n| n.path == expected && n.source == text)
                },
            ) {
                break;
            }
        }
    };
    fs::create_dir(temp.path().join("nested")).unwrap();
    fs::write(temp.path().join("nested/new.md"), "created").unwrap();
    wait(&mut store, "nested/new.md", Some("created"));
    fs::write(temp.path().join("nested/new.md"), "modified").unwrap();
    wait(&mut store, "nested/new.md", Some("modified"));
    fs::rename(
        temp.path().join("nested/new.md"),
        temp.path().join("nested/renamed.md"),
    )
    .unwrap();
    wait(&mut store, "nested/renamed.md", Some("modified"));
    fs::remove_file(temp.path().join("nested/renamed.md")).unwrap();
    wait(&mut store, "nested/renamed.md", None);
    drop(watch);
    assert!(rx.recv_timeout(Duration::from_secs(3)).is_err());
}

#[test]
fn external_folder_creation_rename_and_move_preserve_descendant_identity_and_selection() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("Work/2024")).unwrap();
    fs::create_dir(temp.path().join("Workshop")).unwrap();
    fs::write(temp.path().join("Work/2024/n.md"), "# original\r\n").unwrap();
    fs::write(temp.path().join("Workshop/peer.md"), "unrelated").unwrap();
    let mut store = DiskLibrary::open(temp.path().into(), None).unwrap();
    let mut next = store.data.clone();
    let selected = next
        .notes
        .iter_mut()
        .find(|n| n.path == "Work/2024/n.md")
        .unwrap();
    selected.pinned = true;
    let selected = selected.clone();
    next.active_id = selected.id.clone();
    store.commit(next, "select").unwrap();
    let parent_id = store
        .data
        .folders
        .iter()
        .find(|f| f.path == "Work")
        .unwrap()
        .id
        .clone();
    let child_id = selected.folder_id.clone();
    fs::create_dir_all(temp.path().join("Archive/Empty/Nested")).unwrap();
    store.reconcile(None).unwrap();
    assert!(store
        .data
        .folders
        .iter()
        .any(|f| f.path == "Archive/Empty/Nested"));
    fs::rename(temp.path().join("Work"), temp.path().join("Jobs")).unwrap();
    store.reconcile(buffer(&store, "# original\n")).unwrap();
    assert_eq!(
        store
            .data
            .folders
            .iter()
            .find(|f| f.id == parent_id)
            .unwrap()
            .path,
        "Jobs"
    );
    assert_eq!(
        store
            .data
            .folders
            .iter()
            .find(|f| f.id == child_id)
            .unwrap()
            .path,
        "Jobs/2024"
    );
    assert_eq!(store.data.active_id, selected.id);
    assert!(store
        .data
        .notes
        .iter()
        .any(|n| n.id == selected.id && n.pinned && n.path == "Jobs/2024/n.md"));
    fs::rename(temp.path().join("Jobs"), temp.path().join("Archive/Jobs")).unwrap();
    let result = store.reconcile(buffer(&store, "# unsaved buffer")).unwrap();
    assert!(
        result.backups.is_empty(),
        "an unchanged body moving with its directory does not overwrite the editor"
    );
    assert_eq!(store.data.active_id, selected.id);
    assert!(store
        .data
        .notes
        .iter()
        .any(|n| n.id == selected.id && n.path == "Archive/Jobs/2024/n.md"));
    assert_eq!(
        fs::read(temp.path().join("Archive/Jobs/2024/n.md")).unwrap(),
        b"# original\r\n"
    );
    assert!(store
        .data
        .notes
        .iter()
        .any(|n| n.path == "Workshop/peer.md"));
    let mut saved = store.data.clone();
    saved
        .notes
        .iter_mut()
        .find(|n| n.id == selected.id)
        .unwrap()
        .source = "# resumed pending edit".into();
    store.commit(saved, "edit").unwrap();
    assert_eq!(
        fs::read_to_string(temp.path().join("Archive/Jobs/2024/n.md")).unwrap(),
        "# resumed pending edit"
    );
    assert!(!temp.path().join("Work").exists());
    assert!(!temp.path().join("Jobs").exists());
}

#[test]
fn moved_out_folder_retires_tree_and_backs_up_unsaved_body_without_recreating_directories() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("A/Sub")).unwrap();
    fs::write(temp.path().join("A/Sub/n.md"), "body").unwrap();
    let mut store = DiskLibrary::open(temp.path().into(), None).unwrap();
    fs::rename(temp.path().join("A"), outside.path().join("A")).unwrap();
    let result = store
        .reconcile(buffer(&store, "unsaved local body"))
        .unwrap();
    assert!(store.data.notes.is_empty());
    assert!(store.data.folders.iter().all(|f| f.path.is_empty()));
    assert_eq!(
        fs::read_to_string(temp.path().join(&result.backups[0])).unwrap(),
        "unsaved local body"
    );
    assert!(!temp.path().join("A").exists());
    fs::rename(outside.path().join("A"), temp.path().join("Returned")).unwrap();
    store.reconcile(None).unwrap();
    assert!(store
        .data
        .notes
        .iter()
        .any(|n| n.path == "Returned/Sub/n.md"));
}

#[test]
fn renamed_folder_with_reoccupied_old_path_does_not_transfer_pin_or_identity_to_unrelated_files() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("A")).unwrap();
    fs::write(temp.path().join("A/n.md"), "original").unwrap();
    let mut store = DiskLibrary::open(temp.path().into(), None).unwrap();
    let mut next = store.data.clone();
    next.notes[0].pinned = true;
    store.commit(next, "pin").unwrap();
    let original_id = store.data.notes[0].id.clone();
    fs::rename(temp.path().join("A"), temp.path().join("B")).unwrap();
    fs::create_dir(temp.path().join("A")).unwrap();
    fs::write(temp.path().join("A/n.md"), "unrelated new file").unwrap();
    store.reconcile(None).unwrap();
    let original = store
        .data
        .notes
        .iter()
        .find(|n| n.id == original_id)
        .unwrap();
    assert_eq!(original.path, "B/n.md");
    assert!(original.pinned);
    let replacement = store
        .data
        .notes
        .iter()
        .find(|n| n.path == "A/n.md")
        .unwrap();
    assert_ne!(replacement.id, original_id);
    assert!(!replacement.pinned);
}

#[test]
fn metadata_commit_during_an_external_move_does_not_forget_the_old_directory_identity() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("A")).unwrap();
    fs::write(temp.path().join("A/n.md"), "original").unwrap();
    let mut store = DiskLibrary::open(temp.path().into(), None).unwrap();
    let selected = store.data.notes[0].id.clone();
    fs::rename(temp.path().join("A"), temp.path().join("B")).unwrap();
    store.commit(store.data.clone(), "select").unwrap();
    store.reconcile(None).unwrap();
    assert!(store
        .data
        .notes
        .iter()
        .any(|n| n.id == selected && n.path == "B/n.md"));
}

#[cfg(desktop)]
#[test]
fn real_watcher_reconciles_empty_and_populated_directory_events_as_whole_trees() {
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    let (temp, mut store) = fixture();
    let (tx, rx) = mpsc::channel();
    let watch = Watch::start(store.root.clone(), true, move |event| {
        let _ = tx.send(event);
    })
    .unwrap();
    fn wait(store: &mut DiskLibrary, rx: &mpsc::Receiver<Result<()>>, path: &str, exists: bool) {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("directory event")
                .unwrap();
            store.reconcile(None).unwrap();
            if store.data.folders.iter().any(|f| f.path == path) == exists {
                break;
            }
        }
    }
    fs::create_dir(temp.path().join("Empty")).unwrap();
    wait(&mut store, &rx, "Empty", true);
    let identity = store
        .data
        .folders
        .iter()
        .find(|f| f.path == "Empty")
        .unwrap()
        .id
        .clone();
    fs::rename(temp.path().join("Empty"), temp.path().join("Renamed")).unwrap();
    wait(&mut store, &rx, "Renamed", true);
    assert_eq!(
        store
            .data
            .folders
            .iter()
            .find(|f| f.path == "Renamed")
            .unwrap()
            .id,
        identity
    );
    fs::create_dir_all(temp.path().join("Target/Child")).unwrap();
    fs::write(temp.path().join("Renamed/n.md"), "moved note").unwrap();
    wait(&mut store, &rx, "Target/Child", true);
    let note = store
        .data
        .notes
        .iter()
        .find(|n| n.path == "Renamed/n.md")
        .unwrap()
        .id
        .clone();
    fs::rename(
        temp.path().join("Renamed"),
        temp.path().join("Target/Child/Renamed"),
    )
    .unwrap();
    wait(&mut store, &rx, "Target/Child/Renamed", true);
    assert!(store
        .data
        .notes
        .iter()
        .any(|n| n.id == note && n.path == "Target/Child/Renamed/n.md"));
    fs::remove_dir_all(temp.path().join("Target")).unwrap();
    wait(&mut store, &rx, "Target", false);
    assert!(!store.data.notes.iter().any(|n| n.id == note));
    drop(watch);
}
