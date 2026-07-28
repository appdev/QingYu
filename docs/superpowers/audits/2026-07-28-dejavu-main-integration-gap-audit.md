# Dejavu main integration gap audit

**Date:** 2026-07-28
**Audited commit:** `a89211ee6a645a4627a5cf91a93528dca717dd73`
**Branch:** `codex/dejavu-full-recovery`
**Integrated main:** `453069dc03783a7f5cf0c30751bd51a4d726091d`
**Recovery baseline:** `33b14ec30a55d38f1f0702baf487995c8caf2509`

## Scope and method

This is a document-to-code audit, not an implementation-completion claim. It
compares the merged tree against:

- `docs/superpowers/specs/2026-07-25-qingyu-dejavu-s3-sync-rust-port-design.md`;
- the four `2026-07-25-qingyu-dejavu-*.md` implementation plans;
- `apps/desktop/src-tauri/crates/qingyu-dejavu/UPSTREAM.md`;
- the 2026-07-28 full-recovery design in the primary checkout.

The old plans' unchecked boxes and historical commits were used only to find
requirements. A row is `present` only when the merged tree contains a concrete
implementation or test anchor. Negative findings were checked in the current
tree and against the actual `sync_application` dispatch path.

Classification:

- `present`: required behavior and a concrete code or test anchor exist;
- `adapted`: the approved behavior exists through a current-main replacement;
- `missing`: required behavior is absent or the production route still bypasses it;
- `obsolete`: a historical verification event cannot certify the merged HEAD and
  is superseded by the 2026-07-28 verification sequence.

This matrix has 38 unique requirement rows: **33 present, 1 adapted, 3 missing,
and 1 obsolete**. Repeated requirements in the four plans map back to these
rows, so they are not double-counted.

## Requirement matrix

