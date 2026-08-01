use std::collections::{BTreeMap, HashSet};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::contract::{
    AppConfigLocalStateDto, DocumentContents, FileTreeSortDirection, FileTreeSortKey, Nullable,
    RecentMarkdownFileDto, Revision, StoredFileTreeSortDto, StoredWorkspaceDraftDto,
    StoredWorkspaceLayoutDto, StoredWorkspaceSplitGroupDto, StoredWorkspaceWindowDto,
    StoredWorkspaceWindowStateDto, WindowLabel, WorkspaceLayoutPatchDto, WorkspaceRelativePath,
};

pub(crate) const APP_CONFIG_VERSION: u8 = 1;
pub(crate) const APP_CONFIG_VERSION_KEY: &str = "appConfigVersion";
pub(crate) const UI_LAYOUT_KEY: &str = "uiLayout";
pub(crate) const RECENT_FILES_KEY: &str = "recentMarkdownFilesByWorkspace";
pub(crate) const FILE_TREE_SORT_KEY: &str = "fileTreeSortByWorkspace";
pub(crate) const PANDOC_PATH_KEY: &str = "pandocPath";
pub(crate) const APP_CONFIG_MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_AGGREGATE_DRAFT_BYTES: usize = 48 * 1024 * 1024;
const MAX_RECENT_FILES: usize = 10;

pub(crate) fn default_layout() -> StoredWorkspaceLayoutDto {
    StoredWorkspaceLayoutDto {
        schema_version: APP_CONFIG_VERSION,
        window_states: BTreeMap::new(),
        open_windows: Vec::new(),
    }
}

pub(crate) fn default_window_state() -> StoredWorkspaceWindowStateDto {
    StoredWorkspaceWindowStateDto {
        active_draft_id: Nullable::null(),
        draft_tabs: Vec::new(),
        file_tree_assets_visible: true,
        file_path: Nullable::null(),
        file_tree_open: false,
        folder_name: Nullable::null(),
        folder_path: Nullable::null(),
        open_file_paths: Vec::new(),
        side_by_side_group: Nullable::null(),
    }
}

pub(crate) fn default_sort() -> StoredFileTreeSortDto {
    StoredFileTreeSortDto {
        key: FileTreeSortKey::Name,
        direction: FileTreeSortDirection::Ascending,
    }
}

pub(crate) fn local_state(
    revision: Revision,
    ui_layout: StoredWorkspaceLayoutDto,
    recent_markdown_files: Vec<RecentMarkdownFileDto>,
    file_tree_sort: StoredFileTreeSortDto,
    pandoc_path: Nullable<String>,
) -> AppConfigLocalStateDto {
    AppConfigLocalStateDto {
        revision,
        ui_layout,
        recent_markdown_files,
        file_tree_sort,
        pandoc_path,
    }
}

pub(crate) fn markdown_path(path: &WorkspaceRelativePath) -> bool {
    let lower = path.as_str().to_ascii_lowercase();
    !lower.is_empty() && (lower.ends_with(".md") || lower.ends_with(".markdown"))
}

fn valid_bounded_string(value: &str, max_chars: usize) -> bool {
    !value.is_empty() && value.chars().count() <= max_chars && !value.chars().any(char::is_control)
}

pub(crate) fn normalize_recent_file(
    mut file: RecentMarkdownFileDto,
) -> Result<RecentMarkdownFileDto, ()> {
    file.name = file.name.trim().to_string();
    if !valid_bounded_string(&file.name, 255) || !markdown_path(&file.path) {
        return Err(());
    }
    Ok(file)
}

pub(crate) fn remember_recent_file(
    recent: &mut Vec<RecentMarkdownFileDto>,
    file: RecentMarkdownFileDto,
) -> Result<(), ()> {
    let file = normalize_recent_file(file)?;
    recent.retain(|current| current.path != file.path);
    recent.insert(0, file);
    recent.truncate(MAX_RECENT_FILES);
    Ok(())
}

pub(crate) fn normalize_pandoc_path(path: Nullable<String>) -> Result<Nullable<String>, ()> {
    match path.into_option() {
        None => Ok(Nullable::null()),
        Some(path) => {
            if path.chars().any(char::is_control) {
                return Err(());
            }
            let path = path.trim().to_string();
            if path.is_empty() {
                return Ok(Nullable::null());
            }
            if path.chars().count() > 500 || path.chars().any(char::is_control) {
                return Err(());
            }
            Ok(Nullable::value(path))
        }
    }
}

pub(crate) fn normalize_layout(layout: &mut StoredWorkspaceLayoutDto) -> Result<(), ()> {
    if layout.schema_version != APP_CONFIG_VERSION {
        return Err(());
    }
    for (label, state) in &mut layout.window_states {
        let normalized = WindowLabel::parse(label).map_err(|_| ())?;
        if normalized.as_str() != label {
            return Err(());
        }
        normalize_window_state(state)?;
    }
    normalize_windows(&mut layout.open_windows)?;
    validate_aggregate_drafts(layout)
}

