use qingyu_kernel::config::KernelConfig;
use qingyu_kernel::contract::{
    ApiErrorEnvelope, ApiVersion, AuthenticateFrame, CreateDocumentRequest, CredentialChange,
    DeletionPolicy, DocumentContentDto, DocumentContents, DocumentId, DocumentKind, DocumentName,
    DocumentPageDto, DomainEvent, ErrorCode, ErrorDetails, EventSequence, FontFamilyValueDto,
    FrameErrorCode, HostProfile, InstanceId, ListWorkspaceInventoryQuery, LiveHealthResponse,
    LiveStatus, MoveDocumentRequest, Nullable, PageCursor, PageCursorContext, PageQuery,
    PatchSettingsRequest, PositiveSafeInteger, ProtocolVersion, ReadySequence, ReloadScope,
    ResourceEntryDto, ResourceId, ResourceKind, ResourceName, ResourceRefDto, Revision, Rfc3339Utc,
    SafeInteger, SafeUnsignedInteger, SearchMatchDto, SearchWorkspaceQuery, ServerFrame,
    SettingEntryDto, SettingKey, SettingValueDto, SnapshotRequired, StartupState,
    SyncConfigChangesDto, SyncMode, SyncProvider, SyncSafeErrorCategory, SyncSafeErrorCode,
    SyncSafeErrorDto, SyncSafeErrorOperation, SyncSafeHttpMethod, SyncSafeProviderErrorCode,
    ValidationField, ValidationIssueCode, ValidationIssueDto, ValidationIssues, WireIdentityKey,
    WorkspaceDto, WorkspaceGeneration, WorkspaceId, WorkspaceInventoryEntryDto,
    WorkspaceInventoryPageDto, WorkspaceReadiness, WorkspaceRelativePath,
};
use qingyu_kernel::error::{http_status_for_error_code, safe_error_envelope};
use serde_json::json;
use static_assertions::assert_not_impl_any;
use uuid::Uuid;

assert_not_impl_any!(qingyu_kernel::paths::WorkspaceRoot: serde::Serialize, serde::de::DeserializeOwned);
assert_not_impl_any!(qingyu_kernel::paths::InstanceDataRoot: serde::Serialize, serde::de::DeserializeOwned);
assert_not_impl_any!(qingyu_kernel::paths::CacheRoot: serde::Serialize, serde::de::DeserializeOwned);
assert_not_impl_any!(qingyu_kernel::config::KernelLaunchEpoch: serde::Serialize, serde::de::DeserializeOwned);
assert_not_impl_any!(qingyu_kernel::config::NativeLaunchCredential: Clone, serde::Serialize, serde::de::DeserializeOwned);

#[test]
fn live_health_response_keeps_its_public_deserialization_contract() {
    let response: LiveHealthResponse = serde_json::from_value(json!({
        "status": "live",
        "apiVersion": "v1"
    }))
    .unwrap();

    assert_eq!(response.status, LiveStatus::Live);
    assert_eq!(response.api_version, ApiVersion::V1);
    assert!(serde_json::from_value::<LiveHealthResponse>(json!({
        "status": "live",
        "apiVersion": "v1",
        "unknown": true
    }))
    .is_err());
}

#[test]
fn native_launch_credentials_are_independent_ephemeral_secrets() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    let first = KernelConfig::generate().expect("first kernel config");
    let second = KernelConfig::generate().expect("second kernel config");
    let credential = first.native_launch_credential();
    let exposed = credential.expose_secret();
    let decoded = URL_SAFE_NO_PAD
        .decode(exposed)
        .expect("base64url launch credential");

    assert_eq!(decoded.len(), 32);
    assert!(credential.matches(exposed));
    assert!(!credential.matches(&format!("{exposed}x")));
    assert_ne!(exposed, second.native_launch_credential().expose_secret());
    assert!(!format!("{credential:?}").contains(exposed));
    assert!(std::mem::needs_drop::<
        qingyu_kernel::config::NativeLaunchCredential,
    >());
}

#[test]
fn parent_generated_native_credentials_can_be_injected_and_strictly_reconstructed() {
    let credential = qingyu_kernel::config::NativeLaunchCredential::generate()
        .expect("parent launch credential");
    let encoded = credential.expose_secret().to_owned();
    let config = KernelConfig::generate_with_native_launch_credential(credential)
        .expect("child config with parent credential");

    assert!(config.native_launch_credential().matches(&encoded));
    assert!(
        qingyu_kernel::config::NativeLaunchCredential::from_secret(encoded.clone())
            .unwrap()
            .matches(&encoded)
    );
    for invalid in [
        String::new(),
        "short".to_owned(),
        format!("{encoded}="),
        "A".repeat(42),
    ] {
        assert!(
            qingyu_kernel::config::NativeLaunchCredential::from_secret(invalid).is_err(),
            "accepted a noncanonical or wrong-sized credential"
        );
    }
}

#[test]
fn ready_frame_requires_the_literal_snapshot_required_value() {
    let frame = ServerFrame::Ready {
        protocol_version: ProtocolVersion::new(1).unwrap(),
        connection_id: qingyu_kernel::contract::ConnectionId::new(Uuid::new_v4()),
        instance_id: InstanceId::new(Uuid::new_v4()),
        sequence: ReadySequence::new(0).unwrap(),
        snapshot_required: SnapshotRequired::required(),
    };

    assert_eq!(
        serde_json::to_value(frame).unwrap()["snapshotRequired"],
        json!(true)
    );
    assert!(serde_json::from_value::<ServerFrame>(json!({
        "type": "ready",
        "protocolVersion": 1,
        "connectionId": Uuid::new_v4(),
        "instanceId": Uuid::new_v4(),
        "sequence": 0,
        "snapshotRequired": false
    }))
    .is_err());
}

#[test]
fn sync_validation_fields_use_the_frozen_v1_names() {
    assert_eq!(
        serde_json::to_value(ValidationField::RemoteRoot).unwrap(),
        json!("remoteRoot")
    );
    assert_eq!(
        serde_json::to_value(ValidationField::IntervalSeconds).unwrap(),
        json!("intervalSeconds")
    );
}

