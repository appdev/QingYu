use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions as CapOpenOptions};
use serde::Serialize;
use tauri::Manager;

use super::{
    catalog::{CatalogActivationSource, ThemeCatalog, ACTIVATION_LEASE_PARENT_NAME},
    resources::{
        validate_theme_directory_from_retained, ValidatedThemeDirectory, ValidatedThemeFile,
        MAX_PACKAGE_BYTES, MAX_PACKAGE_ENTRIES,
    },
    ThemeCatalogSnapshot, ThemeDescriptor, ThemeError, ThemeErrorCode,
};

const ACTIVATION_LEASE_PREFIX: &str = ".qingyu-theme-lease-";
const ACTIVATION_LEASE_SUFFIX: &str = ".dir";
const ACTIVATION_QUARANTINE_PREFIX: &str = ".qingyu-theme-quarantine-";
const ACTIVATION_QUARANTINE_SUFFIX: &str = ".dir";
static NEXT_QUARANTINE: AtomicU64 = AtomicU64::new(0);
const MAX_CLEANUP_VALIDATION_DEPTH: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActivationLeaseHookPoint {
    AfterCopy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActivationLeaseCleanupHookPoint {
    AfterIdentityCheck,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActivationPrepareHookPoint {
    AfterCatalogValidation,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ThemeActivationSource {
    Inline { css: String },
    Stylesheet { path: String },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThemeActivationPayload {
    pub(crate) token: String,
    pub(crate) id: String,
    pub(crate) fingerprint: String,
    pub(crate) source: ThemeActivationSource,
}

#[derive(Clone)]
struct PendingActivation {
    window_label: String,
    theme_id: String,
    fingerprint: String,
    lease: Option<ActivationLease>,
}

impl PendingActivation {
    fn belongs_to_theme(&self, id: &str) -> bool {
        self.theme_id == id && !self.fingerprint.is_empty()
    }
}

#[derive(Clone)]
struct ActiveActivation {
    theme_id: String,
    fingerprint: String,
    lease: Option<ActivationLease>,
}

impl ActiveActivation {
    fn belongs_to_theme(&self, id: &str) -> bool {
        self.theme_id == id && !self.fingerprint.is_empty()
    }
}

#[derive(Clone)]
struct ActivationLease {
    source_root: PathBuf,
    fingerprint: String,
    descriptor: ThemeDescriptor,
    files: Vec<ValidatedThemeFile>,
    root: PathBuf,
    parent: Arc<Dir>,
    name: String,
    identity: (u64, u64),
    quarantined: bool,
    content_complete: bool,
}

impl ActivationLease {
    fn relocated_to_quarantine(&self, parent_path: &Path, name: String) -> Self {
        let mut relocated = self.clone();
        relocated.root = parent_path.join(&name);
        relocated.name = name;
        relocated.quarantined = true;
        relocated.content_complete = false;
        relocated
    }
}

#[derive(Clone, Default)]
struct ActivationRegistry {
    next_token: u64,
    next_lease: u64,
    stale_leases_cleaned: bool,
    permission_operation: bool,
    deleting: BTreeSet<String>,
    theme_generations: BTreeMap<String, u64>,
    pending: BTreeMap<String, PendingActivation>,
    active: BTreeMap<String, ActiveActivation>,
    retired: BTreeMap<PathBuf, ActivationLease>,
    orphaned_allowed: BTreeMap<PathBuf, ActivationLease>,
}

impl ActivationRegistry {
    fn referenced_leases(&self) -> BTreeMap<PathBuf, ActivationLease> {
        self.pending
            .values()
            .filter_map(|pending| pending.lease.clone())
            .chain(
                self.active
                    .values()
                    .filter_map(|active| active.lease.clone()),
            )
            .map(|lease| (lease.root.clone(), lease))
            .collect()
    }

    fn next_token(&mut self) -> String {
        self.next_token = self.next_token.wrapping_add(1);
        format!("theme-{}-{}", std::process::id(), self.next_token)
    }

    fn next_lease_name(&mut self) -> String {
        self.next_lease = self.next_lease.wrapping_add(1);
        format!(
            "{ACTIVATION_LEASE_PREFIX}{}-{}{ACTIVATION_LEASE_SUFFIX}",
            std::process::id(),
            self.next_lease
        )
    }

    fn shared_lease(&self, source_root: &Path, fingerprint: &str) -> Option<ActivationLease> {
        self.pending
            .values()
            .filter_map(|pending| pending.lease.as_ref())
            .chain(
                self.active
                    .values()
                    .filter_map(|active| active.lease.as_ref()),
            )
            .find(|lease| lease.source_root == source_root && lease.fingerprint == fingerprint)
            .cloned()
    }

    fn remove_references_to_roots(&mut self, roots: &BTreeSet<PathBuf>) {
        self.pending.retain(|_, pending| {
            pending
                .lease
                .as_ref()
                .is_none_or(|lease| !roots.contains(&lease.root))
        });
        self.active.retain(|_, active| {
            active
                .lease
                .as_ref()
                .is_none_or(|lease| !roots.contains(&lease.root))
        });
    }
}

pub(crate) struct ThemeActivationState {
    registry: Mutex<ActivationRegistry>,
    catalog_hints: Mutex<BTreeMap<(PathBuf, String, String), ThemeDescriptor>>,
}

impl Default for ThemeActivationState {
    fn default() -> Self {
        Self {
            registry: Mutex::new(ActivationRegistry::default()),
            catalog_hints: Mutex::new(BTreeMap::new()),
        }
    }
}

impl ThemeActivationState {
    pub(crate) fn remember_catalog_snapshot(
        &self,
        catalog: &ThemeCatalog,
        snapshot: &ThemeCatalogSnapshot,
    ) -> Result<(), ThemeError> {
        let mut hints = self
            .catalog_hints
            .lock()
            .map_err(|_| activation_state_error())?;
        for descriptor in &snapshot.themes {
            hints.retain(|(root, id, fingerprint), _| {
                root != catalog.root_path()
                    || id != &descriptor.id
                    || fingerprint == &descriptor.fingerprint
            });
            hints.insert(
                (
                    catalog.root_path().to_path_buf(),
                    descriptor.id.clone(),
                    descriptor.fingerprint.clone(),
                ),
                descriptor.clone(),
            );
        }
        Ok(())
    }

    pub(crate) fn prepare_with_permissions(
        &self,
        catalog: &ThemeCatalog,
        window_label: &str,
        id: &str,
        expected_fingerprint: &str,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
    ) -> Result<ThemeActivationPayload, ThemeError> {
        self.prepare_with_permissions_and_hooks(
            catalog,
            window_label,
            id,
            expected_fingerprint,
            allow_directory,
            forbid_directory,
            &mut |_, _| Ok(()),
            &mut |_| Ok(()),
        )
    }

    #[cfg(test)]
    fn prepare_with_permissions_and_lease_hook(
        &self,
        catalog: &ThemeCatalog,
        window_label: &str,
        id: &str,
        expected_fingerprint: &str,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        lease_hook: &mut dyn FnMut(ActivationLeaseHookPoint, &Path) -> Result<(), ThemeError>,
    ) -> Result<ThemeActivationPayload, ThemeError> {
        self.prepare_with_permissions_and_hooks(
            catalog,
            window_label,
            id,
            expected_fingerprint,
            allow_directory,
            forbid_directory,
            lease_hook,
            &mut |_| Ok(()),
        )
    }

    #[cfg(test)]
    fn prepare_with_permissions_and_prepare_hook(
        &self,
        catalog: &ThemeCatalog,
        window_label: &str,
        id: &str,
        expected_fingerprint: &str,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        prepare_hook: &mut dyn FnMut(ActivationPrepareHookPoint) -> Result<(), ThemeError>,
    ) -> Result<ThemeActivationPayload, ThemeError> {
        self.prepare_with_permissions_and_hooks(
            catalog,
            window_label,
            id,
            expected_fingerprint,
            allow_directory,
            forbid_directory,
            &mut |_, _| Ok(()),
            prepare_hook,
        )
    }

    fn prepare_with_permissions_and_hooks(
        &self,
        catalog: &ThemeCatalog,
        window_label: &str,
        id: &str,
        expected_fingerprint: &str,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        lease_hook: &mut dyn FnMut(ActivationLeaseHookPoint, &Path) -> Result<(), ThemeError>,
        prepare_hook: &mut dyn FnMut(ActivationPrepareHookPoint) -> Result<(), ThemeError>,
    ) -> Result<ThemeActivationPayload, ThemeError> {
        let generation = {
            let registry = self.registry.lock().map_err(|_| activation_state_error())?;
            if registry.deleting.contains(id) {
                return Err(fingerprint_mismatch());
            }
            registry.theme_generations.get(id).copied().unwrap_or(0)
        };
        let hint = self
            .catalog_hints
            .lock()
            .map_err(|_| activation_state_error())?
            .get(&(
                catalog.root_path().to_path_buf(),
                id.to_string(),
                expected_fingerprint.to_string(),
            ))
            .cloned();
        let prepared =
            catalog.prepare_activation_with_hint(id, expected_fingerprint, hint.as_ref())?;
        prepare_hook(ActivationPrepareHookPoint::AfterCatalogValidation)?;
        let window_label = window_label.to_string();
        let descriptor = prepared.descriptor;
        let mut current = self.begin_permission_operation(Some((id, generation)))?;
        let outcome = (|| {
            cleanup_orphaned_allowed(&mut current, forbid_directory)?;
            cleanup_retired_leases(&mut current);
            let (lease, source) = match prepared.source {
                CatalogActivationSource::InlineCss(css) => {
                    (None, ThemeActivationSource::Inline { css })
                }
                CatalogActivationSource::ResourceDirectory(theme) => {
                    if catalog.activation_lease_parent_path().to_str().is_none() {
                        return Err(ThemeError::new(
                            ThemeErrorCode::UnsafePath,
                            "Theme activation lease paths must use valid UTF-8.",
                        ));
                    }
                    if !current.stale_leases_cleaned {
                        cleanup_stale_activation_leases(catalog)?;
                        current.stale_leases_cleaned = true;
                    }
                    let lease = if let Some(existing) =
                        current.shared_lease(&theme.root, &descriptor.fingerprint)
                    {
                        revalidate_activation_lease(&existing, &theme)?;
                        existing
                    } else {
                        materialize_activation_lease(catalog, &theme, &mut current, lease_hook)?
                    };
                    let stylesheet_path = lease.root.join("theme.css");
                    let path = stylesheet_path.to_str().ok_or_else(|| {
                        ThemeError::new(
                            ThemeErrorCode::UnsafePath,
                            "The validated activation lease path is not valid UTF-8.",
                        )
                    })?;
                    (
                        Some(lease),
                        ThemeActivationSource::Stylesheet {
                            path: path.to_string(),
                        },
                    )
                }
            };
            let mut next = current.clone();
            next.pending
                .retain(|_, pending| pending.window_label != window_label);
            let token = next.next_token();
            next.pending.insert(
                token.clone(),
                PendingActivation {
                    window_label,
                    theme_id: descriptor.id.clone(),
                    fingerprint: descriptor.fingerprint.clone(),
                    lease,
                },
            );
            self.apply_transition(
                &mut current,
                next,
                token.clone(),
                allow_directory,
                forbid_directory,
                &mut remove_activation_lease,
            )?;

            Ok(ThemeActivationPayload {
                token,
                id: descriptor.id,
                fingerprint: descriptor.fingerprint,
                source,
            })
        })();
        self.finish_permission_operation(current)?;
        outcome
    }

    pub(crate) fn commit_with_permissions(
        &self,
        window_label: &str,
        token: &str,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
    ) -> Result<(), ThemeError> {
        self.transition(
            |registry| {
                let pending = registry
                    .pending
                    .get(token)
                    .cloned()
                    .ok_or_else(stale_token)?;
                if pending.window_label != window_label {
                    return Err(stale_token());
                }
                registry.pending.remove(token);
                registry.active.remove(window_label);
                registry.active.insert(
                    window_label.to_string(),
                    ActiveActivation {
                        theme_id: pending.theme_id,
                        fingerprint: pending.fingerprint,
                        lease: pending.lease,
                    },
                );
                Ok(())
            },
            allow_directory,
            forbid_directory,
        )
    }

    pub(crate) fn cancel_with_permissions(
        &self,
        window_label: &str,
        token: &str,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
    ) -> Result<(), ThemeError> {
        self.transition(
            |registry| {
                let pending = registry.pending.get(token).ok_or_else(stale_token)?;
                if pending.window_label != window_label {
                    return Err(stale_token());
                }
                registry.pending.remove(token);
                Ok(())
            },
            allow_directory,
            forbid_directory,
        )
    }

    pub(crate) fn release_window_with_permissions(
        &self,
        window_label: &str,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
    ) -> Result<(), ThemeError> {
        self.transition(
            |registry| {
                registry
                    .pending
                    .retain(|_, pending| pending.window_label != window_label);
                registry.active.remove(window_label);
                Ok(())
            },
            allow_directory,
            forbid_directory,
        )
    }

    fn release_destroyed_window_with_permissions(
        &self,
        window_label: &str,
        _allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
    ) -> Result<(), ThemeError> {
        let mut current = self.begin_permission_operation(None)?;
        let outcome = (|| {
            cleanup_orphaned_allowed(&mut current, forbid_directory)?;
            cleanup_retired_leases(&mut current);
            let before = current.referenced_leases();
            let mut next = current.clone();
            next.pending
                .retain(|_, pending| pending.window_label != window_label);
            next.active.remove(window_label);
            let after = next.referenced_leases();
            let removals = before
                .iter()
                .filter(|(root, _)| !after.contains_key(*root))
                .map(|(_, lease)| lease.clone())
                .collect::<Vec<_>>();
            let mut first_error = None;
            for lease in removals {
                if let Err(error) = forbid_directory(&lease.root) {
                    next.orphaned_allowed.insert(lease.root.clone(), lease);
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    continue;
                }
                if let Err(retired) = remove_activation_lease(&lease) {
                    next.retired.insert(retired.root.clone(), retired);
                    if first_error.is_none() {
                        first_error = Some(lease_cleanup_error());
                    }
                }
            }
            current = next;
            first_error.map_or(Ok(()), Err)
        })();
        self.finish_permission_operation(current)?;
        outcome
    }

    #[cfg(test)]
    fn release_window_with_permissions_and_remove_hook(
        &self,
        window_label: &str,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        remove_hook: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
    ) -> Result<(), ThemeError> {
        self.transition_with_removal(
            |registry| {
                registry
                    .pending
                    .retain(|_, pending| pending.window_label != window_label);
                registry.active.remove(window_label);
                Ok(())
            },
            allow_directory,
            forbid_directory,
            &mut |lease| {
                if remove_hook(&lease.root).is_err() {
                    return Err(lease.clone());
                }
                remove_activation_lease(lease)
            },
        )
    }

    #[cfg(test)]
    fn release_window_with_permissions_and_cleanup_hook(
        &self,
        window_label: &str,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        cleanup_hook: &mut dyn FnMut(
            ActivationLeaseCleanupHookPoint,
            &Path,
        ) -> Result<(), ThemeError>,
    ) -> Result<(), ThemeError> {
        self.transition_with_removal(
            |registry| {
                registry
                    .pending
                    .retain(|_, pending| pending.window_label != window_label);
                registry.active.remove(window_label);
                Ok(())
            },
            allow_directory,
            forbid_directory,
            &mut |lease| remove_activation_lease_with_hook(lease, cleanup_hook),
        )
    }

    #[cfg(test)]
    fn release_window_with_permissions_and_partial_remove_hook(
        &self,
        window_label: &str,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        removal_hook: &mut dyn FnMut(&Path) -> io::Result<()>,
    ) -> Result<(), ThemeError> {
        self.transition_with_removal(
            |registry| {
                registry
                    .pending
                    .retain(|_, pending| pending.window_label != window_label);
                registry.active.remove(window_label);
                Ok(())
            },
            allow_directory,
            forbid_directory,
            &mut |lease| {
                remove_activation_lease_with_hooks(lease, &mut |_, _| Ok(()), Some(removal_hook))
            },
        )
    }

    fn revoke_theme_for_delete_with_permissions(
        &self,
        current: &mut ActivationRegistry,
        theme_id: &str,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
    ) -> Result<(), ThemeError> {
        cleanup_orphaned_allowed(current, forbid_directory)?;
        cleanup_retired_leases(current);
        let mut next = current.clone();
        next.pending
            .retain(|_, pending| !pending.belongs_to_theme(theme_id));
        next.active
            .retain(|_, active| !active.belongs_to_theme(theme_id));
        self.apply_transition(
            current,
            next,
            (),
            allow_directory,
            forbid_directory,
            &mut remove_activation_lease,
        )
    }

    fn catalog_hint(
        &self,
        catalog: &ThemeCatalog,
        id: &str,
        expected_fingerprint: &str,
    ) -> Result<Option<ThemeDescriptor>, ThemeError> {
        Ok(self
            .catalog_hints
            .lock()
            .map_err(|_| activation_state_error())?
            .get(&(
                catalog.root_path().to_path_buf(),
                id.to_string(),
                expected_fingerprint.to_string(),
            ))
            .cloned())
    }

    fn begin_delete_operation(&self, id: &str) -> Result<ActivationRegistry, ThemeError> {
        let mut registry = self.registry.lock().map_err(|_| activation_state_error())?;
        if registry.permission_operation || registry.deleting.contains(id) {
            return Err(activation_state_error());
        }
        let generation = registry
            .theme_generations
            .entry(id.to_string())
            .or_default();
        *generation = generation.wrapping_add(1);
        registry.deleting.insert(id.to_string());
        registry.permission_operation = true;
        let mut working = registry.clone();
        working.permission_operation = false;
        Ok(working)
    }

    fn finish_delete_operation(
        &self,
        mut working: ActivationRegistry,
        id: &str,
    ) -> Result<(), ThemeError> {
        working.deleting.remove(id);
        self.finish_permission_operation(working)
    }

    fn transition<T>(
        &self,
        change: impl FnOnce(&mut ActivationRegistry) -> Result<T, ThemeError>,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
    ) -> Result<T, ThemeError> {
        self.transition_with_removal(
            change,
            allow_directory,
            forbid_directory,
            &mut remove_activation_lease,
        )
    }

    fn transition_with_removal<T>(
        &self,
        change: impl FnOnce(&mut ActivationRegistry) -> Result<T, ThemeError>,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        remove_lease: &mut dyn FnMut(&ActivationLease) -> Result<(), ActivationLease>,
    ) -> Result<T, ThemeError> {
        let mut current = self.begin_permission_operation(None)?;
        let outcome = (|| {
            cleanup_orphaned_allowed(&mut current, forbid_directory)?;
            cleanup_retired_leases(&mut current);
            let mut next = current.clone();
            let result = change(&mut next)?;
            self.apply_transition(
                &mut current,
                next,
                result,
                allow_directory,
                forbid_directory,
                remove_lease,
            )
        })();
        self.finish_permission_operation(current)?;
        outcome
    }

    fn begin_permission_operation(
        &self,
        prepare_guard: Option<(&str, u64)>,
    ) -> Result<ActivationRegistry, ThemeError> {
        let mut registry = self.registry.lock().map_err(|_| activation_state_error())?;
        if registry.permission_operation {
            return Err(activation_state_error());
        }
        if let Some((id, generation)) = prepare_guard {
            if registry.deleting.contains(id)
                || registry.theme_generations.get(id).copied().unwrap_or(0) != generation
            {
                return Err(fingerprint_mismatch());
            }
        }
        registry.permission_operation = true;
        let mut working = registry.clone();
        working.permission_operation = false;
        Ok(working)
    }

    fn finish_permission_operation(
        &self,
        mut working: ActivationRegistry,
    ) -> Result<(), ThemeError> {
        let mut registry = self.registry.lock().map_err(|_| activation_state_error())?;
        if !registry.permission_operation {
            return Err(activation_state_error());
        }
        working.permission_operation = false;
        *registry = working;
        Ok(())
    }

    fn apply_transition<T>(
        &self,
        current: &mut ActivationRegistry,
        mut next: ActivationRegistry,
        result: T,
        allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
        remove_lease: &mut dyn FnMut(&ActivationLease) -> Result<(), ActivationLease>,
    ) -> Result<T, ThemeError> {
        let before = current.referenced_leases();
        let after = next.referenced_leases();
        let additions = after
            .iter()
            .filter(|(root, _)| !before.contains_key(*root))
            .map(|(_, lease)| lease.clone())
            .collect::<Vec<_>>();
        let removals = before
            .iter()
            .filter(|(root, _)| !after.contains_key(*root))
            .map(|(_, lease)| lease.clone())
            .collect::<Vec<_>>();
        let mut allowed = Vec::new();
        for lease in &additions {
            if let Err(error) = verify_activation_lease_path(lease) {
                rollback_new_leases(current, &additions, &allowed, forbid_directory);
                return Err(error);
            }
            if let Err(error) = allow_directory(&lease.root) {
                let mut possibly_allowed = allowed.clone();
                possibly_allowed.push(lease.clone());
                rollback_new_leases(current, &additions, &possibly_allowed, forbid_directory);
                return Err(error);
            }
            allowed.push(lease.clone());
        }
        let mut forbidden = Vec::new();
        for lease in &removals {
            if let Err(error) = forbid_directory(&lease.root) {
                rollback_new_leases(current, &additions, &allowed, forbid_directory);
                if !forbidden.is_empty() {
                    let roots = forbidden
                        .iter()
                        .map(|lease: &ActivationLease| lease.root.clone())
                        .collect::<BTreeSet<_>>();
                    current.remove_references_to_roots(&roots);
                    for forbidden_lease in forbidden {
                        retire_or_remove(current, forbidden_lease);
                    }
                }
                return Err(error);
            }
            forbidden.push(lease.clone());
        }
        *current = next.clone();
        let mut cleanup_failed = false;
        for lease in removals {
            if let Err(retired) = remove_lease(&lease) {
                cleanup_failed = true;
                next.retired.insert(retired.root.clone(), retired);
            }
        }
        *current = next;
        if cleanup_failed {
            return Err(lease_cleanup_error());
        }
        Ok(result)
    }

    #[cfg(test)]
    fn pending_count(&self) -> usize {
        self.registry.lock().unwrap().pending.len()
    }

    #[cfg(test)]
    fn active_root(&self, window_label: &str) -> Option<PathBuf> {
        self.registry
            .lock()
            .unwrap()
            .active
            .get(window_label)
            .and_then(|active| active.lease.as_ref().map(|lease| lease.root.clone()))
    }

    #[cfg(test)]
    fn retired_count(&self) -> usize {
        self.registry.lock().unwrap().retired.len()
    }

    #[cfg(test)]
    fn orphaned_allowed_count(&self) -> usize {
        self.registry.lock().unwrap().orphaned_allowed.len()
    }
}

fn cleanup_retired_leases(registry: &mut ActivationRegistry) {
    let retired = registry.retired.values().cloned().collect::<Vec<_>>();
    for lease in retired {
        registry.retired.remove(&lease.root);
        if let Err(still_retired) = remove_activation_lease(&lease) {
            registry
                .retired
                .insert(still_retired.root.clone(), still_retired);
        }
    }
}

fn cleanup_orphaned_allowed(
    registry: &mut ActivationRegistry,
    forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
) -> Result<(), ThemeError> {
    let orphaned = registry
        .orphaned_allowed
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for lease in orphaned {
        forbid_directory(&lease.root)?;
        registry.orphaned_allowed.remove(&lease.root);
        if let Err(retired) = remove_activation_lease(&lease) {
            registry.retired.insert(retired.root.clone(), retired);
        }
    }
    Ok(())
}

fn rollback_new_leases(
    registry: &mut ActivationRegistry,
    additions: &[ActivationLease],
    allowed: &[ActivationLease],
    forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
) {
    let allowed_roots = allowed
        .iter()
        .map(|lease| lease.root.clone())
        .collect::<BTreeSet<_>>();
    for lease in additions {
        if allowed_roots.contains(&lease.root) && forbid_directory(&lease.root).is_err() {
            registry
                .orphaned_allowed
                .insert(lease.root.clone(), lease.clone());
            continue;
        }
        retire_or_remove(registry, lease.clone());
    }
}

fn retire_or_remove(registry: &mut ActivationRegistry, lease: ActivationLease) {
    if let Err(retired) = remove_activation_lease(&lease) {
        registry.retired.insert(retired.root.clone(), retired);
    }
}

fn cleanup_stale_activation_leases(catalog: &ThemeCatalog) -> Result<(), ThemeError> {
    let (parent_path, parent) = open_or_create_activation_lease_parent(catalog)?;
    let entries = parent.entries().map_err(io_error)?;
    for entry in entries {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !is_owned_activation_lease_name(name) && !is_owned_activation_quarantine_name(name) {
            continue;
        }
        let metadata = parent.symlink_metadata(name).map_err(io_error)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        let _cleanup =
            remove_stale_activation_child(&parent, &parent_path, name, file_identity(&metadata));
    }
    Ok(())
}

fn materialize_activation_lease(
    catalog: &ThemeCatalog,
    source: &ValidatedThemeDirectory,
    registry: &mut ActivationRegistry,
    hook: &mut dyn FnMut(ActivationLeaseHookPoint, &Path) -> Result<(), ThemeError>,
) -> Result<ActivationLease, ThemeError> {
    let (parent_path, parent) = open_or_create_activation_lease_parent(catalog)?;
    let parent = Arc::new(parent);
    for _attempt in 0..1024 {
        let name = registry.next_lease_name();
        match parent.create_dir(&name) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error(error)),
        }
        let root = parent_path.join(&name);
        let identity = retained_child_identity(&parent, &name)?;
        let mut candidate = ActivationLease {
            source_root: source.root.clone(),
            fingerprint: source.descriptor.fingerprint.clone(),
            descriptor: source.descriptor.clone(),
            files: source.files.clone(),
            root: root.clone(),
            parent: parent.clone(),
            name: name.clone(),
            identity,
            quarantined: false,
            content_complete: false,
        };
        let result = materialize_activation_lease_at(&parent, &name, &root, source, hook);
        if let Err(error) = result {
            if let Err(retired) = remove_activation_lease(&candidate) {
                registry.retired.insert(retired.root.clone(), retired);
            }
            return Err(error);
        }
        candidate.content_complete = true;
        return Ok(candidate);
    }
    Err(ThemeError::new(
        ThemeErrorCode::Io,
        "A unique theme activation lease could not be reserved.",
    ))
}

fn materialize_activation_lease_at(
    parent: &Dir,
    name: &str,
    root: &Path,
    source: &ValidatedThemeDirectory,
    hook: &mut dyn FnMut(ActivationLeaseHookPoint, &Path) -> Result<(), ThemeError>,
) -> Result<(u64, u64), ThemeError> {
    let addressed = parent.symlink_metadata(name).map_err(io_error)?;
    if addressed.file_type().is_symlink() || !addressed.is_dir() {
        return Err(unsafe_lease_path());
    }
    let directory = parent
        .open_dir_nofollow(name)
        .map_err(|_| unsafe_lease_path())?;
    let retained = directory.dir_metadata().map_err(io_error)?;
    if !retained.is_dir() || file_identity(&addressed) != file_identity(&retained) {
        return Err(unsafe_lease_path());
    }
    for file in &source.files {
        let relative_path = Path::new(&file.relative_path);
        if relative_path.is_absolute()
            || relative_path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(unsafe_lease_path());
        }
        let file_name = relative_path.file_name().ok_or_else(unsafe_lease_path)?;
        let file_parent = open_or_create_lease_directory(
            &directory,
            relative_path.parent().unwrap_or_else(|| Path::new("")),
        )?;
        let mut options = CapOpenOptions::new();
        options
            .create_new(true)
            .write(true)
            .follow(FollowSymlinks::No);
        let mut destination = file_parent
            .open_with(file_name, &options)
            .map_err(|_| unsafe_lease_path())?;
        destination.write_all(&file.bytes).map_err(io_error)?;
        destination.sync_all().map_err(io_error)?;
    }
    hook(ActivationLeaseHookPoint::AfterCopy, root)?;
    let validated =
        validate_theme_directory_from_retained(root, &source.descriptor.file_name, &directory)
            .map_err(|_| lease_fingerprint_mismatch())?;
    if validated.descriptor != source.descriptor || validated.files != source.files {
        return Err(lease_fingerprint_mismatch());
    }
    let addressed_after = parent.symlink_metadata(name).map_err(io_error)?;
    let retained_after = directory.dir_metadata().map_err(io_error)?;
    if file_identity(&addressed_after) != file_identity(&retained)
        || file_identity(&retained_after) != file_identity(&retained)
    {
        return Err(unsafe_lease_path());
    }
    Ok(file_identity(&retained))
}

fn open_or_create_lease_directory(root: &Dir, relative: &Path) -> Result<Dir, ThemeError> {
    let mut current = root.try_clone().map_err(io_error)?;
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(unsafe_lease_path());
        };
        match current.create_dir(name) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_error(error)),
        }
        let addressed = current.symlink_metadata(name).map_err(io_error)?;
        if addressed.file_type().is_symlink() || !addressed.is_dir() {
            return Err(unsafe_lease_path());
        }
        let child = current
            .open_dir_nofollow(name)
            .map_err(|_| unsafe_lease_path())?;
        let retained = child.dir_metadata().map_err(io_error)?;
        if !retained.is_dir() || file_identity(&addressed) != file_identity(&retained) {
            return Err(unsafe_lease_path());
        }
        current = child;
    }
    Ok(current)
}

