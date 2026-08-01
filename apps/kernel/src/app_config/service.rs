use std::{collections::BTreeMap, fmt, sync::Arc};

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    contract::{
        AppConfigSnapshotDto, AppConfigStateOperationDto, AppConfigWorkspaceDto, DomainEvent,
        Nullable, PatchAppConfigStateRequest, RecentMarkdownFileDto, ResourceRefDto,
        StoredFileTreeSortDto, StoredWorkspaceLayoutDto, WindowLabel, WorkspaceGeneration,
        WorkspaceId,
    },
    events::{EventPublication, EventSink},
    settings::{
        service::{
            SettingsRuntimeCoordinator, SettingsService, SettingsServiceError,
            SettingsServiceErrorKind,
        },
        storage::{SettingsStore, SettingsStoreError, SettingsStoreErrorKind},
    },
};

use super::model::{
    default_layout, default_sort, default_window_state, local_revision, local_state, markdown_path,
    normalize_layout, normalize_pandoc_path, normalize_recent_file, remember_recent_file,
    validate_aggregate_draft_contents, validate_layout_patch, APP_CONFIG_VERSION,
    APP_CONFIG_VERSION_KEY, FILE_TREE_SORT_KEY, PANDOC_PATH_KEY, RECENT_FILES_KEY, UI_LAYOUT_KEY,
};

pub struct AppConfigService {
    store: Arc<dyn SettingsStore>,
    settings: Arc<SettingsService>,
    coordinator: Arc<SettingsRuntimeCoordinator>,
    workspace_id: WorkspaceId,
    workspace_generation: WorkspaceGeneration,
    events: Arc<dyn EventSink>,
}