#[test]
fn workspace_relative_path_round_trips_root_and_nested_paths_as_strings() {
    let root = WorkspaceRelativePath::parse("").expect("the empty string is the workspace root");
    let nested = WorkspaceRelativePath::parse("notes/daily.md")
        .expect("a slash-normalized nested path is valid");

    assert_eq!(root.as_str(), "");
    assert_eq!(nested.as_str(), "notes/daily.md");
    assert_eq!(serde_json::to_string(&root).unwrap(), "\"\"");
    assert_eq!(
        serde_json::to_string(&nested).unwrap(),
        "\"notes/daily.md\""
    );
}

#[test]
fn workspace_relative_path_rejects_noncanonical_or_escaping_inputs() {
    for invalid in [
        "/notes",
        r"\notes",
        r"C:\notes",
        "C:/notes",
        r"notes\daily.md",
        ".",
        "..",
        "notes/./daily.md",
        "notes/../daily.md",
        "notes//daily.md",
        "notes/",
        "notes/da\0ily.md",
        "notes/da\u{001f}ily.md",
    ] {
        assert!(
            WorkspaceRelativePath::parse(invalid).is_err(),
            "accepted unsafe path {invalid:?}"
        );
    }
}

#[test]
fn workspace_relative_path_deserialization_enforces_the_same_validation() {
    let nested: WorkspaceRelativePath =
        serde_json::from_str(r#""notes/daily.md""#).expect("valid wire path");

    assert_eq!(nested.as_str(), "notes/daily.md");
    assert!(serde_json::from_str::<WorkspaceRelativePath>(r#""notes/../secret.md""#).is_err());
}

#[test]
fn opaque_uuid_identifiers_are_canonical_wire_strings() {
    let value = "018f8f3e-1ca9-7f53-96b1-2d6415fb63a9";
    let workspace: WorkspaceId = serde_json::from_str(&format!(r#""{value}""#)).unwrap();
    let instance = InstanceId::new(Uuid::parse_str(value).unwrap());

    assert_eq!(workspace.as_uuid(), instance.as_uuid());
    assert_eq!(
        serde_json::to_string(&workspace).unwrap(),
        format!(r#""{value}""#)
    );
    assert!(serde_json::from_str::<WorkspaceId>(r#""not-a-uuid""#).is_err());
}

#[test]
fn revisions_are_non_empty_opaque_strings() {
    let revision = Revision::parse("rev:01HZZZZZZZZZZZZZZZZZZZZZZZ").unwrap();

    assert_eq!(revision.as_str(), "rev:01HZZZZZZZZZZZZZZZZZZZZZZZ");
    assert_eq!(
        serde_json::to_string(&revision).unwrap(),
        r#""rev:01HZZZZZZZZZZZZZZZZZZZZZZZ""#
    );
    assert!(Revision::parse("").is_err());
    assert!(serde_json::from_str::<Revision>(r#""""#).is_err());
}

#[test]
fn numeric_wire_values_reject_javascript_unsafe_integers() {
    const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

    assert_eq!(
        SafeUnsignedInteger::new(MAX_SAFE_INTEGER).unwrap().get(),
        MAX_SAFE_INTEGER
    );
    assert!(SafeUnsignedInteger::new(MAX_SAFE_INTEGER + 1).is_err());
    assert_eq!(
        SafeInteger::new(-(MAX_SAFE_INTEGER as i64)).unwrap().get(),
        -(MAX_SAFE_INTEGER as i64)
    );
    assert!(SafeInteger::new(-((MAX_SAFE_INTEGER as i64) + 1)).is_err());
    assert!(serde_json::from_str::<SafeUnsignedInteger>("9007199254740992").is_err());
}

#[test]
fn document_ids_reject_tampering_and_context_reuse() {
    let key = WireIdentityKey::generate().unwrap();
    let restarted_key = WireIdentityKey::generate().unwrap();
    let workspace = WorkspaceId::new(Uuid::new_v4());
    let other_workspace = WorkspaceId::new(Uuid::new_v4());
    let generation = WorkspaceGeneration::parse("generation-7").unwrap();
    let other_generation = WorkspaceGeneration::parse("generation-8").unwrap();
    let path = WorkspaceRelativePath::parse("notes/daily.md").unwrap();
    let document_id = key
        .issue_document_id(workspace, &generation, DocumentKind::File, &path)
        .unwrap();

    assert_eq!(
        key.verify_document_id(&document_id, workspace, &generation, DocumentKind::File,)
            .unwrap(),
        path
    );
    assert!(key
        .verify_document_id(
            &tampered_document_id(&document_id),
            workspace,
            &generation,
            DocumentKind::File,
        )
        .is_err());
    assert!(key
        .verify_document_id(
            &document_id,
            other_workspace,
            &generation,
            DocumentKind::File,
        )
        .is_err());
    assert!(key
        .verify_document_id(
            &document_id,
            workspace,
            &other_generation,
            DocumentKind::File,
        )
        .is_err());
    assert!(key
        .verify_document_id(
            &document_id,
            workspace,
            &generation,
            DocumentKind::Directory,
        )
        .is_err());
    assert!(restarted_key
        .verify_document_id(&document_id, workspace, &generation, DocumentKind::File,)
        .is_err());
}

#[test]
fn resource_ids_reject_tampering_context_reuse_and_document_token_substitution() {
    let key = WireIdentityKey::generate().unwrap();
    let restarted_key = WireIdentityKey::generate().unwrap();
    let workspace = WorkspaceId::new(Uuid::new_v4());
    let other_workspace = WorkspaceId::new(Uuid::new_v4());
    let generation = WorkspaceGeneration::parse("generation-7").unwrap();
    let other_generation = WorkspaceGeneration::parse("generation-8").unwrap();
    let path = WorkspaceRelativePath::parse("assets/photo.png").unwrap();
    let resource_id = key
        .issue_resource_id(workspace, &generation, ResourceKind::Image, &path)
        .unwrap();

    assert_eq!(
        key.verify_resource_id(&resource_id, workspace, &generation, ResourceKind::Image)
            .unwrap(),
        path
    );
    assert!(key
        .verify_resource_id(
            &tampered_resource_id(&resource_id),
            workspace,
            &generation,
            ResourceKind::Image,
        )
        .is_err());
    assert!(key
        .verify_resource_id(
            &resource_id,
            other_workspace,
            &generation,
            ResourceKind::Image,
        )
        .is_err());
    assert!(key
        .verify_resource_id(
            &resource_id,
            workspace,
            &other_generation,
            ResourceKind::Image,
        )
        .is_err());
    assert!(key
        .verify_resource_id(
            &resource_id,
            workspace,
            &generation,
            ResourceKind::Attachment,
        )
        .is_err());
    assert!(restarted_key
        .verify_resource_id(&resource_id, workspace, &generation, ResourceKind::Image)
        .is_err());

    let document_id = key
        .issue_document_id(workspace, &generation, DocumentKind::File, &path)
        .unwrap();
    let document_token_as_resource = ResourceId::parse(document_id.as_str()).unwrap();
    assert!(key
        .verify_resource_id(
            &document_token_as_resource,
            workspace,
            &generation,
            ResourceKind::Image,
        )
        .is_err());
}

#[test]
fn page_cursors_are_bound_to_operation_query_generation_and_launch_key() {
    let key = WireIdentityKey::generate().unwrap();
    let restarted_key = WireIdentityKey::generate().unwrap();
    let generation = WorkspaceGeneration::parse("generation-7").unwrap();
    let other_generation = WorkspaceGeneration::parse("generation-8").unwrap();
    let snapshot = vec!["notes/daily.md", "notes/kernel.md"];
    let context =
        PageCursorContext::new("searchWorkspace", "query=kernel", &generation, &snapshot).unwrap();
    let cursor = key.issue_page_cursor(&context, "notes/daily.md").unwrap();

    assert_eq!(
        key.verify_page_cursor(&cursor, &context).unwrap(),
        "notes/daily.md"
    );
    assert!(key
        .verify_page_cursor(
            &cursor,
            &PageCursorContext::new("listDocuments", "query=kernel", &generation, &snapshot)
                .unwrap(),
        )
        .is_err());
    assert!(key
        .verify_page_cursor(
            &cursor,
            &PageCursorContext::new("searchWorkspace", "query=other", &generation, &snapshot)
                .unwrap(),
        )
        .is_err());
    assert!(key
        .verify_page_cursor(
            &cursor,
            &PageCursorContext::new(
                "searchWorkspace",
                "query=kernel",
                &other_generation,
                &snapshot,
            )
            .unwrap(),
        )
        .is_err());
    assert!(key
        .verify_page_cursor(
            &cursor,
            &PageCursorContext::new(
                "searchWorkspace",
                "query=kernel",
                &generation,
                &vec!["notes/daily.md", "notes/changed.md"],
            )
            .unwrap(),
        )
        .is_err());
    assert!(restarted_key.verify_page_cursor(&cursor, &context).is_err());

    let document_path = WorkspaceRelativePath::parse("notes/daily.md").unwrap();
    let document_id = key
        .issue_document_id(
            WorkspaceId::new(Uuid::new_v4()),
            &generation,
            DocumentKind::File,
            &document_path,
        )
        .unwrap();
    let document_token_as_cursor = PageCursor::parse(document_id.as_str()).unwrap();
    assert!(key
        .verify_page_cursor(&document_token_as_cursor, &context)
        .is_err());

    let cursor_as_document = DocumentId::parse(cursor.as_str()).unwrap();
    assert!(key
        .verify_document_id(
            &cursor_as_document,
            WorkspaceId::new(Uuid::new_v4()),
            &generation,
            DocumentKind::File,
        )
        .is_err());
}

#[test]
fn page_cursor_snapshot_binding_does_not_reduce_the_existing_identity_budget() {
    let key = WireIdentityKey::generate().unwrap();
    let generation = WorkspaceGeneration::parse("generation-1").unwrap();
    let context =
        PageCursorContext::new("documents-list", "", &generation, &Vec::<String>::new()).unwrap();
    let identity = "a".repeat(1_200);

    let cursor = key.issue_page_cursor(&context, identity.clone()).unwrap();

    assert_eq!(key.verify_page_cursor(&cursor, &context).unwrap(), identity);
}

fn tampered_document_id(document_id: &DocumentId) -> DocumentId {
    let mut encoded = document_id.as_str().as_bytes().to_vec();
    let signature_start = encoded.iter().position(|byte| *byte == b'.').unwrap() + 1;
    encoded[signature_start] = if encoded[signature_start] == b'A' {
        b'B'
    } else {
        b'A'
    };
    DocumentId::parse(String::from_utf8(encoded).unwrap()).unwrap()
}

fn tampered_resource_id(resource_id: &ResourceId) -> ResourceId {
    let mut encoded = resource_id.as_str().as_bytes().to_vec();
    let signature_start = encoded.iter().position(|byte| *byte == b'.').unwrap() + 1;
    encoded[signature_start] = if encoded[signature_start] == b'A' {
        b'B'
    } else {
        b'A'
    };
    ResourceId::parse(String::from_utf8(encoded).unwrap()).unwrap()
}

#[test]
fn signed_token_wire_shapes_reject_invalid_syntax_and_oversized_cursors() {
    assert!(serde_json::from_str::<DocumentId>(r#""not-a-token""#).is_err());
    assert!(serde_json::from_str::<PageCursor>(r#""bad.padding==.signature""#).is_err());
    assert!(PageCursor::parse(format!("{}.x", "a".repeat(2_048))).is_err());
    assert!(PageCursor::parse(format!("{}.a", "a".repeat(2_046))).is_ok());
}

#[test]
fn frozen_enums_use_exact_kebab_case_wire_values() {
    assert_eq!(serde_json::to_value(ApiVersion::V1).unwrap(), json!("v1"));
    assert_eq!(
        serde_json::to_value(HostProfile::Desktop).unwrap(),
        json!("desktop")
    );
    assert_eq!(
        serde_json::to_value(StartupState::NeedsWorkspaceInitialization).unwrap(),
        json!("needs-workspace-initialization")
    );
    assert_eq!(
        serde_json::to_value(WorkspaceReadiness::Locked).unwrap(),
        json!("locked")
    );
    assert_eq!(
        serde_json::to_value(DeletionPolicy::Permanent).unwrap(),
        json!("permanent")
    );
    assert_eq!(
        serde_json::to_value(SyncProvider::Webdav).unwrap(),
        json!("webdav")
    );
    assert_eq!(
        serde_json::to_value(SyncMode::FullyManual).unwrap(),
        json!("fully-manual")
    );
}

#[test]
fn safe_dtos_omit_optional_inputs_but_emit_explicit_nullable_outputs() {
    assert_eq!(
        serde_json::to_value(PageQuery::default()).unwrap(),
        json!({})
    );
    assert_eq!(
        serde_json::to_value(DocumentPageDto {
            items: Vec::new(),
            next_cursor: Nullable::null(),
        })
        .unwrap(),
        json!({ "items": [], "nextCursor": null })
    );

    let workspace = WorkspaceDto {
        id: WorkspaceId::new(Uuid::new_v4()),
        generation: WorkspaceGeneration::parse("generation-1").unwrap(),
        display_name: "Notes".to_string(),
        readiness: WorkspaceReadiness::Ready,
        revision: Revision::parse("revision-1").unwrap(),
    };
    let mut unsafe_value = serde_json::to_value(workspace).unwrap();
    unsafe_value
        .as_object_mut()
        .unwrap()
        .insert("absoluteRoot".to_string(), json!("/private/notes"));
    assert!(serde_json::from_value::<WorkspaceDto>(unsafe_value).is_err());
}

#[test]
fn omitted_inputs_and_explicit_nullable_outputs_are_not_interchangeable() {
    assert!(serde_json::from_value::<PageQuery>(json!({})).is_ok());
    assert!(serde_json::from_value::<PageQuery>(json!({ "cursor": null })).is_err());
    assert!(serde_json::from_value::<PageQuery>(json!({ "limit": null })).is_err());
    assert!(serde_json::from_value::<ListWorkspaceInventoryQuery>(json!({})).is_ok());
    assert!(
        serde_json::from_value::<ListWorkspaceInventoryQuery>(json!({
            "cursor": null
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<ListWorkspaceInventoryQuery>(json!({
            "limit": null
        }))
        .is_err()
    );

    assert!(serde_json::from_value::<DocumentPageDto>(json!({
        "items": [],
        "nextCursor": null
    }))
    .is_ok());
    assert!(serde_json::from_value::<DocumentPageDto>(json!({ "items": [] })).is_err());
}

#[test]
fn create_document_request_is_discriminated_by_kind() {
    let file: CreateDocumentRequest = serde_json::from_value(json!({
        "workspaceGeneration": "generation-1",
        "parent": "notes",
        "name": "daily.md",
        "kind": "file",
        "contents": "hello"
    }))
    .unwrap();
    assert!(matches!(file, CreateDocumentRequest::File { .. }));

    assert!(serde_json::from_value::<CreateDocumentRequest>(json!({
        "workspaceGeneration": "generation-1",
        "parent": "notes",
        "name": "folder",
        "kind": "directory",
        "contents": "forbidden"
    }))
    .is_err());
    assert!(serde_json::from_value::<CreateDocumentRequest>(json!({
        "workspaceGeneration": "generation-1",
        "parent": "notes",
        "name": "daily.md",
        "kind": "file"
    }))
    .is_err());
}

#[test]
fn document_content_is_file_only_and_document_debug_redacts_contents() {
    let key = WireIdentityKey::generate().unwrap();
    let workspace = WorkspaceId::new(Uuid::new_v4());
    let generation = WorkspaceGeneration::parse("generation-1").unwrap();
    let path = WorkspaceRelativePath::parse("notes/private.md").unwrap();
    let document_id = key
        .issue_document_id(workspace, &generation, DocumentKind::File, &path)
        .unwrap();
    let base = json!({
        "id": document_id,
        "path": "notes/private.md",
        "parent": "notes",
        "name": "private.md",
        "kind": "file",
        "sizeBytes": 22,
        "modifiedAt": "2026-07-29T12:30:45Z",
        "revision": "revision-1",
        "contents": "private-document-body"
    });

    let content: DocumentContentDto = serde_json::from_value(base.clone()).unwrap();
    assert!(!format!("{content:?}").contains("private-document-body"));

    let mut directory = base;
    directory["kind"] = json!("directory");
    assert!(serde_json::from_value::<DocumentContentDto>(directory).is_err());

    let create: CreateDocumentRequest = serde_json::from_value(json!({
        "workspaceGeneration": "generation-1",
        "parent": "notes",
        "name": "private.md",
        "kind": "file",
        "contents": "private-document-body"
    }))
    .unwrap();
    assert!(!format!("{create:?}").contains("private-document-body"));

    let update: qingyu_kernel::contract::UpdateDocumentRequest = serde_json::from_value(json!({
        "workspaceGeneration": "generation-1",
        "expectedRevision": "revision-1",
        "contents": "private-document-body"
    }))
    .unwrap();
    assert!(!format!("{update:?}").contains("private-document-body"));
}

#[test]
fn document_contents_enforce_the_decoded_v1_size_limit() {
    assert!(DocumentContents::parse("x".repeat(16 * 1024 * 1024)).is_ok());
    assert!(DocumentContents::parse("x".repeat(16 * 1024 * 1024 + 1)).is_err());
}

#[test]
fn document_names_enforce_portable_segment_and_kind_rules_at_deserialization() {
    for invalid in [
        "",
        ".",
        "..",
        "CON.md",
        "PRN.md",
        "AUX.md",
        "NUL.md",
        "COM1.md",
        "COM2.md",
        "COM3.md",
        "COM4.md",
        "COM5.md",
        "COM6.md",
        "COM7.md",
        "COM8.md",
        "COM9.md",
        "LPT1.md",
        "LPT2.md",
        "LPT3.md",
        "LPT4.md",
        "LPT5.md",
        "LPT6.md",
        "LPT7.md",
        "LPT8.md",
        "lpt9.markdown",
        ".QINGYU",
        ".qingyu-ui-update-secret.md",
        ".QINGYU-MCP-UPDATE-secret.md",
        ".markra-sync-stage-secret.md",
        "bad/name.md",
        r"bad\name.md",
        "bad<name.md",
        "bad>name.md",
        "bad:name.md",
        "bad\"name.md",
        "bad|name.md",
        "bad?name.md",
        "bad*name.md",
        "bad\0name.md",
        "bad\u{0001}name.md",
        "trailing.md.",
        "trailing.md ",
        "not-markdown.txt",
    ] {
        assert!(
            serde_json::from_value::<CreateDocumentRequest>(json!({
                "workspaceGeneration": "generation-1",
                "parent": "",
                "name": invalid,
                "kind": "file",
                "contents": ""
            }))
            .is_err(),
            "accepted invalid file name {invalid:?}"
        );
    }

    let at_255_bytes = "x".repeat(255);
    assert_eq!(at_255_bytes.len(), 255);
    assert!(serde_json::from_value::<CreateDocumentRequest>(json!({
        "workspaceGeneration": "generation-1",
        "parent": "",
        "name": at_255_bytes,
        "kind": "directory"
    }))
    .is_ok());

    let over_255_bytes = "x".repeat(256);
    assert_eq!(over_255_bytes.len(), 256);
    assert!(serde_json::from_value::<CreateDocumentRequest>(json!({
        "workspaceGeneration": "generation-1",
        "parent": "",
        "name": over_255_bytes,
        "kind": "directory"
    }))
    .is_err());

    assert!(serde_json::from_value::<CreateDocumentRequest>(json!({
        "workspaceGeneration": "generation-1",
        "parent": "",
        "name": "daily.MARKDOWN",
        "kind": "file",
        "contents": ""
    }))
    .is_ok());
    assert!(serde_json::from_value::<CreateDocumentRequest>(json!({
        "workspaceGeneration": "generation-1",
        "parent": "",
        "name": "folder.with.extension",
        "kind": "directory"
    }))
    .is_ok());

    assert!(serde_json::from_value::<MoveDocumentRequest>(json!({
        "workspaceGeneration": "generation-1",
        "expectedRevision": "revision-1",
        "targetParent": "",
        "name": "../escape.md"
    }))
    .is_err());

    let entry = json!({
        "id": "eyJ2ZXJzaW9uIjoxfQ.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "path": "notes/no-extension",
        "parent": "notes",
        "name": "no-extension",
        "kind": "file",
        "sizeBytes": 0,
        "modifiedAt": "2026-07-29T12:30:45Z",
        "revision": "revision-1"
    });
    assert!(serde_json::from_value::<qingyu_kernel::contract::DocumentEntryDto>(entry).is_err());
}

#[test]
fn document_names_use_the_dejavu_portability_contract_before_product_rules() {
    for candidate in [
        "note.md",
        "个人.md",
        "CON.md",
        "bad:name.md",
        "trailing.md ",
    ] {
        if !qingyu_dejavu::portable_path_component_is_valid(candidate) {
            assert!(DocumentName::parse(candidate).is_err());
            assert!(ResourceName::parse(candidate).is_err());
        }
    }
    assert!(qingyu_dejavu::portable_path_component_is_valid(".qingyu"));
    assert!(DocumentName::parse(".qingyu").is_err());
}

#[test]
fn resource_names_use_portable_segments_without_markdown_extension_rules() {
    for valid in ["photo.png", "archive", "资料.pdf", "cover.MARKDOWN"] {
        assert_eq!(ResourceName::parse(valid).unwrap().as_str(), valid);
        assert!(serde_json::from_value::<ResourceName>(json!(valid)).is_ok());
    }

    for invalid in [
        "",
        ".",
        "..",
        "CON.png",
        "lpt9.bin",
        ".QINGYU",
        ".qingyu-ui-update-secret.png",
        ".QINGYU-MCP-UPDATE-secret.png",
        ".markra-sync-stage-secret.png",
        "bad/name.png",
        r"bad\name.png",
        "bad:name.png",
        "trailing.png.",
        "trailing.png ",
    ] {
        assert!(
            ResourceName::parse(invalid).is_err(),
            "accepted {invalid:?}"
        );
        assert!(serde_json::from_value::<ResourceName>(json!(invalid)).is_err());
    }

    assert!(ResourceName::parse("界".repeat(86)).is_err());
}

#[test]
fn resource_entries_have_stable_wire_shape_and_redacted_signed_identity() {
    let key = WireIdentityKey::generate().unwrap();
    let workspace = WorkspaceId::new(Uuid::new_v4());
    let generation = WorkspaceGeneration::parse("generation-1").unwrap();
    let path = WorkspaceRelativePath::parse("assets/photo.png").unwrap();
    let resource_id = key
        .issue_resource_id(workspace, &generation, ResourceKind::Image, &path)
        .unwrap();
    let value = json!({
        "id": resource_id,
        "path": "assets/photo.png",
        "parent": "assets",
        "name": "photo.png",
        "kind": "image",
        "sizeBytes": 1024,
        "modifiedAt": "2026-07-29T12:30:45Z",
        "revision": "revision-1",
        "mediaType": "image/png",
        "previewable": true
    });

    let entry: ResourceEntryDto = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(entry.kind, ResourceKind::Image);
    assert_eq!(entry.name.as_str(), "photo.png");
    assert_eq!(entry.media_type, "image/png");
    assert_eq!(serde_json::to_value(&entry).unwrap(), value);
    assert!(!format!("{entry:?}").contains(entry.id.as_str()));

    let inventory_value = json!({ "entryType": "resource", "resource": value });
    let inventory: WorkspaceInventoryEntryDto =
        serde_json::from_value(inventory_value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&inventory).unwrap(), inventory_value);
    assert!(serde_json::from_value::<WorkspaceInventoryEntryDto>(json!({
        "entryType": "resource",
        "resource": inventory_value["resource"],
        "absolutePath": "/private/assets/photo.png"
    }))
    .is_err());
    assert!(serde_json::from_value::<WorkspaceInventoryPageDto>(json!({
        "items": [inventory_value],
        "nextCursor": null
    }))
    .is_ok());
}

#[test]
fn search_queries_are_normalized_and_bounded_at_deserialization() {
    let query: SearchWorkspaceQuery = serde_json::from_value(json!({
        "query": "  kernel architecture  "
    }))
    .unwrap();
    assert_eq!(query.query.as_str(), "kernel architecture");

    for invalid in ["".to_string(), " \n\t ".to_string(), "x".repeat(513)] {
        assert!(serde_json::from_value::<SearchWorkspaceQuery>(json!({
            "query": invalid
        }))
        .is_err());
    }
    assert!(serde_json::from_value::<SearchWorkspaceQuery>(json!({
        "query": "界".repeat(512)
    }))
    .is_ok());
}

#[test]
fn search_positions_are_positive_javascript_safe_integers() {
    assert!(PositiveSafeInteger::new(1).is_ok());
    assert!(PositiveSafeInteger::new(0).is_err());
    assert!(PositiveSafeInteger::new(qingyu_kernel::contract::MAX_SAFE_INTEGER + 1).is_err());

    let document = json!({
        "id": "eyJ2ZXJzaW9uIjoxfQ.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "path": "daily.md",
        "parent": "",
        "name": "daily.md",
        "kind": "file",
        "sizeBytes": 0,
        "modifiedAt": "2026-07-29T12:30:45Z",
        "revision": "revision-1"
    });
    for field in ["line", "column"] {
        let mut value = json!({
            "document": document.clone(),
            "line": 1,
            "column": 1,
            "preview": "safe preview"
        });
        value[field] = json!(0);
        assert!(serde_json::from_value::<SearchMatchDto>(value).is_err());
    }

    let match_dto: SearchMatchDto = serde_json::from_value(json!({
        "document": document,
        "line": 1,
        "column": 1,
        "preview": "private-document-preview"
    }))
    .unwrap();
    assert!(!format!("{match_dto:?}").contains("private-document-preview"));
}

#[test]
fn timestamps_are_canonical_rfc3339_utc_values() {
    let timestamp = Rfc3339Utc::parse("2026-07-29T12:30:45Z").unwrap();

    assert_eq!(timestamp.as_str(), "2026-07-29T12:30:45Z");
    assert!(Rfc3339Utc::parse("2026-07-29T20:30:45+08:00").is_err());
    assert!(Rfc3339Utc::parse("2026-02-30T12:30:45Z").is_err());
    assert!(Rfc3339Utc::parse("2026-01-01T24:00:00Z").is_err());
    assert!(serde_json::from_str::<Rfc3339Utc>(r#""2026-07-29 12:30:45""#).is_err());
    assert_eq!(
        Rfc3339Utc::parse("2026-07-29T12:30:45+00:00")
            .unwrap()
            .as_str(),
        "2026-07-29T12:30:45Z"
    );
}

#[test]
fn rfc3339_utc_accepts_negative_zero_offset_and_long_fractional_seconds() {
    for value in [
        "2024-02-29T23:59:59.123456789-00:00",
        "2024-02-29T23:59:59.1234567890Z",
        "2024-02-29T23:59:59.1234567890+00:00",
    ] {
        assert_eq!(
            Rfc3339Utc::parse(value).unwrap().as_str(),
            "2024-02-29T23:59:59.123456789Z"
        );
    }
}

#[test]
fn setting_values_use_strict_nested_discriminators() {
    let entry = SettingEntryDto {
        key: SettingKey::EditorFontFamily,
        value: SettingValueDto::FontFamily {
            value: FontFamilyValueDto::Theme {
                family: Nullable::null(),
            },
        },
    };
    assert_eq!(
        serde_json::to_value(entry).unwrap(),
        json!({
            "key": "editor.fontFamily",
            "value": {
                "type": "font-family",
                "value": { "source": "theme", "family": null }
            }
        })
    );
    assert!(serde_json::from_value::<SettingValueDto>(json!({
        "type": "nullable-string"
    }))
    .is_err());
    assert!(serde_json::from_value::<FontFamilyValueDto>(json!({
        "source": "theme"
    }))
    .is_err());
}

#[test]
fn settings_patch_rejects_empty_and_duplicate_keys() {
    let revision = Revision::parse("revision-1").unwrap();
    let empty = PatchSettingsRequest {
        expected_revision: revision.clone(),
        values: Vec::new(),
    };
    assert!(empty.validate().is_err());

    let duplicate = SettingEntryDto {
        key: SettingKey::EditorShowWordCount,
        value: SettingValueDto::Boolean { value: true },
    };
    let patch = PatchSettingsRequest {
        expected_revision: revision,
        values: vec![duplicate.clone(), duplicate],
    };
    assert!(patch.validate().is_err());
}

#[test]
fn credential_changes_are_exact_and_secret_debug_is_redacted() {
    let change: CredentialChange = serde_json::from_value(json!({
        "operation": "replace",
        "value": "not-for-logs"
    }))
    .unwrap();

    assert!(!format!("{change:?}").contains("not-for-logs"));
    assert!(std::mem::needs_drop::<CredentialChange>());
    assert!(serde_json::from_value::<CredentialChange>(json!({
        "operation": "keep",
        "value": "unexpected"
    }))
    .is_err());
}

#[test]
fn sync_changes_distinguish_omitted_fields_from_null_and_reject_empty_changes() {
    let empty: SyncConfigChangesDto = serde_json::from_value(json!({})).unwrap();
    assert!(empty.validate().is_err());
    assert!(serde_json::from_value::<SyncConfigChangesDto>(json!({
        "provider": null
    }))
    .is_err());

    let provider: SyncConfigChangesDto = serde_json::from_value(json!({
        "provider": "s3"
    }))
    .unwrap();
    assert!(provider.validate().is_ok());
}

#[test]
fn error_codes_have_a_complete_stable_http_mapping() {
    let cases = [
        (ErrorCode::InvalidRequest, 400),
        (ErrorCode::InvalidWorkspacePath, 400),
        (ErrorCode::InvalidDocumentName, 400),
        (ErrorCode::Unauthorized, 401),
        (ErrorCode::InitializationRequired, 409),
        (ErrorCode::AlreadyInitialized, 409),
        (ErrorCode::InvalidCredentials, 401),
        (ErrorCode::CsrfRejected, 403),
        (ErrorCode::AuthenticationRateLimited, 429),
        (ErrorCode::AuthenticationUnavailable, 503),
        (ErrorCode::HostNotAllowed, 403),
        (ErrorCode::OriginNotAllowed, 403),
        (ErrorCode::DocumentNotFound, 404),
        (ErrorCode::ResourceNotFound, 404),
        (ErrorCode::SyncConfigAbsent, 404),
        (ErrorCode::DocumentAlreadyExists, 409),
        (ErrorCode::RevisionConflict, 409),
        (ErrorCode::SettingsRevisionConflict, 409),
        (ErrorCode::WorkspaceGenerationStale, 409),
        (ErrorCode::SyncConfigRevisionConflict, 409),
        (ErrorCode::DocumentTooLarge, 413),
        (ErrorCode::DocumentInvalidEncoding, 422),
        (ErrorCode::InvalidSettingsField, 422),
        (ErrorCode::InvalidAppConfigState, 422),
        (ErrorCode::SyncConfigInvalid, 422),
        (ErrorCode::WorkspaceLocked, 423),
        (ErrorCode::KernelNotReady, 503),
        (ErrorCode::WorkspaceUnavailable, 503),
        (ErrorCode::SettingsUnavailable, 503),
        (ErrorCode::AppConfigUnavailable, 503),
        (ErrorCode::SyncNotReady, 503),
        (ErrorCode::SyncRunUnavailable, 503),
        (ErrorCode::InternalError, 500),
    ];

    for (code, expected) in cases {
        assert_eq!(
            http_status_for_error_code(code),
            expected,
            "wrong status for {code:?}"
        );
    }
}

#[test]
fn error_envelopes_keep_optional_details_omitted_and_non_null() {
    let envelope = safe_error_envelope(
        ErrorCode::InvalidRequest,
        qingyu_kernel::contract::RequestId::new(Uuid::new_v4()),
        None,
    )
    .unwrap();
    assert!(!serde_json::to_value(envelope)
        .unwrap()
        .as_object()
        .unwrap()
        .contains_key("details"));
    assert!(serde_json::from_value::<ApiErrorEnvelope>(json!({
        "code": "revision_conflict",
        "message": "Conflict.",
        "requestId": Uuid::new_v4(),
        "details": {
            "type": "revision-conflict",
            "currentRevision": null
        }
    }))
    .is_err());
    assert_eq!(
        serde_json::to_value(ErrorDetails::Startup {
            state: StartupState::Starting,
        })
        .unwrap(),
        json!({ "type": "startup", "state": "starting" })
    );
    assert_eq!(
        serde_json::to_value(ValidationIssueCode::InvalidFormat).unwrap(),
        json!("invalid-format")
    );
}

#[test]
fn validation_details_are_non_empty_allowlisted_and_use_safe_messages() {
    assert!(serde_json::from_value::<ErrorDetails>(json!({
        "type": "validation",
        "issues": []
    }))
    .is_err());
    assert!(serde_json::from_value::<ValidationIssueDto>(json!({
        "field": "absoluteRoot",
        "code": "unsafe-value",
        "message": "leaked /private/root"
    }))
    .is_err());

    let issue = ValidationIssueDto::new(ValidationField::Name, ValidationIssueCode::InvalidFormat);
    let details = ErrorDetails::Validation {
        issues: ValidationIssues::new(issue, []),
    };
    let envelope = safe_error_envelope(
        ErrorCode::InvalidDocumentName,
        qingyu_kernel::contract::RequestId::new(Uuid::new_v4()),
        Some(details),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(envelope).unwrap()["details"],
        json!({
            "type": "validation",
            "issues": [{
                "field": "name",
                "code": "invalid-format",
                "message": "This field has an invalid format."
            }]
        })
    );

    assert!(safe_error_envelope(
        ErrorCode::InternalError,
        qingyu_kernel::contract::RequestId::new(Uuid::new_v4()),
        Some(ErrorDetails::Startup {
            state: StartupState::Starting,
        }),
    )
    .is_err());
}

#[test]
fn public_error_debug_never_echoes_untrusted_strings() {
    let envelope: ApiErrorEnvelope = serde_json::from_value(json!({
        "code": "internal_error",
        "message": "secret=https://user:pass@example.test/private/root",
        "requestId": Uuid::new_v4()
    }))
    .unwrap();
    assert!(!format!("{envelope:?}").contains("user:pass"));
    assert!(!format!("{envelope:?}").contains("/private/root"));

    assert!(serde_json::from_value::<SyncSafeErrorDto>(json!({
        "category": "secret-category",
        "code": "secret-code",
        "method": "secret-method",
        "objectId": "https://user:pass@example.test/signed?secret=yes",
        "operation": "/private/root",
        "provider": "s3",
        "providerErrorCode": "secret-provider-code"
    }))
    .is_err());
    assert!(serde_json::from_value::<SyncSafeErrorDto>(json!({
        "category": "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcY",
        "code": "AKIAIOSFODNN7EXAMPLE",
        "operation": "0123456789abcdef0123456789abcdef",
        "provider": "s3"
    }))
    .is_err());

    assert!(serde_json::from_value::<SyncSafeErrorDto>(json!({
        "category": "transport",
        "code": "request_failed",
        "method": "PUT",
        "objectId": "documents/note.md",
        "operation": "upload_object",
        "provider": "s3",
        "providerErrorCode": "SlowDown"
    }))
    .is_err());

    let sync_error = SyncSafeErrorDto::new(
        SyncProvider::S3,
        SyncSafeErrorOperation::UploadObject,
        SyncSafeErrorCode::RequestFailed,
    )
    .with_category(SyncSafeErrorCategory::Transport)
    .with_method(SyncSafeHttpMethod::Put)
    .with_provider_error_code(SyncSafeProviderErrorCode::SlowDown);
    assert_eq!(
        serde_json::to_value(&sync_error).unwrap(),
        json!({
            "category": "transport",
            "code": "request_failed",
            "method": "PUT",
            "operation": "upload_object",
            "provider": "s3",
            "providerErrorCode": "SlowDown"
        })
    );
    let debug = format!("{sync_error:?}");
    for secret in ["transport", "request_failed", "SlowDown"] {
        assert!(!debug.contains(secret));
    }
}

#[test]
fn portable_name_required_is_a_stable_safe_sync_error_code() {
    let safe: SyncSafeErrorDto = serde_json::from_value(json!({
        "category": "storage",
        "code": "portable-name-required",
        "operation": "sync_run",
        "provider": "s3",
        "relativePath": "CON.md",
        "runId": "00000000-0000-4000-8000-000000000012"
    }))
    .expect("portable-name-required safe error");

    assert_eq!(safe.code(), "portable-name-required");
    assert_eq!(safe.category(), Some("storage"));
    assert_eq!(safe.operation(), "sync_run");
    assert_eq!(
        safe.relative_path().map(WorkspaceRelativePath::as_str),
        Some("CON.md")
    );
    let serialized = serde_json::to_string(&safe).expect("serialize safe error");
    assert!(!serialized.contains("/Users/"));
    assert!(!serialized.contains(r"C:\Users\"));
}

#[test]
fn event_frame_scalars_enforce_protocol_and_sequence_ranges() {
    assert!(ProtocolVersion::new(1).is_ok());
    assert!(ProtocolVersion::new(2).is_err());
    assert!(ReadySequence::new(0).is_ok());
    assert!(ReadySequence::new(1).is_err());
    assert!(EventSequence::new(1).is_ok());
    assert!(EventSequence::new(0).is_err());
}

#[test]
fn authenticate_and_server_error_frames_are_strict_and_redacted() {
    let authenticate: AuthenticateFrame = serde_json::from_value(json!({
        "type": "authenticate",
        "protocolVersion": 1,
        "credential": "launch-secret"
    }))
    .unwrap();
    assert!(!format!("{authenticate:?}").contains("launch-secret"));
    assert!(std::mem::needs_drop::<AuthenticateFrame>());
    assert!(std::mem::needs_drop::<WireIdentityKey>());
    assert!(serde_json::from_value::<AuthenticateFrame>(json!({
        "type": "authenticate",
        "protocolVersion": 1,
        "credential": "launch-secret",
        "extra": true
    }))
    .is_err());

    let frame = ServerFrame::Error {
        protocol_version: ProtocolVersion::new(1).unwrap(),
        code: FrameErrorCode::InvalidFrame,
        message: "Invalid event frame.".to_string(),
    };
    assert_eq!(
        serde_json::to_value(frame).unwrap(),
        json!({
            "type": "error",
            "protocolVersion": 1,
            "code": "invalid-frame",
            "message": "Invalid event frame."
        })
    );
}

#[test]
fn app_config_event_and_reload_scope_are_exact_and_redacted() {
    let workspace_id = WorkspaceId::new(Uuid::from_u128(1));
    let workspace_generation = WorkspaceGeneration::parse("generation-1").unwrap();
    let revision = Revision::parse("app-config-1").unwrap();
    let frame = ServerFrame::Event {
        protocol_version: ProtocolVersion::new(1).unwrap(),
        connection_id: qingyu_kernel::contract::ConnectionId::new(Uuid::from_u128(2)),
        sequence: EventSequence::new(1).unwrap(),
        resource: ResourceRefDto::AppConfig {
            workspace_id,
            workspace_generation: workspace_generation.clone(),
        },
        revision: revision.clone(),
        event: Box::new(DomainEvent::AppConfigStateChanged {
            workspace_id,
            workspace_generation,
            revision,
        }),
    };
    let value = serde_json::to_value(frame).unwrap();
    assert_eq!(
        value,
        json!({
            "type": "event",
            "protocolVersion": 1,
            "connectionId": Uuid::from_u128(2),
            "sequence": 1,
            "resource": {
                "kind": "app-config",
                "workspaceId": Uuid::from_u128(1),
                "workspaceGeneration": "generation-1"
            },
            "revision": "app-config-1",
            "event": {
                "type": "app-config-state-changed",
                "workspaceId": Uuid::from_u128(1),
                "workspaceGeneration": "generation-1",
                "revision": "app-config-1"
            }
        })
    );
    for forbidden in ["draftTabs", "filePath", "uiLayout", "pandocPath"] {
        assert!(!value.to_string().contains(forbidden));
    }

    let mut extra = value;
    extra["event"]["filePath"] = json!("secret.md");
    assert!(serde_json::from_value::<ServerFrame>(extra).is_err());
    assert_eq!(
        serde_json::to_value(ReloadScope::AppConfig).unwrap(),
        json!("app-config")
    );
    assert!(serde_json::from_value::<ReloadScope>(json!("app-config-extra")).is_err());
}