fn open_or_create_activation_lease_parent(
    catalog: &ThemeCatalog,
) -> Result<(PathBuf, Dir), ThemeError> {
    let parent_path = catalog.activation_lease_parent_path();
    let catalog_path = parent_path.parent().ok_or_else(unsafe_lease_path)?;
    let addressed_catalog =
        crate::storage_capability::ambient_symlink_metadata(catalog_path).map_err(io_error)?;
    if addressed_catalog.file_type().is_symlink() || !addressed_catalog.is_dir() {
        return Err(unsafe_lease_path());
    }
    let catalog_directory =
        Dir::open_ambient_dir(catalog_path, cap_std::ambient_authority()).map_err(io_error)?;
    let retained_catalog = catalog_directory.dir_metadata().map_err(io_error)?;
    if !retained_catalog.is_dir()
        || file_identity(&addressed_catalog) != file_identity(&retained_catalog)
    {
        return Err(unsafe_lease_path());
    }
    match catalog_directory.create_dir(ACTIVATION_LEASE_PARENT_NAME) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(io_error(error)),
    }
    let addressed_parent = catalog_directory
        .symlink_metadata(ACTIVATION_LEASE_PARENT_NAME)
        .map_err(io_error)?;
    if addressed_parent.file_type().is_symlink() || !addressed_parent.is_dir() {
        return Err(unsafe_lease_path());
    }
    let parent = catalog_directory
        .open_dir_nofollow(ACTIVATION_LEASE_PARENT_NAME)
        .map_err(|_| unsafe_lease_path())?;
    let retained_parent = parent.dir_metadata().map_err(io_error)?;
    if !retained_parent.is_dir()
        || file_identity(&addressed_parent) != file_identity(&retained_parent)
    {
        return Err(unsafe_lease_path());
    }
    Ok((parent_path, parent))
}