impl AppConfigService {
    pub fn new(
        store: Arc<dyn SettingsStore>,
        settings: Arc<SettingsService>,
        coordinator: Arc<SettingsRuntimeCoordinator>,
        workspace_id: WorkspaceId,
        workspace_generation: WorkspaceGeneration,
        events: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            store,
            settings,
            coordinator,
            workspace_id,
            workspace_generation,
            events,
        }
    }

    pub fn read(&self) -> Result<AppConfigSnapshotDto, AppConfigServiceError> {
        let _transaction = self
            .coordinator
            .transaction_gate_ref()
            .lock()
            .map_err(|_| AppConfigServiceError::unavailable())?;
        self.coordinator
            .ensure_available()
            .map_err(map_settings_error)?;
        let settings = self
            .settings
            .read_exposed_unlocked()
            .map_err(map_settings_error)?;
        let state = self.load_state()?;
        state.snapshot(
            settings,
            self.workspace_id,
            self.workspace_generation.clone(),
        )
    }

    pub fn patch_state(
        &self,
        request: PatchAppConfigStateRequest,
    ) -> Result<AppConfigSnapshotDto, AppConfigServiceError> {
        if request.workspace_generation != self.workspace_generation {
            return Err(AppConfigServiceError::stale_generation());
        }
        request
            .validate()
            .map_err(|_| AppConfigServiceError::invalid())?;
        preflight_operations(&request.operations)?;

        let (snapshot, publication) = {
            let _transaction = self
                .coordinator
                .transaction_gate_ref()
                .lock()
                .map_err(|_| AppConfigServiceError::unavailable())?;
            self.coordinator
                .ensure_available()
                .map_err(map_settings_error)?;
            let settings = self
                .settings
                .read_exposed_unlocked()
                .map_err(map_settings_error)?;
            let mut state = self.load_state()?;
            state.apply(request.operations)?;
            let changes = state.storage_values()?;
            self.commit(&changes)?;
            let snapshot = state.snapshot(
                settings,
                self.workspace_id,
                self.workspace_generation.clone(),
            )?;
            let revision = snapshot.local_state.revision.clone();
            let publication = EventPublication {
                resource: ResourceRefDto::AppConfig {
                    workspace_id: self.workspace_id,
                    workspace_generation: self.workspace_generation.clone(),
                },
                revision: revision.clone(),
                event: DomainEvent::AppConfigStateChanged {
                    workspace_id: self.workspace_id,
                    workspace_generation: self.workspace_generation.clone(),
                    revision,
                },
            };
            (snapshot, publication)
        };
        let _publication_result = self.events.publish(&publication);
        Ok(snapshot)
    }

    fn load_state(&self) -> Result<LoadedState, AppConfigServiceError> {
        let workspace_key = self.workspace_id.as_uuid().to_string();
        let mut ui_layouts = canonical_workspace_map(self.store.get(UI_LAYOUT_KEY)?);
        let mut layout = ui_layouts
            .get(&workspace_key)
            .cloned()
            .and_then(stored_layout_from_value)
            .filter(|layout| layout.schema_version == APP_CONFIG_VERSION)
            .unwrap_or_else(default_layout);
        if normalize_layout(&mut layout).is_err() {
            layout = default_layout();
        }
        ui_layouts.insert(
            workspace_key.clone(),
            serde_json::to_value(&layout).map_err(|_| AppConfigServiceError::unavailable())?,
        );

        let mut recent_by_workspace = canonical_workspace_map(self.store.get(RECENT_FILES_KEY)?);
        let mut recent = recent_by_workspace
            .get(&workspace_key)
            .cloned()
            .and_then(|value| serde_json::from_value::<Vec<RecentMarkdownFileDto>>(value).ok())
            .unwrap_or_default();
        if normalize_recent(&mut recent).is_err() {
            recent.clear();
        }
        recent_by_workspace.insert(
            workspace_key.clone(),
            serde_json::to_value(&recent).map_err(|_| AppConfigServiceError::unavailable())?,
        );

        let mut sort_by_workspace = canonical_workspace_map(self.store.get(FILE_TREE_SORT_KEY)?);
        let sort = sort_by_workspace
            .get(&workspace_key)
            .cloned()
            .and_then(|value| serde_json::from_value::<StoredFileTreeSortDto>(value).ok())
            .unwrap_or_else(default_sort);
        sort_by_workspace.insert(
            workspace_key,
            serde_json::to_value(&sort).map_err(|_| AppConfigServiceError::unavailable())?,
        );

        let pandoc_path = self
            .store
            .get(PANDOC_PATH_KEY)?
            .and_then(|value| serde_json::from_value::<Nullable<String>>(value).ok())
            .and_then(|path| normalize_pandoc_path(path).ok())
            .unwrap_or_else(Nullable::null);

        Ok(LoadedState {
            ui_layouts,
            recent_by_workspace,
            sort_by_workspace,
            layout,
            recent,
            sort,
            pandoc_path,
            workspace_key: self.workspace_id.as_uuid().to_string(),
        })
    }

    fn commit(&self, changes: &BTreeMap<&'static str, Value>) -> Result<(), AppConfigServiceError> {
        let mut previous = BTreeMap::new();
        for key in changes.keys() {
            previous.insert(*key, self.store.get(key)?);
        }
        for (key, value) in changes {
            if let Err(error) = self.store.set(key, value.clone()) {
                return self.rollback_or_error(&previous, error);
            }
        }
        if let Err(error) = self.store.save() {
            if error.kind() == SettingsStoreErrorKind::PublishUncertain {
                self.coordinator.require_recovery();
                return Err(AppConfigServiceError::recovery_required());
            }
            return self.rollback_or_error(&previous, error);
        }
        Ok(())
    }

    fn rollback_or_error(
        &self,
        previous: &BTreeMap<&'static str, Option<Value>>,
        error: SettingsStoreError,
    ) -> Result<(), AppConfigServiceError> {
        let mut rollback_failed = false;
        for (key, value) in previous {
            let result = match value {
                Some(value) => self.store.set(key, value.clone()),
                None => self.store.delete(key),
            };
            rollback_failed |= result.is_err();
        }
        if rollback_failed {
            self.coordinator.require_recovery();
            Err(AppConfigServiceError::recovery_required())
        } else {
            Err(error.into())
        }
    }
}

fn preflight_operations(
    operations: &[AppConfigStateOperationDto],
) -> Result<(), AppConfigServiceError> {
    validate_aggregate_draft_contents(
        operations
            .iter()
            .filter_map(|operation| match operation {
                AppConfigStateOperationDto::PatchUiLayout { patch, .. } => {
                    patch.draft_tabs.as_ref()
                }
                _ => None,
            })
            .flatten()
            .map(|draft| &draft.content),
    )
    .map_err(|_| AppConfigServiceError::invalid())?;
    for operation in operations {
        match operation {
            AppConfigStateOperationDto::PatchUiLayout { patch, .. } => {
                validate_layout_patch(patch).map_err(|_| AppConfigServiceError::invalid())?;
            }
            AppConfigStateOperationDto::RememberRecentFile { file } => {
                normalize_recent_file(file.clone())
                    .map_err(|_| AppConfigServiceError::invalid())?;
            }
            AppConfigStateOperationDto::RemoveRecentFile { path } => {
                if !markdown_path(path) {
                    return Err(AppConfigServiceError::invalid());
                }
            }
            AppConfigStateOperationDto::SetPandocPath { path } => {
                normalize_pandoc_path(path.clone())
                    .map_err(|_| AppConfigServiceError::invalid())?;
            }
            AppConfigStateOperationDto::ClearRecentFiles
            | AppConfigStateOperationDto::SetFileTreeSort { .. } => {}
        }
    }
    Ok(())
}

