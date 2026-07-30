//! Sync engine planning and execution boundary.

use std::collections::BTreeMap;

use super::repository::SyncManifestEntry;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileSyncAction {
    Conflict,
    DeleteLocal,
    DeleteRemote,
    Download,
    Skip,
    Upload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteSyncPhase {
    RemoteHydration,
    LocalPublication,
}

pub fn plan_file_sync(
    local_hash: Option<&str>,
    remote_identity: Option<&str>,
    manifest: Option<&SyncManifestEntry>,
) -> FileSyncAction {
    match (local_hash, remote_identity) {
        (Some(local), None) => match manifest {
            Some(manifest) if local == manifest.local_hash => FileSyncAction::DeleteLocal,
            _ => FileSyncAction::Upload,
        },
        (None, Some(remote)) => match manifest {
            Some(manifest) if remote == manifest.remote_identity => FileSyncAction::DeleteRemote,
            _ => FileSyncAction::Download,
        },
        (None, None) => FileSyncAction::Skip,
        (Some(local), Some(remote)) => {
            let Some(manifest) = manifest else {
                return FileSyncAction::Conflict;
            };
            match (
                local != manifest.local_hash,
                remote != manifest.remote_identity,
            ) {
                (false, false) => FileSyncAction::Skip,
                (true, false) => FileSyncAction::Upload,
                (false, true) => FileSyncAction::Download,
                (true, true) => FileSyncAction::Conflict,
            }
        }
    }
}

pub fn plan_incomplete_sync(
    local_hash: Option<&str>,
    remote_identity: Option<&str>,
    partial: Option<&SyncManifestEntry>,
) -> FileSyncAction {
    match (local_hash, remote_identity) {
        (Some(_), None) => FileSyncAction::Upload,
        (None, Some(_)) => FileSyncAction::Download,
        (None, None) => FileSyncAction::Skip,
        (Some(local), Some(remote)) => match partial {
            Some(entry) if entry.local_hash == local && entry.remote_identity == remote => {
                FileSyncAction::Skip
            }
            _ => FileSyncAction::Conflict,
        },
    }
}

pub fn ordered_first_sync_actions(
    planned: BTreeMap<String, FileSyncAction>,
) -> Vec<(RemoteSyncPhase, String, FileSyncAction)> {
    let mut actions = planned
        .into_iter()
        .map(|(path, action)| {
            let phase = match action {
                FileSyncAction::Conflict | FileSyncAction::Download => {
                    RemoteSyncPhase::RemoteHydration
                }
                FileSyncAction::DeleteLocal
                | FileSyncAction::DeleteRemote
                | FileSyncAction::Skip
                | FileSyncAction::Upload => RemoteSyncPhase::LocalPublication,
            };
            (phase, path, action)
        })
        .collect::<Vec<_>>();
    actions.sort_by(|left, right| {
        first_sync_action_rank(left.2)
            .cmp(&first_sync_action_rank(right.2))
            .then_with(|| left.1.cmp(&right.1))
    });
    actions
}

fn first_sync_action_rank(action: FileSyncAction) -> u8 {
    match action {
        FileSyncAction::Conflict | FileSyncAction::Download => 0,
        FileSyncAction::Skip => 1,
        FileSyncAction::Upload => 2,
        FileSyncAction::DeleteLocal | FileSyncAction::DeleteRemote => 3,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        ordered_first_sync_actions, plan_file_sync, plan_incomplete_sync, FileSyncAction,
        RemoteSyncPhase,
    };
    use crate::sync::repository::SyncManifestEntry;

    fn baseline() -> SyncManifestEntry {
        SyncManifestEntry {
            local_hash: "local-old".to_string(),
            remote_identity: "remote-old".to_string(),
        }
    }

    #[test]
    fn complete_baseline_plans_the_full_three_way_matrix() {
        let cases = [
            (Some("local"), None, None, FileSyncAction::Upload),
            (None, Some("remote"), None, FileSyncAction::Download),
            (None, None, None, FileSyncAction::Skip),
            (
                Some("local"),
                Some("remote"),
                None,
                FileSyncAction::Conflict,
            ),
            (
                Some("local-old"),
                None,
                Some(baseline()),
                FileSyncAction::DeleteLocal,
            ),
            (
                None,
                Some("remote-old"),
                Some(baseline()),
                FileSyncAction::DeleteRemote,
            ),
            (
                Some("local-old"),
                Some("remote-old"),
                Some(baseline()),
                FileSyncAction::Skip,
            ),
            (
                Some("local-new"),
                Some("remote-old"),
                Some(baseline()),
                FileSyncAction::Upload,
            ),
            (
                Some("local-old"),
                Some("remote-new"),
                Some(baseline()),
                FileSyncAction::Download,
            ),
            (
                Some("local-new"),
                Some("remote-new"),
                Some(baseline()),
                FileSyncAction::Conflict,
            ),
        ];

        for (local, remote, manifest, expected) in cases {
            assert_eq!(plan_file_sync(local, remote, manifest.as_ref()), expected);
        }
    }

    #[test]
    fn incomplete_baseline_never_infers_a_deletion() {
        assert_eq!(
            plan_incomplete_sync(Some("local-old"), None, Some(&baseline())),
            FileSyncAction::Upload
        );
        assert_eq!(
            plan_incomplete_sync(None, Some("remote-old"), Some(&baseline())),
            FileSyncAction::Download
        );
        assert_eq!(
            plan_incomplete_sync(Some("local-old"), Some("remote-old"), Some(&baseline())),
            FileSyncAction::Skip
        );
        assert_eq!(
            plan_incomplete_sync(Some("local-new"), Some("remote-old"), Some(&baseline())),
            FileSyncAction::Conflict
        );
    }

    #[test]
    fn first_sync_hydrates_remote_content_before_local_publication() {
        let planned = BTreeMap::from([
            ("upload.md".to_string(), FileSyncAction::Upload),
            ("download.md".to_string(), FileSyncAction::Download),
            ("conflict.md".to_string(), FileSyncAction::Conflict),
            ("skip.md".to_string(), FileSyncAction::Skip),
        ]);

        let actions = ordered_first_sync_actions(planned);

        assert_eq!(
            actions,
            vec![
                (
                    RemoteSyncPhase::RemoteHydration,
                    "conflict.md".to_string(),
                    FileSyncAction::Conflict,
                ),
                (
                    RemoteSyncPhase::RemoteHydration,
                    "download.md".to_string(),
                    FileSyncAction::Download,
                ),
                (
                    RemoteSyncPhase::LocalPublication,
                    "skip.md".to_string(),
                    FileSyncAction::Skip,
                ),
                (
                    RemoteSyncPhase::LocalPublication,
                    "upload.md".to_string(),
                    FileSyncAction::Upload,
                ),
            ]
        );
    }
}