fn remove_activation_lease(lease: &ActivationLease) -> Result<(), ActivationLease> {
    remove_activation_lease_with_hooks(lease, &mut |_, _| Ok(()), None)
}

#[cfg(test)]
fn remove_activation_lease_with_hook(
    lease: &ActivationLease,
    hook: &mut dyn FnMut(ActivationLeaseCleanupHookPoint, &Path) -> Result<(), ThemeError>,
) -> Result<(), ActivationLease> {
    remove_activation_lease_with_hooks(lease, hook, None)
}

fn remove_activation_lease_with_hooks(
    lease: &ActivationLease,
    hook: &mut dyn FnMut(ActivationLeaseCleanupHookPoint, &Path) -> Result<(), ThemeError>,
    removal_hook: Option<&mut dyn FnMut(&Path) -> io::Result<()>>,
) -> Result<(), ActivationLease> {
    let parent_path = lease.root.parent().ok_or_else(|| lease.clone())?;
    if parent_path.file_name().and_then(|name| name.to_str()) != Some(ACTIVATION_LEASE_PARENT_NAME)
    {
        return Err(lease.clone());
    }
    let owned_name = if lease.quarantined {
        is_owned_activation_quarantine_name(&lease.name)
    } else {
        is_owned_activation_lease_name(&lease.name)
    };
    if lease.root.file_name().and_then(|name| name.to_str()) != Some(lease.name.as_str())
        || !owned_name
    {
        return Err(lease.clone());
    }
    let addressed = match lease.parent.symlink_metadata(&lease.name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(lease.clone()),
    };
    if addressed.file_type().is_symlink()
        || !addressed.is_dir()
        || file_identity(&addressed) != lease.identity
    {
        return Err(lease.clone());
    }
    let retained = lease
        .parent
        .open_dir_nofollow(&lease.name)
        .map_err(|_| lease.clone())?;
    let retained_metadata = retained.dir_metadata().map_err(|_| lease.clone())?;
    if !retained_metadata.is_dir() || file_identity(&retained_metadata) != lease.identity {
        return Err(lease.clone());
    }
    hook(
        ActivationLeaseCleanupHookPoint::AfterIdentityCheck,
        &parent_path.join(&lease.name),
    )
    .map_err(|_| lease.clone())?;
    // Windows directory handles intentionally omit FILE_SHARE_DELETE. Close
    // the verified source handle before the atomic quarantine rename, then
    // reopen and revalidate the renamed child before removing anything.
    drop(retained);

    let freshly_quarantined = !lease.quarantined;
    let quarantine_name = if lease.quarantined {
        lease.name.clone()
    } else {
        reserve_quarantine_name(&lease.parent, &lease.name).map_err(|_| lease.clone())?
    };
    let quarantine = lease
        .parent
        .open_dir_nofollow(&quarantine_name)
        .map_err(|_| lease.clone())?;
    let quarantine_metadata = quarantine.dir_metadata().map_err(|_| lease.clone())?;
    let addressed_quarantine = lease
        .parent
        .symlink_metadata(&quarantine_name)
        .map_err(|_| lease.clone())?;
    let identity_matches = !addressed_quarantine.file_type().is_symlink()
        && addressed_quarantine.is_dir()
        && quarantine_metadata.is_dir()
        && file_identity(&addressed_quarantine) == lease.identity
        && file_identity(&quarantine_metadata) == lease.identity;
    let descriptor_matches = !lease.descriptor.id.is_empty()
        && !lease.descriptor.fingerprint.is_empty()
        && activation_files_match(&quarantine, &lease.files, lease.content_complete);
    if !identity_matches || !descriptor_matches {
        drop(quarantine);
        if freshly_quarantined && lease.content_complete {
            if restore_quarantined_child(&lease.parent, &quarantine_name, &lease.name).is_err() {
                return Err(lease.relocated_to_quarantine(parent_path, quarantine_name));
            }
        } else if freshly_quarantined {
            return Err(lease.relocated_to_quarantine(parent_path, quarantine_name));
        }
        return Err(lease.clone());
    }
    let relocated = lease.relocated_to_quarantine(parent_path, quarantine_name.clone());
    let removal = if let Some(removal_hook) = removal_hook {
        super::activation_cleanup::remove_quarantined_directory_with_hook(
            &lease.parent,
            &quarantine_name,
            quarantine,
            removal_hook,
        )
    } else {
        super::activation_cleanup::remove_quarantined_directory(
            &lease.parent,
            &quarantine_name,
            quarantine,
        )
    };
    if removal.is_err() {
        return Err(relocated);
    }
    Ok(())
}