struct LoadedState {
    ui_layouts: Map<String, Value>,
    recent_by_workspace: Map<String, Value>,
    sort_by_workspace: Map<String, Value>,
    layout: StoredWorkspaceLayoutDto,
    recent: Vec<RecentMarkdownFileDto>,
    sort: StoredFileTreeSortDto,
    pandoc_path: Nullable<String>,
    workspace_key: String,
}

impl LoadedState {
    fn apply(
        &mut self,
        operations: Vec<AppConfigStateOperationDto>,
    ) -> Result<(), AppConfigServiceError> {
        for operation in operations {
            match operation {
                AppConfigStateOperationDto::PatchUiLayout {
                    window_label,
                    patch,
                } => {
                    let state = self
                        .layout
                        .window_states
                        .entry(window_label.as_str().to_string())
                        .or_insert_with(default_window_state);
                    if let Some(value) = patch.active_draft_id {
                        state.active_draft_id = value;
                    }
                    if let Some(value) = patch.draft_tabs {
                        state.draft_tabs = value;
                    }
                    if let Some(value) = patch.file_tree_assets_visible {
                        state.file_tree_assets_visible = value;
                    }
                    if let Some(value) = patch.file_path {
                        state.file_path = value;
                    }
                    if let Some(value) = patch.file_tree_open {
                        state.file_tree_open = value;
                    }
                    if let Some(value) = patch.folder_name {
                        state.folder_name = value;
                    }
                    if let Some(value) = patch.folder_path {
                        state.folder_path = value;
                    }
                    if let Some(value) = patch.open_file_paths {
                        state.open_file_paths = value;
                    }
                    if let Some(value) = patch.side_by_side_group {
                        state.side_by_side_group = value;
                    }
                    if let Some(value) = patch.open_windows {
                        self.layout.open_windows = value;
                    }
                    normalize_layout(&mut self.layout)
                        .map_err(|_| AppConfigServiceError::invalid())?;
                }
                AppConfigStateOperationDto::RememberRecentFile { file } => {
                    remember_recent_file(&mut self.recent, file)
                        .map_err(|_| AppConfigServiceError::invalid())?;
                }
                AppConfigStateOperationDto::RemoveRecentFile { path } => {
                    if !markdown_path(&path) {
                        return Err(AppConfigServiceError::invalid());
                    }
                    self.recent.retain(|file| file.path != path);
                }
                AppConfigStateOperationDto::ClearRecentFiles => self.recent.clear(),
                AppConfigStateOperationDto::SetFileTreeSort { sort } => self.sort = sort,
                AppConfigStateOperationDto::SetPandocPath { path } => {
                    self.pandoc_path = normalize_pandoc_path(path)
                        .map_err(|_| AppConfigServiceError::invalid())?;
                }
            }
        }
        normalize_layout(&mut self.layout).map_err(|_| AppConfigServiceError::invalid())?;
        normalize_recent(&mut self.recent).map_err(|_| AppConfigServiceError::invalid())?;
        Ok(())
    }

    fn refresh_maps(&mut self) -> Result<(), AppConfigServiceError> {
        self.ui_layouts.insert(
            self.workspace_key.clone(),
            serde_json::to_value(&self.layout).map_err(|_| AppConfigServiceError::unavailable())?,
        );
        self.recent_by_workspace.insert(
            self.workspace_key.clone(),
            serde_json::to_value(&self.recent).map_err(|_| AppConfigServiceError::unavailable())?,
        );
        self.sort_by_workspace.insert(
            self.workspace_key.clone(),
            serde_json::to_value(&self.sort).map_err(|_| AppConfigServiceError::unavailable())?,
        );
        Ok(())
    }

    fn values(&mut self) -> Result<(Value, Value, Value, Value), AppConfigServiceError> {
        self.refresh_maps()?;
        Ok((
            Value::Object(self.ui_layouts.clone()),
            Value::Object(self.recent_by_workspace.clone()),
            Value::Object(self.sort_by_workspace.clone()),
            serde_json::to_value(&self.pandoc_path)
                .map_err(|_| AppConfigServiceError::unavailable())?,
        ))
    }