| ID | Requirement and document source | Classification | Merged code/test evidence |
| --- | --- | --- | --- |
| R01 | Pinned Dejavu/SiYuan provenance, license attribution, and stable public crate boundary. Design “规范来源和优先级”; core plan Task 1; `UPSTREAM.md`. | `present` | `apps/desktop/src-tauri/crates/qingyu-dejavu/UPSTREAM.md`; `src/lib.rs` constants `UPSTREAM_DEJAVU_COMMIT` and `UPSTREAM_SIYUAN_COMMIT`; `tests/provenance.rs::source_baselines_are_pinned`; crate `Cargo.toml` license metadata. |
| R02 | Four-layer architecture with a Tauri-independent core crate and application adapters. Design “总体架构”; core plan Goal. | `present` | Core modules under `apps/desktop/src-tauri/crates/qingyu-dejavu/src/`; product adapters under `apps/desktop/src-tauri/src/dejavu_sync/`; frontend integration under `packages/app/src/`. The core crate has no Tauri dependency. |
| R03 | Exact entities, Go JSON names, SHA-1 IDs, deterministic diffs, refs/index/check-index repository format. Design “忠实移植的内容”; core plan Task 2. | `present` | `entity.rs::{File,Chunk,Index,CheckIndex,MergeResult}`; `diff.rs::{diff_upsert_remove,diff_index_files}`; tests `file_id_matches_dejavu_path_plus_second_timestamp`, `index_json_uses_go_field_names`, and `diff_results_are_sorted_by_path`. |
| R04 | Scrypt key derivation, AES-256-GCM nonce-prefix layout, zstd framing, encrypted File/Chunk objects, zstd-only indexes. Design “密钥与设备身份” and repository-format boundary; core plan Task 3. | `present` | `crypto.rs::{derive_key,encrypt,decrypt}` and tests `kdf_is_scrypt_32768_8_1_32`, `decrypts_pinned_go_aes_gcm_fixture`; `store.rs::Store` and tests `rust_store_round_trips_all_four_object_types`, `indexes_are_zstd_only_while_data_objects_are_encrypted`. |
| R05 | Restic-compatible Rabin chunking, all ordinary files, hidden/temp/non-regular filtering, `.qingyu/syncignore`, no empty-directory objects. Design “同步数据范围”; core plan Task 4. | `present` | `chunker.rs::RabinChunker` with `boundaries_match_the_pinned_restic_go_oracle`; `indexer.rs::index_once` with `built_in_rules_ignore_hidden_tmp_symlink_non_regular_and_empty_entries` and `protected_include_crosses_hidden_and_user_ignored_ancestors`; `repository.rs::prepare_syncignore`. |
| R06 | Safe refs, checkout, conflict history, latest/latest-sync, and reachability primitives. Design “同步状态机” and “冲突行为”; core plan Task 5. | `present` | `ref_store.rs::RefStore` and tests `malformed_and_oversized_refs_are_rejected_instead_of_ignored`, `atomic_ref_publication_failure_preserves_the_obstruction_and_cleans_its_temp`; `repo.rs::{checkout_file,checkout_files}` with `interrupted_checkout_preserves_the_existing_target_and_removes_temp`; `history.rs::History::store_remote_conflict`. |
| R07 | Complete Dejavu sync/download state machine, first-sync empty baseline, asymmetric seven-minute rule, ordinary-file local retention, and publication ordering. Design “Dejavu 同步状态机” and “冲突行为”; core plan Task 7. | `present` | `sync.rs::{Repo::sync,Repo::sync_download}`; tests `seven_minute_filters_are_strict_and_asymmetric`, `remote_ref_failure_does_not_advance_either_local_ref`, `restart_converges_after_object_index_and_ref_publication_failures`, and `sequence_ref_is_visible_before_stale_sequence_cleanup`; shared scenario runner in `tests/scenarios.rs`. |
| R08 | One local directory per repository, outer `qingyu/repositories/<uuid>/metadata.json`, inner Dejavu `repo/`, catalog create/list/read/rename/delete, no machine paths or secrets in metadata. Design “仓库与目录模型”; S3 plan Task 4. | `present` | `catalog.rs::{RepositoryMetadata,S3RepositoryCatalog}` and methods `create`, `list`, `read`, `rename`, `delete_repository`; HTTP tests `catalog_metadata_serde_contract_is_exact_and_denies_unknown_fields`, `catalog_list_uses_delimited_direct_prefixes_sorts_and_reports_safe_typed_issues`, and `catalog_delete_rejects_cross_prefix_listing_before_any_delete`; product list adapter `dejavu_sync/repository.rs::list_s3_repository_catalog`. |
| R09 | No QingYu user-file/repository capacity caps; bounded refs/locks/chunks/metadata; streamed staged File/Index/CheckIndex transport and task-owned temp cleanup. Design “容量与资源边界”; S3 plan Task 2. | `present` | `cloud/mod.rs::Cloud::{get_bounded,download_to,upload_from}`; `repo.rs::{create_staged_download,download_raw_to_store}` with `dropping_a_staged_download_removes_its_partial_task_owned_file`; `store.rs::import_raw_staged_unlocked`; `s3_http.rs` tests `cloud_get_is_bounded_at_max_plus_one_and_maps_404_to_not_found`, `cloud_download_streams_once_and_never_appends_a_retry_after_partial_body`, and `cloud_upload_prehashes_and_reopens_the_source_for_every_retry`. |
| R10 | SigV4, secret redaction, true zero-byte bodies, and blank S3 region normalized to `auto`. Design resource/privacy boundary; S3 plan Task 1; explicit recovery defect audit. | `present` | `cloud/s3_signing.rs::S3Connection::new` trims region and stores `"auto"` when blank; the production adapter calls it in `dejavu_sync/repository.rs::s3_transport`; `tests/s3_http.rs::signing_blank_region_uses_auto_and_query_parameters_are_sorted`; UI keeps an empty stored field with an `auto` placeholder in `SyncSettings.tsx` and `SyncSettings.test.tsx::keeps_an_empty_S3_region_while_showing_the_automatic_runtime_value`. |
| R11 | S3 key confinement, bounded XML, retry classification, streamed upload/download, no conditional writes, and unknown capacity. S3 plan Task 3. | `present` | `cloud/s3.rs::{S3Cloud,S3TransportOptions}`; `cloud/s3_xml.rs`; tests in `tests/s3_http.rs`, including `cloud_put_uses_plain_no_cache_put_for_both_overwrite_values_and_true_empty_body`, `cloud_download_streams_once_and_never_appends_a_retry_after_partial_body`, paginated/cross-prefix XML tests, and non-retry authentication tests. |
| R12 | Dejavu `lock-sync`, three acquire/release attempts, 30-second refresh, 65-second stale rule, latest sequence refs and object/index/ref order. Design “仓库锁”; core plan Task 6; S3 plan Task 5. | `present` | `sync_lock.rs::{acquire_remote_lock,RemoteLockGuard}` with tests `fresh_other_device_lock_gets_three_attempts_and_three_five_second_waits`, `refresh_runs_every_thirty_seconds_and_explicit_release_stops_it`, `release_succeeds_on_the_third_remove_attempt`; `sync.rs` sequence tests; `tests/s3_http.rs::full_sync_request_order_preserves_lock_and_sequence_publication_over_s3`. |
| R13 | Local-only global repository key, stable device ID, per-repository bindings, duplicate rejection, atomic `local-sync.json`, and key redaction. Design “密钥与设备身份”; background plan Task 1. | `present` | `dejavu_sync/local_state.rs::{LocalSyncState,RepositoryBinding,LocalSyncStateService}`; tests `absent_state_initializes_versioned_local_identity_and_pretty_json`, `adding_bindings_rejects_duplicate_repository_ids_and_note_roots`, `saving_replaces_the_prior_state_atomically`, `debug_output_redacts_the_repository_key_and_device_id`. |
| R14 | Local layout `<app-data>/sync/repositories/<id>/{repo,history,temp,state.json}` and notes root containing only user data plus `.qingyu/syncignore`. Design “本地布局”; background plan Task 3. | `present` | `dejavu_sync/repository.rs::prepare_repository_layout`; test `bind_and_sync_uses_exact_layout_config_semantics_and_one_core_sync_index` asserts `repo`, `history`, `temp`, one core index, and the protected syncignore; status path is owned by `RepositoryStatusStore`. |
| R15 | One `bind_and_sync` workflow for enable, current-directory restore, empty/nonempty local/remote, and same-path conflicts. Design “统一的绑定、恢复与首次同步”; background plan Task 6. | `present` | `DejavuRepositoryRunner::bind_and_sync`; command `bind_dejavu_repository`; matrix tests `bind_and_sync_matrix_accepts_empty_local_and_empty_remote`, `...uploads_nonempty_local_to_empty_remote`, `...restores_nonempty_remote_into_empty_local`, `...merges_independent_nonempty_local_and_remote`, and `...keeps_local_same_path_conflict_and_remote_history`. The UI supplies its already selected `primaryRoot`, matching the design's “select/create a directory, then bind” sequence. |
| R16 | Window-independent accepted jobs, one running operation per repository, cross-repository concurrency, and a global key barrier. Design “后台同步”; background plan Task 3. | `present` | `dejavu_sync/service.rs::DejavuSyncService`; tests `enqueue_returns_acceptance_while_the_repository_run_is_blocked`, `dropping_the_enqueue_caller_does_not_cancel_an_accepted_job`, `accepted_jobs_for_one_repository_run_serially`, `accepted_jobs_for_different_repositories_run_concurrently`, and `global_key_write_barrier_waits_for_all_accepted_repository_jobs`. |
| R17 | Authoritative per-repository `state.json`, persist-before-event, schedule/maintenance preservation, safe errors and no absolute notes root. Design “状态、错误和隐私”; background plan Task 3. | `present` | `dejavu_sync/status.rs::{RepositorySyncStatus,RepositoryStatusStore,TauriRepositoryStatusEmitter}`; tests `status_is_atomically_persisted_before_the_same_public_payload_is_emitted`, `persisted_and_emitted_status_omit_the_machine_absolute_notes_root`, `schedule_fields_are_persisted_before_emit_and_survive_sync_status_updates`, and `maintenance_survives_attempting_succeeded_failed_and_schedule_writes`. |
| R18 | Automatic/startup-exit/manual modes, 30–43,200 seconds, exact no-change backoff, five-minute failures, eighth-failure pause, typed DNS retry, active-root-only timer, launch/exit triggers. Design “调度与重试”; background plan Tasks 2 and 4. | `present` | `sync_config/model.rs::{SyncMode,SYNC_CONFIG_VERSION}`; `dejavu_sync/scheduler.rs` tests `modes_accept_only_the_siyuan_trigger_matrix`, `no_change_backoff_is_exact_and_resets_the_eleventh_count_to_five`, `ordinary_failures_wait_five_minutes_and_eighth_automatic_failure_waits_sixty_four`, `the_single_timer_polls_only_the_current_active_root`, and `dns_retry_is_throttled_per_repository_for_five_minutes`; `watcher.rs` and `app_exit.rs` wiring. |
| R19 | Repository-scoped sync/maintenance admission, unrelated repositories concurrent, global key replacement serialized, and maintenance admission established before spawn polling. Design “本地锁”; background plan Tasks 3 and 7. | `present` | `DejavuSyncService::{begin_repository_bind,reserve_repository_maintenance,reserve_global_maintenance,with_global_key_state_transaction}`; `maintenance.rs::LocalMaintenanceController`; regression `purge_execution_runs_inside_the_injected_repository_transaction`; service tests for reservation rejection and cross-repository concurrency. |
| R20 | Coordinate only affected working-tree paths, flush matching dirty tabs, revalidate snapshot identity, block intersecting mutations, always release, and leave unrelated paths usable. Design “后台同步与实时编辑协调”; core plan Tasks 6–7; background plan Task 5. | `present` | Core `working_tree.rs::{WorkingTreeCoordinator,WorkingTreeChange,with_working_tree_permit}` and release tests; Tauri `dejavu_sync/path_guard.rs::PathGuardCoordinatorFactory` with `owner_ack_uses_canonical_identity_and_releases_exactly_once`, `timeout_and_listener_loss_apply_nothing_and_cleanup_release`, `prepare_blocks_new_intersections_and_waits_only_for_affected_native_mutations`; frontend `useSyncPathGuard`, `useMarkdownDocument::saveDirtyMarkdownPaths`, and tests `flushes_only_exact_requested_dirty_paths_and_leaves_unrelated_tabs_dirty` and `re-saves_an_edit_made_while_an_exact_path_flush_is_awaiting_native_I/O`. |
| R21 | Safer-write order, same-directory private stage file, file sync, mode, replace, Windows bounded retry, and cleanup of only owned temp files. Design “安全写入”; core plan Task 3. | `present` | `atomic_write.rs::{write_file_safer,write_cap_file_safer,is_owned_stage_name}`; tests `safer_write_replaces_destination_and_leaves_no_owned_temp`, `safer_write_failure_removes_only_its_own_temp`, `owned_stage_name_requires_exact_lowercase_sha1_grammar`, and Unix mode coverage. |
| R22 | Startup cleanup only for exact owned `stage-<sha1>.tmp` parents; no arbitrary `*.tmp`, invented `temp/repo`, WAL, or remote full-scan recovery; next run converges after publication interruption. Design “启动恢复”; background plan Task 7. | `present` | `maintenance.rs::clean_startup_residue` and test `startup_cleanup_removes_only_direct_owned_stage_entries_from_owned_parents`; `Repo::open` uses the same owned-stage predicate; `sync.rs::restart_converges_after_object_index_and_ref_publication_failures`; `tests/s3_http.rs` failure-before-publication coverage; interop interruption scenarios in `scripts/test-dejavu-interop.mjs`. |
| R23 | Local 180-day/today/all/older-day-random retention, minimum three indexes, first-success and six-hour/daily/12-hour limits, 30-day conflict history, and explicit locked remote purge. Design “清理与保留”; core plan Task 5; background plan Task 7. | `present` | `purge.rs::{Repo::purge_cloud,purge_store_with_cancel_check}` with reachability, cancellation, lock-loss, indexes-v2 and idempotence tests; `maintenance.rs::{select_retained_indexes,LocalMaintenanceController,clean_expired_conflict_history}` with tests `retained_indexes_skip_purge_below_three_unique_ids`, `first_successful_non_exit_completion_starts_detached_maintenance`, `repeated_completion_deduplicates_and_exact_six_hours_is_due`, `twelve_hour_timeout_sets_the_executor_flag_and_waits_for_cooperative_exit`, and conflict-history cleanup. |
| R24 | Rebuild local repository, stop sync, change global key, remote purge, and remote delete are distinct accepted operations with documented targets and lock order. Design “重置和删除操作”; background plan Task 7. | `present` | `dejavu_sync/lifecycle.rs::RepositoryLifecycleController` methods `rebuild_local_repository`, `stop_repository_sync`, `change_global_key`, `purge_remote_repository`, `delete_remote_repository`; tests `rebuild_returns_an_accepted_background_job_with_a_stable_public_shape`, `stop_waits_in_the_repository_lane_then_removes_only_that_binding_and_schedule`, `key_change_clears_all_old_key_repositories_then_preserves_identity_and_disables_bindings`, and `remote_delete_rejects_an_enabled_binding_but_allows_a_disabled_one`. |
| R25 | Persist unresolved conflicts, keep backend paths private, read local/remote versions safely, suppress only the public `.qingyu/syncignore` record, and support exactly keep-local/use-remote/keep-both. Design “冲突行为”; conflict plan Tasks 1–2. | `present` | `dejavu_sync/conflicts.rs::{SyncConflictRecord,ConflictStore,ConflictResolver,ConflictResolution}`; repository mapping filters exact `/.qingyu/syncignore` before public record creation; tests `missing_history_and_another_repository_are_safely_unavailable`, `oversized_and_binary_versions_report_size_without_loading_text`, `keep_local_marks_the_conflict_without_touching_the_working_tree`, and `keep_both_writes_remote_to_an_absent_destination_and_rejects_traversal`. |
| R26 | One-time non-blocking conflict notice, exact active-file marker, user-opened comparison dialog, and no automatic Markdown merge. Design “轻语用户流程”; conflict plan Tasks 3–4. | `present` | `useSyncConflicts.ts` and tests `loads_persisted_conflicts_without_replaying_old_notices_and_matches_only_the_active_file`, `notifies_once_for_a_newly_emitted_id_and_drops_it_after_accepted_resolution`; `SyncConflictIndicator.tsx`; `SyncConflictDialog.tsx` and tests for the exact three resolutions; `App.tsx` renders the indicator beside the CodeMirror editor without changing read-only state. |
| R27 | Settings expose scheduling, key import/export/change, lifecycle controls, manual remote purge, and unresolved conflict paths. Conflict plan Task 5 excluding the status subsection. | `present` | `packages/app/src/components/settings/SyncSettings.tsx` renders mode/interval, key, rebuild/stop/purge/delete, and conflict paths; `SyncSettings.test.tsx::imports_a_local_key_and_dispatches_repository_maintenance_as_accepted_background_work`; typed runtime commands in `packages/app/src/lib/sync-config.ts` and `apps/desktop/src/runtime/tauri/sync-config/shared.ts`. |
| R28 | Settings visibly show Dejavu background phase, trigger, attempt/success times, next schedule, transfer totals, safe error, and conflict count/path while a job continues. Design “产品界面层” and “状态、错误和隐私”; conflict plan Task 5; explicit recovery defect audit. | `missing` | Backend and TypeScript data exist (`RepositorySyncStatus`; `DejavuRepositoryStatus`), and `SyncSettings.tsx` loads/listens to them. However the component only reads `repositoryId` and `conflicts`; it never renders `dejavuRepositoryStatus.phase`, `trigger`, `lastAttemptAt`, `lastSuccessfulSyncAt`, `nextScheduledAt`, `transfer`, `error`, or `maintenance`. The visible `SyncStatusSummary` is the legacy `SyncStatus` surface. `SyncSettings.test.tsx` supplies those Dejavu fields but asserts only key/maintenance behavior. See repair M1. |
| R29 | S3 restore uses provider-tagged catalog entries, returns after accepted background binding, closes only the restore dialog, keeps Settings mounted, and leaves failures retryable. Design restore/window behavior; conflict plan Task 6. | `present` | `remote_sync/catalog.rs::list_remote_notebooks` dispatches S3 to `list_s3_repository_catalog` and returns repository ID/display name; `useSettingsRemoteNotebookDialog.ts` sends `notesRoot`, repository ID and display name and resumes after acceptance; tests `accepts_an_S3_repository_binding_for_the_selected_local_root_and_resumes_immediately`, `keeps_a_failed_restore_open_and_retryable`, and `App.test.tsx::accepts_a_settings-owned_S3_repository_binding_without_waiting_for_sync_completion`. |
| R30 | Exact four upstream fixture hashes and all 27 Go/Rust shared JSON scenarios. Design “Go 行为基线/Rust 场景运行器”; core plan Task 8; `UPSTREAM.md`. | `present` | Four fixtures under `tests/fixtures/dejavu/cases/`; exact SHA-256 table in `UPSTREAM.md`; `tests/scenarios.rs`; `scripts/test-dejavu-oracle.mjs` pins and verifies commit/fixtures before Go and Rust tests; package script `test:dejavu-oracle`. |
| R31 | Bidirectional Go/Rust repository interoperability including both creation directions, independent paths, same-path conflict, protected syncignore oracle, and both languages interrupted before ref publication. Design “跨语言测试”; S3 plan Task 7. | `present` | `scripts/test-dejavu-interop.mjs` declares seven scenarios, including `go-failure-before-ref-publication` and `rust-failure-before-ref-publication`; Rust CLI `src/bin/dejavu-interop.rs`; pinned Go CLI `scripts/dejavu-interop-go/main.go`; package script `test:dejavu-interop`. |
| R32 | Opt-in real S3/MinIO coverage, unique repository prefix, exact cleanup, merge/conflict/lock scenarios, and live-script integration without committed credentials. Design verification/live boundary; S3 plan Task 6. | `present` | `tests/s3_minio.rs::dejavu_s3_sync_round_trips_through_real_minio`, `exercise_lock_contention`, `cleanup_exact_repository`; `scripts/test-s3-sync-live.mjs::runAllLiveS3Tests` runs both legacy live tests and `-p qingyu-dejavu --test s3_minio` with `QINGYU_S3_LIVE_TESTS=1`; orchestration tests in `packages/scripts/src/test-s3-sync-live.test.mjs`. This audit did not contact a live server. |
| R33 | Ordinary S3 note synchronization must dispatch only to `DejavuSyncService::enqueue`; WebDAV remains legacy; S3 portable settings remain on their protected legacy settings scope; frontend understands accepted vs completed. Design S3 cutover acceptance; conflict plan Task 7. | `missing` | `apps/desktop/src-tauri/src/sync_config.rs::sync_application` still returns only `SyncRunResult`; `remote_sync/service.rs::run_application_sync_inner` constructs legacy `S3Backend` values for both notes and settings and calls `build_sync_scopes`; `build_sync_scopes` constructs `RemoteSyncScope::notes(..., "manifest.json", ...)`; `apps/desktop/src/runtime/tauri/sync-config/shared.ts::syncApplication` invokes `sync_application` as `SyncRunResult`; `useAppSyncCoordinator.ts` has no accepted-job branch. See repair M2. |
| R34 | After cutover, remove only unreachable legacy S3 note constructors/catalog/tests/live fixtures and add a static boundary that S3 notes cannot reach manifest or `remote-conflict` code. Conflict plan Task 8. | `missing` | No production routing-boundary test exists. The current S3 note route still reaches `RemoteSyncScope::notes` and `manifest.json`; `remote_sync/engine.rs` still contains `MANIFEST_VERSION` and `remote_conflict_file_name`, and they remain reachable for S3 notes through `run_application_sync_inner`. Removal cannot safely start before R33. See repair M3. |
| R35 | Preserve WebDAV behavior and portable `settings.json` synchronization while changing only S3 note data. Design “本期不包含/验收条件”; conflict plan Tasks 7–8. | `present` | `remote_sync/service.rs` retains the WebDAV branch and `prepare_portable_settings_sync`; `RemoteSyncScope::portable_settings` remains isolated from notes; focused portable-settings durability and legacy-MCP sanitation tests remain. R33 must preserve these anchors. |
| R36 | Merging `main` must not reintroduce removed AI/Agent/provider, spellcheck, proxy/Network/SOCKS, or theme-export product surfaces. 2026-07-28 recovery design “Isolation and integration strategy”. | `present` | `git diff --name-status main..HEAD` is empty for known removed feature paths (`packages/ai`, AI settings/components/runtime, spellcheck modules). Current `rg --files` has none of the deleted feature modules. The branch adds only Dejavu-related files for names matching the audit. The current tree retains ordinary theme selection and HTML `spellCheck={false}` attributes, not the removed product features. |
| R37 | Preserve current-main V2 CodeMirror behavior while integrating Dejavu path guards and conflict UI; old Milkdown-specific implementation locations are no longer authoritative. Recovery design main-conflict rule. | `adapted` | `packages/editor/package.json` and `packages/editor/src/codemirror/` use CodeMirror 6; no Milkdown dependency remains. `App.tsx` uses `useCodeMirrorEditorController`, installs `useSyncPathGuard`, derives per-path read-only state, and renders `SyncConflictIndicator`; `useMarkdownDocument::saveDirtyMarkdownPaths` supplies the editor flush boundary. This is the approved V2 replacement of the old editor integration shape. |
| R38 | Historical milestone/full-suite verification tasks prove the current merged branch. Core plan Task 9; S3 plan Task 8; background plan Task 8; conflict plan Task 8 verification steps. | `obsolete` | Pre-merge historical pass states cannot certify `a89211e`. The 2026-07-28 recovery design explicitly supersedes them with a new focused-repair, full Rust/frontend/build, Go oracle, seven-scenario interop, live S3, and desktop Computer Use sequence. This audit intentionally runs only focused evidence checks. |