pub(crate) fn validate_layout_patch(patch: &WorkspaceLayoutPatchDto) -> Result<(), ()> {
    let mut state = default_window_state();
    if let Some(value) = &patch.active_draft_id {
        state.active_draft_id = value.clone();
    }
    if let Some(value) = &patch.draft_tabs {
        state.draft_tabs = value.clone();
    }
    if let Some(value) = patch.file_tree_assets_visible {
        state.file_tree_assets_visible = value;
    }
    if let Some(value) = &patch.file_path {
        state.file_path = value.clone();
    }
    if let Some(value) = patch.file_tree_open {
        state.file_tree_open = value;
    }
    if let Some(value) = &patch.folder_name {
        state.folder_name = value.clone();
    }
    if let Some(value) = &patch.folder_path {
        state.folder_path = value.clone();
    }
    if let Some(value) = &patch.open_file_paths {
        state.open_file_paths = value.clone();
    }
    if let Some(value) = &patch.side_by_side_group {
        state.side_by_side_group = value.clone();
    }
    let mut layout = default_layout();
    layout.window_states.insert("preflight".to_string(), state);
    if let Some(value) = &patch.open_windows {
        layout.open_windows = value.clone();
    }
    normalize_layout(&mut layout)
}

pub(crate) fn normalize_window_state(state: &mut StoredWorkspaceWindowStateDto) -> Result<(), ()> {
    state.active_draft_id = normalize_nullable_string(state.active_draft_id.clone(), 128)?;
    state.folder_name = normalize_nullable_string(state.folder_name.clone(), 255)?;
    if state
        .file_path
        .as_ref()
        .is_some_and(|path| !markdown_path(path))
        || state
            .open_file_paths
            .iter()
            .any(|path| !markdown_path(path))
    {
        return Err(());
    }
    dedupe_paths(&mut state.open_file_paths);
    normalize_drafts(&mut state.draft_tabs)?;
    if let Some(group) = state.side_by_side_group.as_ref() {
        validate_split_group(group)?;
    }
    if let Some(active) = state.active_draft_id.as_ref() {
        if !state
            .draft_tabs
            .iter()
            .any(|draft| draft.id.as_str() == active.as_str())
        {
            state.active_draft_id = Nullable::null();
        }
    }
    Ok(())
}

fn normalize_nullable_string(
    value: Nullable<String>,
    max_chars: usize,
) -> Result<Nullable<String>, ()> {
    match value.into_option() {
        None => Ok(Nullable::null()),
        Some(value) => {
            if value.chars().any(char::is_control) {
                return Err(());
            }
            let value = value.trim().to_string();
            if value.is_empty() {
                return Ok(Nullable::null());
            }
            if !valid_bounded_string(&value, max_chars) {
                return Err(());
            }
            Ok(Nullable::value(value))
        }
    }
}

fn normalize_drafts(drafts: &mut Vec<StoredWorkspaceDraftDto>) -> Result<(), ()> {
    let mut ids = HashSet::new();
    for draft in drafts {
        if draft.id.chars().any(char::is_control) || draft.name.chars().any(char::is_control) {
            return Err(());
        }
        draft.id = draft.id.trim().to_string();
        draft.name = draft.name.trim().to_string();
        if !valid_bounded_string(&draft.id, 128)
            || !valid_bounded_string(&draft.name, 255)
            || !ids.insert(draft.id.clone())
            || draft.path.as_ref().is_some_and(|path| !markdown_path(path))
            || DocumentContents::exceeds_limit(draft.content.as_str())
        {
            return Err(());
        }
    }
    Ok(())
}

fn normalize_windows(windows: &mut Vec<StoredWorkspaceWindowDto>) -> Result<(), ()> {
    let mut labels = HashSet::new();
    for window in windows {
        if !labels.insert(window.label.clone())
            || window
                .file_path
                .as_ref()
                .is_some_and(|path| !markdown_path(path))
            || window
                .open_file_paths
                .iter()
                .any(|path| !markdown_path(path))
        {
            return Err(());
        }
        dedupe_paths(&mut window.open_file_paths);
    }
    Ok(())
}

fn validate_split_group(group: &StoredWorkspaceSplitGroupDto) -> Result<(), ()> {
    if !markdown_path(&group.primary_file_path)
        || !markdown_path(&group.side_file_path)
        || group.primary_file_path == group.side_file_path
    {
        Err(())
    } else {
        Ok(())
    }
}

fn validate_aggregate_drafts(layout: &StoredWorkspaceLayoutDto) -> Result<(), ()> {
    let aggregate = layout
        .window_states
        .values()
        .flat_map(|state| &state.draft_tabs)
        .try_fold(0usize, |total, draft| {
            total.checked_add(draft.content.as_str().len())
        })
        .ok_or(())?;
    if aggregate > MAX_AGGREGATE_DRAFT_BYTES {
        Err(())
    } else {
        Ok(())
    }
}

fn dedupe_paths(paths: &mut Vec<WorkspaceRelativePath>) {
    let mut seen = HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalRevisionPayload<'a> {
    ui_layout: &'a Value,
    recent_markdown_files_by_workspace: &'a Value,
    file_tree_sort_by_workspace: &'a Value,
    pandoc_path: &'a Value,
}

pub(crate) fn local_revision(
    ui_layout: &Value,
    recent: &Value,
    sort: &Value,
    pandoc: &Value,
) -> Result<Revision, ()> {
    let bytes = serde_json::to_vec(&LocalRevisionPayload {
        ui_layout,
        recent_markdown_files_by_workspace: recent,
        file_tree_sort_by_workspace: sort,
        pandoc_path: pandoc,
    })
    .map_err(|_| ())?;
    Revision::parse(format!("sha256:{:x}", Sha256::digest(bytes))).map_err(|_| ())
}