    fn storage_values(&mut self) -> Result<BTreeMap<&'static str, Value>, AppConfigServiceError> {
        let (ui_layout, recent, sort, pandoc) = self.values()?;
        Ok(BTreeMap::from([
            (APP_CONFIG_VERSION_KEY, Value::from(APP_CONFIG_VERSION)),
            (UI_LAYOUT_KEY, ui_layout),
            (RECENT_FILES_KEY, recent),
            (FILE_TREE_SORT_KEY, sort),
            (PANDOC_PATH_KEY, pandoc),
        ]))
    }

    fn snapshot(
        mut self,
        settings: crate::contract::SettingsSnapshotDto,
        workspace_id: WorkspaceId,
        workspace_generation: WorkspaceGeneration,
    ) -> Result<AppConfigSnapshotDto, AppConfigServiceError> {
        let (ui_layout, recent, sort, pandoc) = self.values()?;
        let revision = local_revision(&ui_layout, &recent, &sort, &pandoc)
            .map_err(|_| AppConfigServiceError::unavailable())?;
        Ok(AppConfigSnapshotDto {
            app_config_version: APP_CONFIG_VERSION,
            settings,
            workspace: AppConfigWorkspaceDto {
                id: workspace_id,
                generation: workspace_generation,
            },
            local_state: local_state(
                revision,
                self.layout,
                self.recent,
                self.sort,
                self.pandoc_path,
            ),
        })
    }
}

fn normalize_recent(recent: &mut Vec<RecentMarkdownFileDto>) -> Result<(), ()> {
    let mut normalized = Vec::new();
    for file in recent.drain(..) {
        let file = normalize_recent_file(file)?;
        if normalized
            .iter()
            .any(|current: &RecentMarkdownFileDto| current.path == file.path)
        {
            continue;
        }
        normalized.push(file);
        if normalized.len() == 10 {
            break;
        }
    }
    *recent = normalized;
    Ok(())
}

fn canonical_workspace_map(value: Option<Value>) -> Map<String, Value> {
    value
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|(key, _)| Uuid::parse_str(key).is_ok_and(|uuid| uuid.to_string() == *key))
        .collect()
}

fn stored_layout_from_value(value: Value) -> Option<StoredWorkspaceLayoutDto> {
    if !raw_layout_labels_are_canonical(&value) {
        return None;
    }
    serde_json::from_value(value).ok()
}

fn raw_layout_labels_are_canonical(value: &Value) -> bool {
    let Some(layout) = value.as_object() else {
        return true;
    };
    let window_state_labels_are_canonical = layout
        .get("windowStates")
        .and_then(Value::as_object)
        .is_none_or(|states| {
            states
                .keys()
                .all(|label| raw_window_label_is_canonical(label))
        });
    let open_window_labels_are_canonical = layout
        .get("openWindows")
        .and_then(Value::as_array)
        .is_none_or(|windows| {
            windows.iter().all(|window| {
                window
                    .as_object()
                    .and_then(|window| window.get("label"))
                    .and_then(Value::as_str)
                    .is_none_or(raw_window_label_is_canonical)
            })
        });
    window_state_labels_are_canonical && open_window_labels_are_canonical
}

fn raw_window_label_is_canonical(label: &str) -> bool {
    WindowLabel::parse(label).is_ok_and(|normalized| normalized.as_str() == label)
}

fn map_settings_error(error: SettingsServiceError) -> AppConfigServiceError {
    match error.kind() {
        SettingsServiceErrorKind::RecoveryRequired => AppConfigServiceError::recovery_required(),
        SettingsServiceErrorKind::InvalidField
        | SettingsServiceErrorKind::RevisionConflict
        | SettingsServiceErrorKind::Unavailable
        | SettingsServiceErrorKind::ReconcileFailed => AppConfigServiceError::unavailable(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppConfigServiceErrorKind {
    InvalidAppConfigState,
    StaleWorkspaceGeneration,
    Unavailable,
    RecoveryRequired,
}

#[derive(Clone, Eq, PartialEq)]
pub struct AppConfigServiceError {
    kind: AppConfigServiceErrorKind,
}

impl AppConfigServiceError {
    const fn invalid() -> Self {
        Self {
            kind: AppConfigServiceErrorKind::InvalidAppConfigState,
        }
    }

    const fn stale_generation() -> Self {
        Self {
            kind: AppConfigServiceErrorKind::StaleWorkspaceGeneration,
        }
    }

    const fn unavailable() -> Self {
        Self {
            kind: AppConfigServiceErrorKind::Unavailable,
        }
    }

    const fn recovery_required() -> Self {
        Self {
            kind: AppConfigServiceErrorKind::RecoveryRequired,
        }
    }

    pub const fn kind(&self) -> AppConfigServiceErrorKind {
        self.kind
    }
}

impl From<SettingsStoreError> for AppConfigServiceError {
    fn from(error: SettingsStoreError) -> Self {
        if error.kind() == SettingsStoreErrorKind::PublishUncertain {
            Self::recovery_required()
        } else {
            Self::unavailable()
        }
    }
}

impl fmt::Debug for AppConfigServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppConfigServiceError")
            .field("kind", &self.kind)
            .finish()
    }
}

impl fmt::Display for AppConfigServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("app configuration state is unavailable")
    }
}

impl std::error::Error for AppConfigServiceError {}