## Implementation-plan task coverage index

This index ensures every implementation-plan task is represented without using
its checkbox state as evidence.

| Plan task | Classification | Requirement rows |
| --- | --- | --- |
| Core Task 1: crate/provenance | `present` | R01–R02 |
| Core Task 2: entities/diffs | `present` | R03 |
| Core Task 3: crypto/store/safer write | `present` | R04, R21 |
| Core Task 4: chunking/ignore/index | `present` | R05 |
| Core Task 5: refs/checkout/history/purge | `present` | R06, R23 |
| Core Task 6: cloud/working tree/remote lock | `present` | R11–R12, R20 |
| Core Task 7: complete state machine | `present` | R07 |
| Core Task 8: 27 Go/Rust scenarios | `present` | R30 |
| Core Task 9: old milestone verification | `obsolete` | R38 |
| S3 Task 1: SigV4 | `present` | R10 |
| S3 Task 2: bounded/staged transfers | `present` | R09 |
| S3 Task 3: S3 operations | `present` | R11 |
| S3 Task 4: repository catalog | `present` | R08 |
| S3 Task 5: lock/sequence publication | `present` | R12 |
| S3 Task 6: real MinIO | `present` | R32 |
| S3 Task 7: mixed Go/Rust | `present` | R31 |
| S3 Task 8: old milestone verification | `obsolete` | R38 |
| Background Task 1: local key/device/bindings | `present` | R13 |
| Background Task 2: version-3 scheduling contract | `present` | R18, R27 |
| Background Task 3: background service/status | `present` | R14, R16–R17 |
| Background Task 4: scheduler/backoff/DNS | `present` | R18 |
| Background Task 5: affected-path coordination | `adapted` | R20, R37 |
| Background Task 6: unified bind/restore | `present` | R15, R29 |
| Background Task 7: recovery/retention/lifecycle | `present` | R19, R21–R24 |
| Background Task 8: old milestone verification | `obsolete` | R38 |
| Conflict Task 1: conflict records | `present` | R25 |
| Conflict Task 2: three resolutions | `present` | R25 |
| Conflict Task 3: notice/current-file marker | `present` | R26 |
| Conflict Task 4: comparison/resolution dialog | `present` | R26 |
| Conflict Task 5: settings controls and status | `missing` | R27 present; R28 missing |
| Conflict Task 6: accepted background restore | `present` | R29 |
| Conflict Task 7: S3 cutover | `missing` | R33; preservation half R35 is present |
| Conflict Task 8: legacy removal/final acceptance | `missing` | R34; verification half is superseded by R38 |