fn reserve_quarantine_name(parent: &Dir, name: &str) -> io::Result<String> {
    (0..1024)
        .find_map(|_| {
            let sequence = NEXT_QUARANTINE.fetch_add(1, Ordering::Relaxed);
            let candidate = format!(
                "{ACTIVATION_QUARANTINE_PREFIX}{}-{sequence}{ACTIVATION_QUARANTINE_SUFFIX}",
                std::process::id()
            );
            match crate::atomic_noreplace::rename_noreplace(
                parent,
                Path::new(name),
                parent,
                Path::new(&candidate),
            ) {
                Ok(()) => Some(Ok(candidate)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(error)),
            }
        })
        .transpose()?
        .ok_or_else(|| io::Error::other("a unique theme quarantine name could not be reserved"))
}

fn remove_stale_activation_child(
    parent: &Dir,
    _parent_path: &Path,
    name: &str,
    expected_identity: (u64, u64),
) -> Result<(), ThemeError> {
    let already_quarantined = is_owned_activation_quarantine_name(name);
    let quarantine_name = if already_quarantined {
        name.to_string()
    } else {
        reserve_quarantine_name(parent, name).map_err(|_| lease_cleanup_error())?
    };
    let quarantine = parent
        .open_dir_nofollow(&quarantine_name)
        .map_err(|_| lease_cleanup_error())?;
    let retained = quarantine.dir_metadata().map_err(io_error)?;
    let addressed = parent
        .symlink_metadata(&quarantine_name)
        .map_err(|_| lease_cleanup_error())?;
    if addressed.file_type().is_symlink()
        || !addressed.is_dir()
        || !retained.is_dir()
        || file_identity(&addressed) != expected_identity
        || file_identity(&retained) != expected_identity
    {
        drop(quarantine);
        if !already_quarantined {
            restore_quarantined_child(parent, &quarantine_name, name)?;
        }
        return Err(lease_cleanup_error());
    }
    super::activation_cleanup::remove_quarantined_directory(parent, &quarantine_name, quarantine)
        .map_err(|_| lease_cleanup_error())
}

fn retained_child_identity(parent: &Dir, name: &str) -> Result<(u64, u64), ThemeError> {
    let addressed = parent.symlink_metadata(name).map_err(io_error)?;
    if addressed.file_type().is_symlink() || !addressed.is_dir() {
        return Err(unsafe_lease_path());
    }
    let retained = parent
        .open_dir_nofollow(name)
        .map_err(|_| unsafe_lease_path())?;
    let metadata = retained.dir_metadata().map_err(io_error)?;
    if !metadata.is_dir() || file_identity(&addressed) != file_identity(&metadata) {
        return Err(unsafe_lease_path());
    }
    Ok(file_identity(&metadata))
}

#[derive(Default)]
struct CleanupValidationStats {
    visited_entries: usize,
    max_depth: usize,
}

fn activation_files_match(
    directory: &Dir,
    expected: &[ValidatedThemeFile],
    require_complete: bool,
) -> bool {
    activation_files_match_inner(directory, expected, require_complete).0
}

#[cfg(test)]
fn activation_files_match_with_stats(
    directory: &Dir,
    expected: &[ValidatedThemeFile],
) -> (bool, CleanupValidationStats) {
    activation_files_match_inner(directory, expected, true)
}

fn activation_files_match_inner(
    directory: &Dir,
    expected: &[ValidatedThemeFile],
    require_complete: bool,
) -> (bool, CleanupValidationStats) {
    let mut stats = CleanupValidationStats::default();
    if expected.len() > MAX_PACKAGE_ENTRIES
        || expected
            .iter()
            .try_fold(0_u64, |total, file| {
                total.checked_add(file.bytes.len() as u64)
            })
            .is_none_or(|total| total > MAX_PACKAGE_BYTES)
    {
        return (false, stats);
    }
    let expected_files = expected
        .iter()
        .map(|file| (file.relative_path.clone(), file.bytes.as_slice()))
        .collect::<BTreeMap<_, _>>();
    let mut expected_directories = BTreeSet::new();
    for file in expected {
        if Path::new(&file.relative_path).components().count() > MAX_CLEANUP_VALIDATION_DEPTH + 1 {
            return (false, stats);
        }
        let mut parent = Path::new(&file.relative_path).parent();
        while let Some(directory) = parent.filter(|directory| !directory.as_os_str().is_empty()) {
            let Some(key) = activation_relative_key(directory) else {
                return (false, stats);
            };
            expected_directories.insert(key);
            parent = directory.parent();
        }
    }
    let Ok(root) = directory.try_clone() else {
        return (false, stats);
    };
    let mut pending = vec![(root, PathBuf::new(), 0_usize)];
    let mut actual_files = BTreeSet::new();
    let mut actual_directories = BTreeSet::new();
    let mut actual_bytes = 0_u64;
    while let Some((current, relative, depth)) = pending.pop() {
        let Ok(entries) = current.entries() else {
            return (false, stats);
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return (false, stats);
            };
            stats.visited_entries += 1;
            if stats.visited_entries > MAX_PACKAGE_ENTRIES {
                return (false, stats);
            }
            let name = entry.file_name();
            let Ok(metadata) = current.symlink_metadata(&name) else {
                return (false, stats);
            };
            if metadata.file_type().is_symlink() {
                return (false, stats);
            }
            let child_relative = relative.join(&name);
            let Some(key) = activation_relative_key(&child_relative) else {
                return (false, stats);
            };
            if metadata.is_dir() {
                if !expected_directories.contains(&key) || depth + 1 > MAX_CLEANUP_VALIDATION_DEPTH
                {
                    return (false, stats);
                }
                actual_directories.insert(key);
                stats.max_depth = stats.max_depth.max(depth + 1);
                let Ok(child) = current.open_dir_nofollow(&name) else {
                    return (false, stats);
                };
                pending.push((child, child_relative, depth + 1));
                continue;
            }
            if !metadata.is_file() {
                return (false, stats);
            }
            let Some(expected_bytes) = expected_files.get(&key) else {
                return (false, stats);
            };
            if metadata.len() != expected_bytes.len() as u64 {
                return (false, stats);
            }
            actual_bytes = match actual_bytes.checked_add(metadata.len()) {
                Some(total) if total <= MAX_PACKAGE_BYTES => total,
                _ => return (false, stats),
            };
            let mut options = CapOpenOptions::new();
            options.read(true).follow(FollowSymlinks::No);
            let Ok(file) = current.open_with(&name, &options) else {
                return (false, stats);
            };
            let mut bytes = Vec::with_capacity(expected_bytes.len().saturating_add(1));
            if file
                .take(expected_bytes.len() as u64 + 1)
                .read_to_end(&mut bytes)
                .is_err()
                || bytes != *expected_bytes
            {
                return (false, stats);
            }
            actual_files.insert(key);
        }
    }
    let matches = if require_complete {
        actual_files == expected_files.keys().cloned().collect()
            && actual_directories == expected_directories
    } else {
        actual_files.len() <= expected_files.len()
            && actual_directories.len() <= expected_directories.len()
    };
    (matches, stats)
}

fn activation_relative_key(path: &Path) -> Option<String> {
    path.components()
        .map(|component| match component {
            std::path::Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .map(|components| components.join("/"))
}

fn restore_quarantined_child(
    parent: &Dir,
    quarantine_name: &str,
    original_name: &str,
) -> Result<(), ThemeError> {
    crate::atomic_noreplace::rename_noreplace(
        parent,
        Path::new(quarantine_name),
        parent,
        Path::new(original_name),
    )
    .map_err(|_| lease_cleanup_error())
}

fn verify_activation_lease_path(lease: &ActivationLease) -> Result<(), ThemeError> {
    let parent_path = lease.root.parent().ok_or_else(unsafe_lease_path)?;
    let addressed_parent = crate::storage_capability::ambient_symlink_metadata(parent_path)
        .map_err(|_| unsafe_lease_path())?;
    let retained_parent = lease.parent.dir_metadata().map_err(io_error)?;
    if addressed_parent.file_type().is_symlink()
        || !addressed_parent.is_dir()
        || !retained_parent.is_dir()
        || file_identity(&addressed_parent) != file_identity(&retained_parent)
    {
        return Err(unsafe_lease_path());
    }
    let addressed = lease
        .parent
        .symlink_metadata(&lease.name)
        .map_err(|_| unsafe_lease_path())?;
    if addressed.file_type().is_symlink()
        || !addressed.is_dir()
        || file_identity(&addressed) != lease.identity
    {
        return Err(unsafe_lease_path());
    }
    let retained = lease
        .parent
        .open_dir_nofollow(&lease.name)
        .map_err(|_| unsafe_lease_path())?;
    let retained = retained.dir_metadata().map_err(io_error)?;
    if !retained.is_dir() || file_identity(&retained) != lease.identity {
        return Err(unsafe_lease_path());
    }
    Ok(())
}

fn revalidate_activation_lease(
    lease: &ActivationLease,
    source: &ValidatedThemeDirectory,
) -> Result<(), ThemeError> {
    verify_activation_lease_path(lease)?;
    let directory = lease
        .parent
        .open_dir_nofollow(&lease.name)
        .map_err(|_| unsafe_lease_path())?;
    let validated = validate_theme_directory_from_retained(
        &lease.root,
        &source.descriptor.file_name,
        &directory,
    )
    .map_err(|_| lease_fingerprint_mismatch())?;
    if validated.descriptor != source.descriptor || validated.files != source.files {
        return Err(lease_fingerprint_mismatch());
    }
    verify_activation_lease_path(lease)
}

fn is_owned_activation_lease_name(name: &str) -> bool {
    is_owned_activation_child_name(name, ACTIVATION_LEASE_PREFIX, ACTIVATION_LEASE_SUFFIX)
}

fn is_owned_activation_quarantine_name(name: &str) -> bool {
    is_owned_activation_child_name(
        name,
        ACTIVATION_QUARANTINE_PREFIX,
        ACTIVATION_QUARANTINE_SUFFIX,
    )
}

fn is_owned_activation_child_name(name: &str, prefix: &str, suffix: &str) -> bool {
    let Some(stem) = name
        .strip_prefix(prefix)
        .and_then(|name| name.strip_suffix(suffix))
    else {
        return false;
    };
    let Some((process, sequence)) = stem.split_once('-') else {
        return false;
    };
    !process.is_empty()
        && !sequence.is_empty()
        && process.bytes().all(|byte| byte.is_ascii_digit())
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
}

fn file_identity<T: MetadataExt>(metadata: &T) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}

fn io_error(_error: io::Error) -> ThemeError {
    ThemeError::new(
        ThemeErrorCode::Io,
        "Theme activation storage is unavailable.",
    )
}

fn unsafe_lease_path() -> ThemeError {
    ThemeError::new(
        ThemeErrorCode::UnsafePath,
        "Theme activation storage changed or contains an unsafe path.",
    )
}

fn lease_fingerprint_mismatch() -> ThemeError {
    ThemeError::new(
        ThemeErrorCode::FingerprintMismatch,
        "The validated theme activation copy changed before it could be granted.",
    )
}

fn fingerprint_mismatch() -> ThemeError {
    ThemeError::new(
        ThemeErrorCode::FingerprintMismatch,
        "The installed theme changed after it was listed.",
    )
}

fn lease_cleanup_error() -> ThemeError {
    ThemeError::new(
        ThemeErrorCode::Io,
        "A retired theme activation lease could not be removed safely.",
    )
}

pub(crate) fn delete_theme_with_permissions(
    catalog: &ThemeCatalog,
    state: &ThemeActivationState,
    id: &str,
    expected_fingerprint: &str,
    allow_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
    forbid_directory: &mut dyn FnMut(&Path) -> Result<(), ThemeError>,
) -> Result<(), ThemeError> {
    if matches!(id, "light" | "dark") {
        return catalog.delete(id, expected_fingerprint);
    }
    let hint = state.catalog_hint(catalog, id, expected_fingerprint)?;
    let mut current = state.begin_delete_operation(id)?;
    let outcome = (|| {
        catalog.validated_resource_root_with_hint(id, expected_fingerprint, hint.as_ref())?;
        state.revoke_theme_for_delete_with_permissions(
            &mut current,
            id,
            allow_directory,
            forbid_directory,
        )?;
        catalog.delete(id, expected_fingerprint)
    })();
    state.finish_delete_operation(current, id)?;
    outcome
}

fn stale_token() -> ThemeError {
    ThemeError::new(
        ThemeErrorCode::UnsafeResource,
        "The theme activation token is no longer current for this window.",
    )
}

fn activation_state_error() -> ThemeError {
    ThemeError::new(ThemeErrorCode::Io, "Theme activation state is unavailable.")
}

fn allow_theme_directory<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    path: &Path,
) -> Result<(), ThemeError> {
    app.asset_protocol_scope()
        .allow_directory(path, true)
        .map_err(|_| permission_update_error())
}