## Focused repair and test tasks

### M1 — Render the authoritative Dejavu background status

**Repair:** Add a dedicated Dejavu status summary in
`packages/app/src/components/settings/SyncSettings.tsx`. Render phase, trigger,
last attempt, last success, next scheduled time, transfer totals, safe error,
maintenance timing, and unresolved conflict count/paths from
`dejavuRepositoryStatus`. Keep the legacy `SyncStatusSummary` only for the
legacy/WebDAV surface until R33 is complete.

**Focused test:** Extend
`packages/app/src/components/settings/SyncSettings.test.tsx` with a running,
failed, and succeeded Dejavu status. Assert that a
`qingyu://dejavu-sync-status-changed` event updates the visible fields while
Settings remains mounted, and that no secret or absolute local path is rendered.

### M2 — Cut ordinary S3 notes over to the accepted Dejavu job route

**Repair:** Introduce the provider-tagged dispatch result described by the plan.
At `sync_application`, send S3 note work to the installed
`DejavuSyncService::enqueue` using the active binding and return
`{ status: "accepted", job }`; keep WebDAV note work as
`{ status: "completed", result }`. Extract and retain the existing S3 portable
settings journal/publication path so `settings.json` stays outside Dejavu. Update
the TypeScript runtime and `useAppSyncCoordinator` so acceptance ends only the
submission state; terminal success/failure comes from Dejavu status events.