fn forbid_theme_directory<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    path: &Path,
) -> Result<(), ThemeError> {
    app.asset_protocol_scope()
        .forbid_directory(path, true)
        .map_err(|_| permission_update_error())
}

fn permission_update_error() -> ThemeError {
    ThemeError::new(
        ThemeErrorCode::Io,
        "Theme asset permission could not be updated safely.",
    )
}

#[tauri::command]
pub(crate) fn prepare_theme_activation(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ThemeActivationState>,
    id: String,
    expected_fingerprint: String,
) -> Result<ThemeActivationPayload, ThemeError> {
    let catalog = super::prepared_catalog(window.app_handle())?;
    state.prepare_with_permissions(
        &catalog,
        window.label(),
        &id,
        &expected_fingerprint,
        &mut |path| allow_theme_directory(window.app_handle(), path),
        &mut |path| forbid_theme_directory(window.app_handle(), path),
    )
}

#[tauri::command]
pub(crate) fn commit_theme_activation(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ThemeActivationState>,
    token: String,
) -> Result<(), ThemeError> {
    state.commit_with_permissions(
        window.label(),
        &token,
        &mut |path| allow_theme_directory(window.app_handle(), path),
        &mut |path| forbid_theme_directory(window.app_handle(), path),
    )
}

#[tauri::command]
pub(crate) fn cancel_theme_activation(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ThemeActivationState>,
    token: String,
) -> Result<(), ThemeError> {
    state.cancel_with_permissions(
        window.label(),
        &token,
        &mut |path| allow_theme_directory(window.app_handle(), path),
        &mut |path| forbid_theme_directory(window.app_handle(), path),
    )
}

#[tauri::command]
pub(crate) fn release_theme_activation(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ThemeActivationState>,
) -> Result<(), ThemeError> {
    state.release_window_with_permissions(
        window.label(),
        &mut |path| allow_theme_directory(window.app_handle(), path),
        &mut |path| forbid_theme_directory(window.app_handle(), path),
    )
}

pub(crate) fn release_theme_activation_for_window<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    window_label: &str,
) -> Result<(), ThemeError> {
    let state = app
        .try_state::<ThemeActivationState>()
        .ok_or_else(activation_state_error)?;
    state.release_destroyed_window_with_permissions(
        window_label,
        &mut |path| allow_theme_directory(app, path),
        &mut |path| forbid_theme_directory(app, path),
    )
}

pub(crate) fn delete_theme_for_app(
    app: &tauri::AppHandle,
    state: &ThemeActivationState,
    catalog: &ThemeCatalog,
    id: &str,
    expected_fingerprint: &str,
) -> Result<(), ThemeError> {
    delete_theme_with_permissions(
        catalog,
        state,
        id,
        expected_fingerprint,
        &mut |path| allow_theme_directory(app, path),
        &mut |path| forbid_theme_directory(app, path),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::BTreeSet,
        fs,
        path::{Path, PathBuf},
    };

    use cap_std::fs::Dir;
    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        activation_files_match_with_stats, delete_theme_with_permissions,
        ActivationLeaseCleanupHookPoint, ActivationLeaseHookPoint, ActivationPrepareHookPoint,
        ThemeActivationSource, ThemeActivationState, ACTIVATION_QUARANTINE_PREFIX,
    };
    use crate::themes::{
        catalog::ThemeCatalog,
        resources::{ValidatedThemeFile, ValidatedThemeFileKind},
        ThemeError, ThemeErrorCode,
    };

    fn css(id: &str) -> Vec<u8> {
        format!(
            "/*\n@qingyu-theme\nid: {id}\nname: {id}\nappearance: light\npreview-background: #ffffff\npreview-panel: #f6f8fa\npreview-text: #1f2328\npreview-accent: #0969da\n*/\n:root {{ --theme-id: {id}; }}\n"
        )
        .into_bytes()
    }

    fn write_resource(root: &Path, id: &str) {
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::create_dir_all(root.join("licenses")).unwrap();
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec_pretty(&json!({
                "schemaVersion": 1,
                "id": id,
                "name": id,
                "appearance": "dark",
                "entry": "theme.css",
                "author": "QingYu",
                "preview": {
                    "background": "#101820",
                    "panel": "#182430",
                    "text": "#f2f4f6",
                    "accent": "#ffcc66"
                },
                "licenseFiles": ["licenses/LICENSE.txt"]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("theme.css"),
            b"@font-face { font-family: Fixture; src: url('./assets/font.woff2'); }\n:root { background-image: url('./assets/image.png'); }\n",
        )
        .unwrap();
        fs::write(root.join("assets/font.woff2"), b"wOF2fixture-font").unwrap();
        fs::write(root.join("assets/image.png"), b"\x89PNG\r\n\x1a\nfixture").unwrap();
        fs::write(root.join("licenses/LICENSE.txt"), b"Fixture license\n").unwrap();
    }

    fn resource_descriptor(catalog: &ThemeCatalog, id: &str) -> crate::themes::ThemeDescriptor {
        catalog
            .scan()
            .unwrap()
            .themes
            .into_iter()
            .find(|theme| theme.id == id)
            .unwrap()
    }

    fn permission_error() -> ThemeError {
        ThemeError::new(ThemeErrorCode::Io, "Injected asset permission failure.")
    }

    fn stylesheet_path(payload: &super::ThemeActivationPayload) -> PathBuf {
        let ThemeActivationSource::Stylesheet { path } = &payload.source else {
            panic!("resource themes should prepare a stylesheet path");
        };
        PathBuf::from(path)
    }

    fn lease_root(payload: &super::ThemeActivationPayload) -> PathBuf {
        stylesheet_path(payload).parent().unwrap().to_path_buf()
    }

    fn quarantine_roots(parent: &Path) -> Vec<PathBuf> {
        fs::read_dir(parent)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(ACTIVATION_QUARANTINE_PREFIX))
            })
            .collect()
    }

    #[test]
    fn legacy_preparation_returns_inline_css_without_granting_a_directory() {
        let temporary = tempdir().unwrap();
        let catalog = ThemeCatalog::at(temporary.path().join("themes"));
        let descriptor = catalog.import_bytes(&css("legacy"), "legacy.css").unwrap();
        let state = ThemeActivationState::default();
        let allowed = RefCell::new(Vec::new());
        let forbidden = RefCell::new(Vec::new());

        let payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "legacy",
                &descriptor.fingerprint,
                &mut |path| {
                    allowed.borrow_mut().push(path.to_path_buf());
                    Ok(())
                },
                &mut |path| {
                    forbidden.borrow_mut().push(path.to_path_buf());
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(payload.id, "legacy");
        assert_eq!(payload.fingerprint, descriptor.fingerprint);
        assert!(!payload.token.is_empty());
        let ThemeActivationSource::Inline { css } = payload.source else {
            panic!("legacy themes should prepare inline CSS");
        };
        assert!(css.contains("--theme-id: legacy"));
        assert!(allowed.borrow().is_empty());
        assert!(forbidden.borrow().is_empty());
    }

    #[test]
    fn resource_preparation_copies_exact_files_and_grants_only_an_owned_lease() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        write_resource(&package_root, "resource");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();
        let allowed = RefCell::new(Vec::new());

        let payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |path| {
                    allowed.borrow_mut().push(path.to_path_buf());
                    Ok(())
                },
                &mut |_| Ok(()),
            )
            .unwrap();

        let stylesheet = stylesheet_path(&payload);
        let lease_root = lease_root(&payload);
        assert_eq!(stylesheet, lease_root.join("theme.css"));
        assert_ne!(lease_root, package_root);
        assert_eq!(
            lease_root.parent(),
            Some(
                catalog_root
                    .join(".qingyu-theme-activation-leases")
                    .as_path()
            )
        );
        assert_eq!(allowed.into_inner(), vec![lease_root.clone()]);
        assert_ne!(lease_root, catalog_root);
        for relative_path in [
            "theme.css",
            "assets/font.woff2",
            "assets/image.png",
            "manifest.json",
            "licenses/LICENSE.txt",
        ] {
            assert_eq!(
                fs::read(lease_root.join(relative_path)).unwrap(),
                fs::read(package_root.join(relative_path)).unwrap(),
                "{relative_path} should be copied byte-for-byte"
            );
        }
    }

    #[test]
    fn switching_a_to_b_and_back_uses_a_fresh_lease_after_permanent_revocation() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let first_root = catalog_root.join("first");
        let second_root = catalog_root.join("second");
        write_resource(&first_root, "first");
        write_resource(&second_root, "second");
        let catalog = ThemeCatalog::at(catalog_root);
        let first = resource_descriptor(&catalog, "first");
        let second = resource_descriptor(&catalog, "second");
        let state = ThemeActivationState::default();
        let allowed = RefCell::new(BTreeSet::new());
        let forbidden = RefCell::new(BTreeSet::new());
        let mut allow = |path: &Path| {
            assert!(
                !forbidden.borrow().contains(path),
                "Tauri permanently denies a forbidden asset path"
            );
            allowed.borrow_mut().insert(path.to_path_buf());
            Ok(())
        };
        let mut forbid = |path: &Path| {
            allowed.borrow_mut().remove(path);
            forbidden.borrow_mut().insert(path.to_path_buf());
            Ok(())
        };

        let first_payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "first",
                &first.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        let first_lease = lease_root(&first_payload);
        state
            .commit_with_permissions("main", &first_payload.token, &mut allow, &mut forbid)
            .unwrap();
        let second_payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "second",
                &second.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        let second_lease = lease_root(&second_payload);
        state
            .commit_with_permissions("main", &second_payload.token, &mut allow, &mut forbid)
            .unwrap();
        assert!(forbidden.borrow().contains(&first_lease));
        assert!(!first_lease.exists());

        let replacement_payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "first",
                &first.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        let replacement_lease = lease_root(&replacement_payload);
        assert_ne!(replacement_lease, first_lease);
        assert!(!forbidden.borrow().contains(&replacement_lease));
        state
            .commit_with_permissions("main", &replacement_payload.token, &mut allow, &mut forbid)
            .unwrap();

        assert!(forbidden.borrow().contains(&second_lease));
        assert!(!second_lease.exists());
        assert_eq!(
            allowed.into_inner(),
            BTreeSet::from([replacement_lease.clone()])
        );
        assert_eq!(state.active_root("main"), Some(replacement_lease));
    }

    #[test]
    fn windows_share_one_live_lease_for_the_same_source_and_fingerprint() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("shared");
        write_resource(&package_root, "shared");
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "shared");
        let state = ThemeActivationState::default();
        let allowed = RefCell::new(Vec::new());
        let forbidden = RefCell::new(Vec::new());
        let mut allow = |path: &Path| {
            allowed.borrow_mut().push(path.to_path_buf());
            Ok(())
        };
        let mut forbid = |path: &Path| {
            forbidden.borrow_mut().push(path.to_path_buf());
            Ok(())
        };

        let main = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "shared",
                &descriptor.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        let settings = state
            .prepare_with_permissions(
                &catalog,
                "settings",
                "shared",
                &descriptor.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        let shared_lease = lease_root(&main);
        assert_eq!(lease_root(&settings), shared_lease);
        assert_eq!(allowed.borrow().as_slice(), [shared_lease.clone()]);
        state
            .commit_with_permissions("main", &main.token, &mut allow, &mut forbid)
            .unwrap();
        state
            .cancel_with_permissions("settings", &settings.token, &mut allow, &mut forbid)
            .unwrap();
        assert!(forbidden.borrow().is_empty());
        state
            .release_window_with_permissions("main", &mut allow, &mut forbid)
            .unwrap();
        assert_eq!(forbidden.into_inner(), vec![shared_lease.clone()]);
        assert!(!shared_lease.exists());
    }

    #[test]
    fn first_resource_activation_removes_only_stale_owned_leases() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        let lease_parent = catalog_root.join(".qingyu-theme-activation-leases");
        let stale = lease_parent.join(".qingyu-theme-lease-999999-1.dir");
        let arbitrary = lease_parent.join("author-content");
        write_resource(&package_root, "resource");
        fs::create_dir_all(&stale).unwrap();
        fs::write(stale.join("old"), b"stale").unwrap();
        fs::create_dir_all(&arbitrary).unwrap();
        fs::write(arbitrary.join("keep"), b"author").unwrap();
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();

        let payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();

        assert!(!stale.exists());
        assert_eq!(fs::read(arbitrary.join("keep")).unwrap(), b"author");
        assert_ne!(lease_root(&payload), arbitrary);
    }

    #[test]
    fn wide_stale_lease_cleanup_is_bounded_and_advisory() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        let lease_parent = catalog_root.join(".qingyu-theme-activation-leases");
        let stale = lease_parent.join(".qingyu-theme-lease-999999-2.dir");
        write_resource(&package_root, "resource");
        fs::create_dir_all(&stale).unwrap();
        for index in 0..=crate::themes::resources::MAX_PACKAGE_ENTRIES {
            fs::write(stale.join(format!("wide-{index}")), b"preserve").unwrap();
        }
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();

        let payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();

        assert!(lease_root(&payload).exists());
        assert!(!stale.exists());
        let quarantines = quarantine_roots(&lease_parent);
        assert_eq!(quarantines.len(), 1);
        assert_eq!(
            fs::read_dir(&quarantines[0]).unwrap().count(),
            crate::themes::resources::MAX_PACKAGE_ENTRIES + 1
        );
    }

    #[test]
    fn deep_stale_lease_cleanup_is_bounded_and_advisory() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        let lease_parent = catalog_root.join(".qingyu-theme-activation-leases");
        let stale = lease_parent.join(".qingyu-theme-lease-999999-3.dir");
        write_resource(&package_root, "resource");
        fs::create_dir_all(&stale).unwrap();
        let mut deepest = stale.clone();
        for depth in 0..=16 {
            deepest = deepest.join(format!("depth-{depth}"));
            fs::create_dir(&deepest).unwrap();
        }
        fs::write(deepest.join("preserve"), b"deep").unwrap();
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();

        let payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();

        assert!(lease_root(&payload).exists());
        assert!(!stale.exists());
        let quarantines = quarantine_roots(&lease_parent);
        assert_eq!(quarantines.len(), 1);
        let mut preserved = quarantines[0].clone();
        for depth in 0..=16 {
            preserved = preserved.join(format!("depth-{depth}"));
        }
        assert_eq!(fs::read(preserved.join("preserve")).unwrap(), b"deep");
    }

    #[test]
    fn copied_lease_is_revalidated_before_permission_is_granted() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        write_resource(&package_root, "resource");
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();
        let allowed = RefCell::new(Vec::new());
        let copied_root = RefCell::new(None);

        let error = state
            .prepare_with_permissions_and_lease_hook(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |path| {
                    allowed.borrow_mut().push(path.to_path_buf());
                    Ok(())
                },
                &mut |_| Ok(()),
                &mut |point, root| {
                    if point == ActivationLeaseHookPoint::AfterCopy {
                        copied_root.replace(Some(root.to_path_buf()));
                        fs::write(root.join("theme.css"), b":root { --tampered: true; }").unwrap();
                    }
                    Ok(())
                },
            )
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::FingerprintMismatch);
        assert!(allowed.borrow().is_empty());
        assert_eq!(state.pending_count(), 0);
        assert!(!copied_root.into_inner().unwrap().exists());
    }

    #[test]
    fn lease_creation_retries_create_new_collisions_without_touching_the_occupant() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        write_resource(&package_root, "resource");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();
        let first = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();
        let first_lease = lease_root(&first);
        state
            .cancel_with_permissions("main", &first.token, &mut |_| Ok(()), &mut |_| Ok(()))
            .unwrap();
        assert!(!first_lease.exists());

        let occupied = catalog_root
            .join(".qingyu-theme-activation-leases")
            .join(format!(".qingyu-theme-lease-{}-2.dir", std::process::id()));
        fs::create_dir(&occupied).unwrap();
        fs::write(occupied.join("keep"), b"collision").unwrap();

        let replacement = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();

        assert_ne!(lease_root(&replacement), occupied);
        assert_eq!(fs::read(occupied.join("keep")).unwrap(), b"collision");
    }

    #[test]
    fn removal_failure_commits_truthful_revocation_and_tracks_cleanup_for_retry() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        write_resource(&package_root, "resource");
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();
        let prepared = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();
        let retired_lease = lease_root(&prepared);
        state
            .commit_with_permissions("main", &prepared.token, &mut |_| Ok(()), &mut |_| Ok(()))
            .unwrap();
        let forbidden = RefCell::new(Vec::new());

        let error = state
            .release_window_with_permissions_and_remove_hook(
                "main",
                &mut |_| Ok(()),
                &mut |path| {
                    forbidden.borrow_mut().push(path.to_path_buf());
                    Ok(())
                },
                &mut |_| Err(permission_error()),
            )
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::Io);
        assert_eq!(forbidden.into_inner(), vec![retired_lease.clone()]);
        assert_eq!(state.active_root("main"), None);
        assert_eq!(state.pending_count(), 0);
        assert_eq!(state.retired_count(), 1);
        assert!(retired_lease.exists());

        let replacement = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();
        assert!(!retired_lease.exists());
        assert_eq!(state.retired_count(), 0);
        assert_ne!(lease_root(&replacement), retired_lease);
    }

    #[test]
    fn release_uses_the_retained_lease_parent_and_never_removes_a_path_replacement() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        write_resource(&package_root, "resource");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();
        let prepared = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();
        let lease = lease_root(&prepared);
        state
            .commit_with_permissions("main", &prepared.token, &mut |_| Ok(()), &mut |_| Ok(()))
            .unwrap();

        let lease_parent = lease.parent().unwrap();
        let retained_parent = catalog_root.join("retained-lease-parent");
        let lease_name = lease.file_name().unwrap();
        fs::rename(lease_parent, &retained_parent).unwrap();
        fs::create_dir(lease_parent).unwrap();
        let replacement = lease_parent.join(lease_name);
        fs::create_dir(&replacement).unwrap();
        fs::write(replacement.join("keep"), b"arbitrary replacement").unwrap();

        state
            .release_window_with_permissions("main", &mut |_| Ok(()), &mut |_| Ok(()))
            .unwrap();

        assert_eq!(
            fs::read(replacement.join("keep")).unwrap(),
            b"arbitrary replacement"
        );
        assert!(!retained_parent.join(lease_name).exists());
        assert_eq!(state.active_root("main"), None);
    }

    #[test]
    fn deleting_a_source_revokes_all_of_its_leases_without_touching_other_sources() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let first_root = catalog_root.join("first");
        let second_root = catalog_root.join("second");
        write_resource(&first_root, "first");
        write_resource(&second_root, "second");
        let catalog = ThemeCatalog::at(catalog_root);
        let first = resource_descriptor(&catalog, "first");
        let second = resource_descriptor(&catalog, "second");
        let state = ThemeActivationState::default();
        let forbidden = RefCell::new(Vec::new());
        let mut allow = |_: &Path| Ok(());
        let mut forbid = |path: &Path| {
            forbidden.borrow_mut().push(path.to_path_buf());
            Ok(())
        };

        let first_main = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "first",
                &first.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        state
            .commit_with_permissions("main", &first_main.token, &mut allow, &mut forbid)
            .unwrap();
        let first_settings = state
            .prepare_with_permissions(
                &catalog,
                "settings",
                "first",
                &first.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        let second_other = state
            .prepare_with_permissions(
                &catalog,
                "other",
                "second",
                &second.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        state
            .commit_with_permissions("other", &second_other.token, &mut allow, &mut forbid)
            .unwrap();
        let first_lease = lease_root(&first_main);
        let second_lease = lease_root(&second_other);
        assert_eq!(lease_root(&first_settings), first_lease);

        delete_theme_with_permissions(
            &catalog,
            &state,
            "first",
            &first.fingerprint,
            &mut allow,
            &mut forbid,
        )
        .unwrap();

        assert_eq!(forbidden.into_inner(), vec![first_lease.clone()]);
        assert!(!first_root.exists());
        assert!(!first_lease.exists());
        assert!(second_root.exists());
        assert!(second_lease.exists());
        assert_eq!(state.active_root("main"), None);
        assert_eq!(state.active_root("other"), Some(second_lease));
        assert_eq!(state.pending_count(), 0);
    }

    #[test]
    fn commit_replaces_the_window_activation_and_revokes_the_unreferenced_old_root() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let first_root = catalog_root.join("first");
        let second_root = catalog_root.join("second");
        write_resource(&first_root, "first");
        write_resource(&second_root, "second");
        let catalog = ThemeCatalog::at(catalog_root);
        let first = resource_descriptor(&catalog, "first");
        let second = resource_descriptor(&catalog, "second");
        let state = ThemeActivationState::default();
        let allowed = RefCell::new(Vec::new());
        let forbidden = RefCell::new(Vec::new());
        let mut allow = |path: &Path| {
            allowed.borrow_mut().push(path.to_path_buf());
            Ok(())
        };
        let mut forbid = |path: &Path| {
            forbidden.borrow_mut().push(path.to_path_buf());
            Ok(())
        };

        let first_payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "first",
                &first.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        let first_lease = lease_root(&first_payload);
        state
            .commit_with_permissions("main", &first_payload.token, &mut allow, &mut forbid)
            .unwrap();
        let second_payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "second",
                &second.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        let second_lease = lease_root(&second_payload);

        assert!(forbidden.borrow().is_empty());
        state
            .commit_with_permissions("main", &second_payload.token, &mut allow, &mut forbid)
            .unwrap();

        assert_eq!(
            allowed.into_inner(),
            vec![first_lease.clone(), second_lease.clone()]
        );
        assert_eq!(forbidden.into_inner(), vec![first_lease.clone()]);
        assert!(!first_lease.exists());
        assert_eq!(state.active_root("main"), Some(second_lease));
    }

    #[test]
    fn cancel_and_release_revoke_only_roots_without_other_references() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("shared");
        write_resource(&package_root, "shared");
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "shared");
        let state = ThemeActivationState::default();
        let allowed = RefCell::new(Vec::new());
        let forbidden = RefCell::new(Vec::new());
        let mut allow = |path: &Path| {
            allowed.borrow_mut().push(path.to_path_buf());
            Ok(())
        };
        let mut forbid = |path: &Path| {
            forbidden.borrow_mut().push(path.to_path_buf());
            Ok(())
        };

        let first = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "shared",
                &descriptor.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        let shared_lease = lease_root(&first);
        state
            .commit_with_permissions("main", &first.token, &mut allow, &mut forbid)
            .unwrap();
        let second = state
            .prepare_with_permissions(
                &catalog,
                "settings",
                "shared",
                &descriptor.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();

        state
            .cancel_with_permissions("settings", &second.token, &mut allow, &mut forbid)
            .unwrap();
        assert!(forbidden.borrow().is_empty());
        state
            .release_window_with_permissions("main", &mut allow, &mut forbid)
            .unwrap();

        assert_eq!(allowed.into_inner(), vec![shared_lease.clone()]);
        assert_eq!(forbidden.into_inner(), vec![shared_lease.clone()]);
        assert!(!shared_lease.exists());
    }

    #[test]
    fn newer_preparation_invalidates_the_older_token_for_the_same_window() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        write_resource(&package_root, "resource");
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();
        let mut allow = |_: &Path| Ok(());
        let mut forbid = |_: &Path| Ok(());

        let older = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        let newer = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();

        assert_ne!(older.token, newer.token);
        assert_eq!(
            state
                .commit_with_permissions("main", &older.token, &mut allow, &mut forbid)
                .unwrap_err()
                .code,
            ThemeErrorCode::UnsafeResource
        );
        state
            .commit_with_permissions("main", &newer.token, &mut allow, &mut forbid)
            .unwrap();
    }

    #[test]
    fn release_window_and_delete_theme_remove_all_matching_references() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        write_resource(&package_root, "resource");
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();
        let forbidden = RefCell::new(Vec::new());
        let mut allow = |_: &Path| Ok(());
        let mut forbid = |path: &Path| {
            forbidden.borrow_mut().push(path.to_path_buf());
            Ok(())
        };

        let main = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        let lease = lease_root(&main);
        state
            .commit_with_permissions("main", &main.token, &mut allow, &mut forbid)
            .unwrap();
        state
            .prepare_with_permissions(
                &catalog,
                "settings",
                "resource",
                &descriptor.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();

        state
            .release_window_with_permissions("settings", &mut allow, &mut forbid)
            .unwrap();
        assert!(forbidden.borrow().is_empty());
        delete_theme_with_permissions(
            &catalog,
            &state,
            "resource",
            &descriptor.fingerprint,
            &mut allow,
            &mut forbid,
        )
        .unwrap();

        assert_eq!(forbidden.into_inner(), vec![lease.clone()]);
        assert!(!lease.exists());
        assert!(!package_root.exists());
        assert_eq!(state.active_root("main"), None);
        assert_eq!(state.pending_count(), 0);
    }

    #[test]
    fn every_package_file_is_revalidated_before_a_stylesheet_path_is_exposed() {
        for relative_path in [
            "theme.css",
            "assets/font.woff2",
            "assets/image.png",
            "manifest.json",
            "licenses/LICENSE.txt",
        ] {
            for remove in [false, true] {
                let temporary = tempdir().unwrap();
                let catalog_root = temporary.path().join("themes");
                let package_root = catalog_root.join("resource");
                write_resource(&package_root, "resource");
                let catalog = ThemeCatalog::at(catalog_root);
                let descriptor = resource_descriptor(&catalog, "resource");
                let target = package_root.join(relative_path);
                if remove {
                    fs::remove_file(target).unwrap();
                } else {
                    let replacement: &[u8] = match relative_path {
                        "theme.css" => b":root { --changed: true; }\n",
                        "assets/font.woff2" => b"wOF2changed-font",
                        "assets/image.png" => b"\x89PNG\r\n\x1a\nchanged",
                        "manifest.json" => br##"{"schemaVersion":1,"id":"resource","name":"resource","appearance":"dark","entry":"theme.css","author":"Changed","preview":{"background":"#101820","panel":"#182430","text":"#f2f4f6","accent":"#ffcc66"},"licenseFiles":["licenses/LICENSE.txt"]}"##,
                        "licenses/LICENSE.txt" => b"Changed license\n",
                        _ => unreachable!(),
                    };
                    fs::write(target, replacement).unwrap();
                }
                let state = ThemeActivationState::default();
                let allowed = RefCell::new(Vec::new());

                let error = state
                    .prepare_with_permissions(
                        &catalog,
                        "main",
                        "resource",
                        &descriptor.fingerprint,
                        &mut |path| {
                            allowed.borrow_mut().push(path.to_path_buf());
                            Ok(())
                        },
                        &mut |_| Ok(()),
                    )
                    .unwrap_err();

                assert_eq!(
                    error.code,
                    ThemeErrorCode::FingerprintMismatch,
                    "{relative_path} remove={remove} should be stale"
                );
                assert!(allowed.borrow().is_empty());
            }
        }
    }

    #[test]
    fn a_shared_lease_is_fully_revalidated_before_its_path_is_exposed_again() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        write_resource(&package_root, "resource");
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();
        let allowed = RefCell::new(Vec::new());
        let first = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |path| {
                    allowed.borrow_mut().push(path.to_path_buf());
                    Ok(())
                },
                &mut |_| Ok(()),
            )
            .unwrap();
        fs::write(
            lease_root(&first).join("assets/font.woff2"),
            b"wOF2tampered-lease-font",
        )
        .unwrap();

        let error = state
            .prepare_with_permissions(
                &catalog,
                "settings",
                "resource",
                &descriptor.fingerprint,
                &mut |path| {
                    allowed.borrow_mut().push(path.to_path_buf());
                    Ok(())
                },
                &mut |_| Ok(()),
            )
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::FingerprintMismatch);
        assert_eq!(allowed.into_inner(), vec![lease_root(&first)]);
        assert_eq!(state.pending_count(), 1);
    }

    #[test]
    fn permission_failures_roll_back_state_without_granting_a_broader_root() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        write_resource(&package_root, "resource");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();
        let attempted_grant = RefCell::new(None);
        let revoked_after_error = RefCell::new(Vec::new());

        let allow_error = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |path| {
                    attempted_grant.replace(Some(path.to_path_buf()));
                    Err(permission_error())
                },
                &mut |path| {
                    revoked_after_error.borrow_mut().push(path.to_path_buf());
                    Ok(())
                },
            )
            .unwrap_err();
        assert_eq!(allow_error.code, ThemeErrorCode::Io);
        let attempted_grant = attempted_grant.into_inner().unwrap();
        assert_eq!(
            revoked_after_error.into_inner(),
            vec![attempted_grant.clone()]
        );
        assert!(!attempted_grant.exists());
        assert_eq!(state.pending_count(), 0);
        assert_eq!(state.active_root("main"), None);

        let payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |path| {
                    assert_ne!(path, package_root);
                    assert_ne!(path, catalog_root);
                    assert_eq!(
                        path.parent(),
                        Some(
                            catalog_root
                                .join(".qingyu-theme-activation-leases")
                                .as_path()
                        )
                    );
                    Ok(())
                },
                &mut |_| Ok(()),
            )
            .unwrap();
        let lease = lease_root(&payload);
        let forbid_error = state
            .cancel_with_permissions("main", &payload.token, &mut |_| Ok(()), &mut |_| {
                Err(permission_error())
            })
            .unwrap_err();
        assert_eq!(forbid_error.code, ThemeErrorCode::Io);
        assert_eq!(state.pending_count(), 1);

        state
            .cancel_with_permissions("main", &payload.token, &mut |_| Ok(()), &mut |path| {
                assert_eq!(path, lease);
                Ok(())
            })
            .unwrap();
        assert_eq!(state.pending_count(), 0);
    }

    #[test]
    fn delete_forbids_an_unreferenced_resource_root_and_preserves_protected_errors() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        write_resource(&package_root, "resource");
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();
        let forbidden = RefCell::new(Vec::new());

        delete_theme_with_permissions(
            &catalog,
            &state,
            "resource",
            &descriptor.fingerprint,
            &mut |_| Ok(()),
            &mut |path| {
                forbidden.borrow_mut().push(path.to_path_buf());
                Ok(())
            },
        )
        .unwrap();

        assert!(forbidden.into_inner().is_empty());
        let error = delete_theme_with_permissions(
            &catalog,
            &state,
            "light",
            "default:light",
            &mut |_| Ok(()),
            &mut |_| panic!("protected themes have no resource scope"),
        )
        .unwrap_err();
        assert_eq!(error.code, ThemeErrorCode::ProtectedTheme);
    }

    #[test]
    fn failed_commit_forbid_keeps_the_old_activation_and_retryable_candidate() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let first_root = catalog_root.join("first");
        let second_root = catalog_root.join("second");
        write_resource(&first_root, "first");
        write_resource(&second_root, "second");
        let catalog = ThemeCatalog::at(catalog_root);
        let first = resource_descriptor(&catalog, "first");
        let second = resource_descriptor(&catalog, "second");
        let state = ThemeActivationState::default();
        let mut allow = |_: &Path| Ok(());
        let mut forbid = |_: &Path| Ok(());
        let first_payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "first",
                &first.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        let first_lease = lease_root(&first_payload);
        state
            .commit_with_permissions("main", &first_payload.token, &mut allow, &mut forbid)
            .unwrap();
        let second_payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "second",
                &second.fingerprint,
                &mut allow,
                &mut forbid,
            )
            .unwrap();
        let second_lease = lease_root(&second_payload);

        let error = state
            .commit_with_permissions("main", &second_payload.token, &mut allow, &mut |path| {
                assert_eq!(path, first_lease);
                Err(permission_error())
            })
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::Io);
        assert_eq!(state.active_root("main"), Some(first_lease.clone()));
        assert_eq!(state.pending_count(), 1);
        state
            .commit_with_permissions("main", &second_payload.token, &mut allow, &mut |path| {
                assert_eq!(path, first_lease);
                Ok(())
            })
            .unwrap();
        assert_eq!(state.active_root("main"), Some(second_lease));
    }

    #[test]
    fn supersession_failure_revokes_the_new_grant_and_keeps_the_old_token() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let first_root = catalog_root.join("first");
        let second_root = catalog_root.join("second");
        write_resource(&first_root, "first");
        write_resource(&second_root, "second");
        let catalog = ThemeCatalog::at(catalog_root);
        let first = resource_descriptor(&catalog, "first");
        let second = resource_descriptor(&catalog, "second");
        let state = ThemeActivationState::default();
        let old_payload = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "first",
                &first.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();
        let first_lease = lease_root(&old_payload);
        let calls = RefCell::new(Vec::new());

        let error = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "second",
                &second.fingerprint,
                &mut |path| {
                    calls.borrow_mut().push(("allow", path.to_path_buf()));
                    Ok(())
                },
                &mut |path| {
                    calls.borrow_mut().push(("forbid", path.to_path_buf()));
                    if path == first_lease {
                        Err(permission_error())
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::Io);
        let calls = calls.into_inner();
        let second_lease = calls.first().unwrap().1.clone();
        assert_eq!(
            calls,
            vec![
                ("allow", second_lease.clone()),
                ("forbid", first_lease.clone()),
                ("forbid", second_lease.clone())
            ]
        );
        assert!(!second_lease.exists());
        assert_eq!(state.pending_count(), 1);
        state
            .commit_with_permissions("main", &old_payload.token, &mut |_| Ok(()), &mut |_| Ok(()))
            .unwrap();
        assert_eq!(state.active_root("main"), Some(first_lease));
    }

    #[test]
    fn deleting_a_legacy_theme_invalidates_its_pending_token() {
        let temporary = tempdir().unwrap();
        let catalog = ThemeCatalog::at(temporary.path().join("themes"));
        let descriptor = catalog
            .import_bytes(&css("legacy-delete"), "author.css")
            .unwrap();
        let state = ThemeActivationState::default();
        let prepared = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "legacy-delete",
                &descriptor.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();

        delete_theme_with_permissions(
            &catalog,
            &state,
            "legacy-delete",
            &descriptor.fingerprint,
            &mut |_| Ok(()),
            &mut |_| Ok(()),
        )
        .unwrap();

        assert_eq!(state.pending_count(), 0);
        assert_eq!(
            state
                .commit_with_permissions("main", &prepared.token, &mut |_| Ok(()), &mut |_| Ok(()),)
                .unwrap_err()
                .code,
            ThemeErrorCode::UnsafeResource
        );
        let protected = delete_theme_with_permissions(
            &catalog,
            &state,
            "light",
            "default:light",
            &mut |_| Ok(()),
            &mut |_| panic!("protected themes have no activation scope"),
        )
        .unwrap_err();
        assert_eq!(protected.code, ThemeErrorCode::ProtectedTheme);
    }

    #[test]
    fn listed_direct_author_storage_maps_every_invalidated_file_to_fingerprint_mismatch() {
        for relative_path in [
            "theme.css",
            "assets/font.woff2",
            "assets/image.png",
            "manifest.json",
            "licenses/LICENSE.txt",
        ] {
            for remove in [false, true] {
                let temporary = tempdir().unwrap();
                let catalog_root = temporary.path().join("themes");
                let package_root = catalog_root.join("author-chosen-storage");
                write_resource(&package_root, "direct-resource");
                let catalog = ThemeCatalog::at(catalog_root);
                let snapshot = catalog.scan().unwrap();
                let descriptor = snapshot
                    .themes
                    .iter()
                    .find(|theme| theme.id == "direct-resource")
                    .unwrap()
                    .clone();
                let state = ThemeActivationState::default();
                state
                    .remember_catalog_snapshot(&catalog, &snapshot)
                    .unwrap();
                let target = package_root.join(relative_path);
                if remove {
                    fs::remove_file(target).unwrap();
                } else {
                    let replacement: &[u8] = match relative_path {
                        "theme.css" => b":root { --changed: true; }\n",
                        "assets/font.woff2" => b"wOF2changed-font",
                        "assets/image.png" => b"\x89PNG\r\n\x1a\nchanged",
                        "manifest.json" => b"{not-valid-json",
                        "licenses/LICENSE.txt" => b"Changed license\n",
                        _ => unreachable!(),
                    };
                    fs::write(target, replacement).unwrap();
                }
                let invalid_snapshot = catalog.scan().unwrap();
                state
                    .remember_catalog_snapshot(&catalog, &invalid_snapshot)
                    .unwrap();

                let error = state
                    .prepare_with_permissions(
                        &catalog,
                        "main",
                        "direct-resource",
                        &descriptor.fingerprint,
                        &mut |_| Ok(()),
                        &mut |_| Ok(()),
                    )
                    .unwrap_err();

                assert_eq!(
                    error.code,
                    ThemeErrorCode::FingerprintMismatch,
                    "{relative_path} remove={remove} should map through its listed storage hint"
                );
            }
        }
    }

    #[test]
    fn cached_storage_hint_never_activates_an_id_omitted_by_a_fresh_duplicate_scan() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let author_root = catalog_root.join("author-storage");
        write_resource(&author_root, "duplicate-resource");
        let catalog = ThemeCatalog::at(catalog_root.clone());
        let listed = catalog.scan().unwrap();
        let descriptor = listed
            .themes
            .iter()
            .find(|theme| theme.id == "duplicate-resource")
            .unwrap()
            .clone();
        let state = ThemeActivationState::default();
        state.remember_catalog_snapshot(&catalog, &listed).unwrap();

        write_resource(
            &catalog_root.join("second-valid-storage"),
            "duplicate-resource",
        );
        let duplicate_scan = catalog.scan().unwrap();
        assert!(duplicate_scan
            .themes
            .iter()
            .all(|theme| theme.id != "duplicate-resource"));
        state
            .remember_catalog_snapshot(&catalog, &duplicate_scan)
            .unwrap();
        let allowed = RefCell::new(Vec::new());

        let error = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "duplicate-resource",
                &descriptor.fingerprint,
                &mut |path| {
                    allowed.borrow_mut().push(path.to_path_buf());
                    Ok(())
                },
                &mut |_| Ok(()),
            )
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::FingerprintMismatch);
        assert!(allowed.into_inner().is_empty());
        assert_eq!(state.pending_count(), 0);
    }

    #[test]
    fn cached_direct_legacy_invalidation_hides_size_and_metadata_subtypes() {
        for invalid_bytes in [
            vec![b'A'; crate::themes::parser::MAX_THEME_BYTES + 1],
            b":root { --invalid-metadata: true; }\n".to_vec(),
        ] {
            let temporary = tempdir().unwrap();
            let catalog_root = temporary.path().join("themes");
            fs::create_dir_all(&catalog_root).unwrap();
            let source = catalog_root.join("author-choice.css");
            fs::write(&source, css("direct-legacy")).unwrap();
            let catalog = ThemeCatalog::at(catalog_root);
            let listed = catalog.scan().unwrap();
            let descriptor = listed
                .themes
                .iter()
                .find(|theme| theme.id == "direct-legacy")
                .unwrap()
                .clone();
            let state = ThemeActivationState::default();
            state.remember_catalog_snapshot(&catalog, &listed).unwrap();
            fs::write(&source, invalid_bytes).unwrap();
            let invalid_scan = catalog.scan().unwrap();
            assert!(invalid_scan
                .themes
                .iter()
                .all(|theme| theme.id != "direct-legacy"));
            state
                .remember_catalog_snapshot(&catalog, &invalid_scan)
                .unwrap();

            let error = state
                .prepare_with_permissions(
                    &catalog,
                    "main",
                    "direct-legacy",
                    &descriptor.fingerprint,
                    &mut |_| Ok(()),
                    &mut |_| Ok(()),
                )
                .unwrap_err();

            assert_eq!(error.code, ThemeErrorCode::FingerprintMismatch);
        }
    }

    #[test]
    fn deletion_operation_serializes_unrelated_permission_state_without_resurrecting_tombstone() {
        let state = ThemeActivationState::default();
        let deletion = state.begin_delete_operation("target-theme").unwrap();

        let busy = match state.begin_permission_operation(None) {
            Err(error) => error,
            Ok(_) => panic!("an unrelated permission operation should observe deletion as busy"),
        };
        assert_eq!(busy.code, ThemeErrorCode::Io);

        state
            .finish_delete_operation(deletion, "target-theme")
            .unwrap();
        let unrelated = state.begin_permission_operation(None).unwrap();
        state.finish_permission_operation(unrelated).unwrap();

        let registry = state.registry.lock().unwrap();
        assert!(!registry.permission_operation);
        assert!(!registry.deleting.contains("target-theme"));
        assert_eq!(
            registry.theme_generations.get("target-theme").copied(),
            Some(1)
        );
    }

    #[test]
    fn cleanup_preserves_a_child_substituted_after_identity_validation() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        write_resource(&package_root, "resource");
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();
        let prepared = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();
        let lease = lease_root(&prepared);
        state
            .commit_with_permissions("main", &prepared.token, &mut |_| Ok(()), &mut |_| Ok(()))
            .unwrap();
        let displaced = lease.parent().unwrap().join("displaced-owned-lease");
        let hook_calls = RefCell::new(0_u32);

        let error = state
            .release_window_with_permissions_and_cleanup_hook(
                "main",
                &mut |_| Ok(()),
                &mut |_| Ok(()),
                &mut |point, root| {
                    assert_eq!(point, ActivationLeaseCleanupHookPoint::AfterIdentityCheck);
                    hook_calls.replace_with(|calls| *calls + 1);
                    fs::rename(root, &displaced).unwrap();
                    fs::create_dir(root).unwrap();
                    fs::write(root.join("keep"), b"attacker replacement").unwrap();
                    Ok(())
                },
            )
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::Io);
        assert_eq!(hook_calls.into_inner(), 1);
        assert_eq!(
            fs::read(lease.join("keep")).unwrap(),
            b"attacker replacement"
        );
        assert!(displaced.join("theme.css").exists());
        assert_eq!(state.active_root("main"), None);
        assert_eq!(state.retired_count(), 1);
    }

    #[test]
    fn partial_recursive_delete_stays_quarantined_and_does_not_block_unrelated_activation() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        write_resource(&catalog_root.join("first"), "first");
        write_resource(&catalog_root.join("second"), "second");
        let catalog = ThemeCatalog::at(catalog_root);
        let first = resource_descriptor(&catalog, "first");
        let second = resource_descriptor(&catalog, "second");
        let state = ThemeActivationState::default();
        let prepared = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "first",
                &first.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();
        let original_lease = lease_root(&prepared);
        state
            .commit_with_permissions("main", &prepared.token, &mut |_| Ok(()), &mut |_| Ok(()))
            .unwrap();
        let removals = RefCell::new(0_u32);

        let error = state
            .release_window_with_permissions_and_partial_remove_hook(
                "main",
                &mut |_| Ok(()),
                &mut |_| Ok(()),
                &mut |_| {
                    removals.replace_with(|count| *count + 1);
                    Err(std::io::Error::other("injected partial cleanup failure"))
                },
            )
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::Io);
        assert_eq!(removals.into_inner(), 1);
        assert!(!original_lease.exists());
        let retired_root = state
            .registry
            .lock()
            .unwrap()
            .retired
            .values()
            .next()
            .unwrap()
            .root
            .clone();
        assert!(retired_root.exists());
        assert!(retired_root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(ACTIVATION_QUARANTINE_PREFIX)));

        state
            .prepare_with_permissions(
                &catalog,
                "other",
                "second",
                &second.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();

        assert!(!retired_root.exists());
        assert_eq!(state.retired_count(), 0);
    }

    #[test]
    fn cleanup_validation_rejects_deep_and_wide_unknown_trees_before_descent() {
        let temporary = tempdir().unwrap();
        let root = temporary.path().join("lease");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("theme.css"), b":root {}\n").unwrap();
        let unknown = root.join("unknown");
        fs::create_dir(&unknown).unwrap();
        let mut deep = unknown.clone();
        for _ in 0..96 {
            deep = deep.join("d");
            fs::create_dir(&deep).unwrap();
        }
        for index in 0..300 {
            fs::write(deep.join(format!("wide-{index}")), b"unknown").unwrap();
        }
        let directory = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let expected = vec![ValidatedThemeFile {
            relative_path: "theme.css".to_string(),
            bytes: b":root {}\n".to_vec(),
            kind: ValidatedThemeFileKind::Stylesheet,
        }];

        let (matches, stats) = activation_files_match_with_stats(&directory, &expected);

        assert!(!matches);
        assert!(stats.visited_entries <= 2);
        assert_eq!(stats.max_depth, 0);
    }

    #[test]
    fn deletion_between_validation_and_registration_invalidates_prepare() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        write_resource(&package_root, "resource");
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();
        let allowed = RefCell::new(Vec::new());

        let error = state
            .prepare_with_permissions_and_prepare_hook(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |path| {
                    allowed.borrow_mut().push(path.to_path_buf());
                    Ok(())
                },
                &mut |_| Ok(()),
                &mut |point| {
                    assert_eq!(point, ActivationPrepareHookPoint::AfterCatalogValidation);
                    delete_theme_with_permissions(
                        &catalog,
                        &state,
                        "resource",
                        &descriptor.fingerprint,
                        &mut |_| Ok(()),
                        &mut |_| Ok(()),
                    )
                },
            )
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::FingerprintMismatch);
        assert!(allowed.into_inner().is_empty());
        assert!(!package_root.exists());
        assert_eq!(state.pending_count(), 0);
    }

    #[test]
    fn destroyed_window_forbid_failure_queues_cleanup_without_a_zombie_reference() {
        let temporary = tempdir().unwrap();
        let catalog_root = temporary.path().join("themes");
        let package_root = catalog_root.join("resource");
        write_resource(&package_root, "resource");
        let catalog = ThemeCatalog::at(catalog_root);
        let descriptor = resource_descriptor(&catalog, "resource");
        let state = ThemeActivationState::default();
        let prepared = state
            .prepare_with_permissions(
                &catalog,
                "dead-window",
                "resource",
                &descriptor.fingerprint,
                &mut |_| Ok(()),
                &mut |_| Ok(()),
            )
            .unwrap();
        let dead_lease = lease_root(&prepared);
        state
            .commit_with_permissions("dead-window", &prepared.token, &mut |_| Ok(()), &mut |_| {
                Ok(())
            })
            .unwrap();
        let forbid_calls = RefCell::new(Vec::new());

        let error = state
            .release_destroyed_window_with_permissions(
                "dead-window",
                &mut |_| Ok(()),
                &mut |path| {
                    forbid_calls.borrow_mut().push(path.to_path_buf());
                    Err(permission_error())
                },
            )
            .unwrap_err();
        assert_eq!(error.code, ThemeErrorCode::Io);
        assert_eq!(state.active_root("dead-window"), None);
        assert_eq!(state.orphaned_allowed_count(), 1);

        let replacement = state
            .prepare_with_permissions(
                &catalog,
                "main",
                "resource",
                &descriptor.fingerprint,
                &mut |_| Ok(()),
                &mut |path| {
                    forbid_calls.borrow_mut().push(path.to_path_buf());
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(
            forbid_calls.into_inner(),
            vec![dead_lease.clone(), dead_lease.clone()]
        );
        assert!(!dead_lease.exists());
        assert_eq!(state.orphaned_allowed_count(), 0);
        assert_ne!(lease_root(&replacement), dead_lease);
    }
}