**Focused tests:** Add Rust provider-routing tests proving:

1. S3 notes call only the Dejavu queue;
2. WebDAV notes call only the legacy notes engine;
3. S3 portable settings still use `RemoteSyncScope::portable_settings`;
4. an old S3 note manifest alone is neither read nor migrated;
5. a missing/disabled active binding returns a safe error before network work.

Add frontend coordinator tests for accepted versus completed dispatch and for a
terminal Dejavu failure event after acceptance.

### M3 — Remove the now-unreachable legacy S3 note path

**Repair:** Only after M2 is green, remove S3-specific legacy note constructors,
catalog branches, tests, and live fixtures proven unreachable. Retain WebDAV,
portable-settings journals, safe filesystem helpers, and any legacy engine
symbols still required by those paths.

**Focused test:** Add a static/structural boundary test asserting that the
production S3 note dispatch cannot reference `RemoteSyncScope::notes`,
`manifest.json`, legacy S3 note prefixes, or `remote_conflict_file_name`.
Retain behavior tests for WebDAV notes and S3/WebDAV portable settings.

## Focused command evidence

The audit used read-only source searches, branch comparisons, and focused tests;
it did not run the full repository, Go oracle, interoperability, live S3, or
desktop smoke suites.

Focused verification results are recorded here after execution:

- `pnpm --filter @markra/app exec vitest run
  src/components/settings/SyncSettings.test.tsx -t
  "keeps an empty S3 region|imports a local key"` — passed: 1 file, 2 tests
  passed, 24 skipped.
- `PATH=/Users/ying/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
  -p qingyu-dejavu --test s3_http
  signing_blank_region_uses_auto_and_query_parameters_are_sorted -- --exact`
  — passed: 1 test.
- `PATH=/Users/ying/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
  dejavu_sync::service::tests::enqueue_returns_acceptance_while_the_repository_run_is_blocked
  -- --exact` — passed: 1 test.
- `PATH=/Users/ying/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH
  cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib
  dejavu_sync::maintenance::tests::purge_execution_runs_inside_the_injected_repository_transaction
  -- --exact` — passed: 1 test.
- The first shell invocation of the three Cargo checks found no `cargo` on
  `PATH` and did not start tests. The commands above are the successful retries
  with the repository's stable Rust toolchain path.
- The matrix count check reported `total=38 present=33 adapted=1 missing=3
  obsolete=1`; `git diff --check` passed.

## Audit conclusion

The merged branch contains the complete recovered Dejavu core, S3 transport,
catalog, local state, scheduler, conflict, path-guard, restore, maintenance,
oracle, interoperability, and live-test harnesses. The blank-region defect is
already fixed and tested. The merge also preserves the current-main hard-removal
boundary and CodeMirror V2 editor.

The integration is not ready for full verification because three product-route
gaps remain: the Dejavu background status is not rendered, ordinary S3 note
sync still uses the legacy manifest engine, and the legacy S3 note route has not
been made unreachable or removed. M1–M3 are the required repair/test sequence
before the 2026-07-28 full verification plan.
