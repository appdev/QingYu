export interface paths {
    "/api/v1/auth/initialize": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["initializeServerOwner"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/logout": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["logoutServerSession"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/password": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch: operations["changeServerOwnerPassword"];
        trace?: never;
    };
    "/api/v1/auth/session": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["getServerSession"];
        put?: never;
        post: operations["createServerSession"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/auth/status": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["getAuthenticationStatus"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/documents": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["listDocuments"];
        put?: never;
        post: operations["createDocument"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/documents/{documentId}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["getDocument"];
        put: operations["updateDocument"];
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/documents/{documentId}/delete": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["deleteDocument"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/documents/{documentId}/history": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["listDocumentHistory"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/documents/{documentId}/history/{snapshotId}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["getDocumentHistory"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/documents/{documentId}/history/{snapshotId}/restore": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["restoreDocumentHistory"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/documents/{documentId}/move": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["moveDocument"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/documents/{documentId}/resources": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["createWorkspaceResource"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/health/live": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["healthLive"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/health/ready": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["healthReady"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/inventory": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["listWorkspaceInventory"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/resources/{resourceId}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["openWorkspaceResource"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/runtime": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["getRuntimeState"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/search": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["searchWorkspace"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/settings": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["getSettings"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch: operations["patchSettings"];
        trace?: never;
    };
    "/api/v1/sync/config": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["getSyncConfig"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch: operations["patchSyncConfig"];
        trace?: never;
    };
    "/api/v1/sync/connection-test": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["testSyncConnection"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/sync/runs": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get?: never;
        put?: never;
        post: operations["triggerSyncRun"];
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/sync/runs/{runId}": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["getSyncRun"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/sync/status": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["getSyncStatus"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/system/version": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["getSystemVersion"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
    "/api/v1/workspace": {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        get: operations["getWorkspace"];
        put?: never;
        post?: never;
        delete?: never;
        options?: never;
        head?: never;
        patch?: never;
        trace?: never;
    };
}
export type webhooks = Record<string, never>;
export interface components {
    schemas: {
        ApiErrorEnvelope: {
            code: components["schemas"]["ErrorCode"];
            details?: components["schemas"]["ErrorDetails"];
            message: string;
            requestId: components["schemas"]["RequestId"];
        };
        /** @enum {string} */
        ApiVersion: "v1";
        AuthenticateFrame: {
            credential: string;
            protocolVersion: components["schemas"]["ProtocolVersion"];
            type: components["schemas"]["AuthenticateFrameKind"];
        };
        /** @enum {string} */
        AuthenticateFrameKind: "authenticate";
        ChangeServerOwnerPasswordRequest: {
            currentPassword: string;
            newPassword: string;
        };
        /** Format: uuid */
        ConnectionId: string;
        CreateDocumentRequest: {
            contents: components["schemas"]["DocumentContents"];
            /** @enum {string} */
            kind: "file";
            name: components["schemas"]["FileDocumentName"];
            parent: components["schemas"]["WorkspaceRelativePath"];
            workspaceGeneration: components["schemas"]["WorkspaceGeneration"];
        } | {
            /** @enum {string} */
            kind: "directory";
            name: components["schemas"]["DocumentName"];
            parent: components["schemas"]["WorkspaceRelativePath"];
            workspaceGeneration: components["schemas"]["WorkspaceGeneration"];
        };
        CreateServerSessionRequest: {
            password: string;
        };
        CreateWorkspaceResourceQuery: {
            folder: components["schemas"]["WorkspaceRelativePath"];
            kind: components["schemas"]["ResourceKind"];
            name: components["schemas"]["ResourceName"];
            workspaceGeneration: components["schemas"]["WorkspaceGeneration"];
        };
        CreatedDocumentDto: {
            contents: components["schemas"]["DocumentContents"];
            id: components["schemas"]["DocumentId"];
            /** @enum {string} */
            kind: "file";
            modifiedAt: components["schemas"]["Rfc3339Utc"];
            name: components["schemas"]["FileDocumentName"];
            parent: components["schemas"]["WorkspaceRelativePath"];
            path: components["schemas"]["WorkspaceRelativePath"];
            revision: components["schemas"]["Revision"];
            sizeBytes: components["schemas"]["SafeUnsignedInteger"];
        } | {
            id: components["schemas"]["DocumentId"];
            /** @enum {string} */
            kind: "directory";
            modifiedAt: components["schemas"]["Rfc3339Utc"];
            name: components["schemas"]["DocumentName"];
            parent: components["schemas"]["WorkspaceRelativePath"];
            path: components["schemas"]["WorkspaceRelativePath"];
            revision: components["schemas"]["Revision"];
            sizeBytes: components["schemas"]["SafeUnsignedInteger"];
        };
        CredentialChange: {
            /** @enum {string} */
            operation: "keep";
        } | {
            /** @enum {string} */
            operation: "replace";
            value: string;
        } | {
            /** @enum {string} */
            operation: "clear";
        };
        CredentialState: {
            present: boolean;
        };
        DeleteDocumentRequest: {
            deletionPolicy: components["schemas"]["DeletionPolicy"];
            expectedRevision: components["schemas"]["Revision"];
            workspaceGeneration: components["schemas"]["WorkspaceGeneration"];
        };
        /** @enum {string} */
        DeletionPolicy: "recoverable" | "permanent";
        DocumentChangedEvent: {
            document: components["schemas"]["DocumentEntryDto"];
            type: components["schemas"]["DocumentChangedKind"];
        };
        /** @enum {string} */
        DocumentChangedKind: "document-changed";
        DocumentContentDto: {
            contents: components["schemas"]["DocumentContents"];
            id: components["schemas"]["DocumentId"];
            kind: components["schemas"]["FileDocumentKind"];
            modifiedAt: components["schemas"]["Rfc3339Utc"];
            name: components["schemas"]["FileDocumentName"];
            parent: components["schemas"]["WorkspaceRelativePath"];
            path: components["schemas"]["WorkspaceRelativePath"];
            revision: components["schemas"]["Revision"];
            sizeBytes: components["schemas"]["SafeUnsignedInteger"];
        };
        DocumentContents: string;
        DocumentCreatedEvent: {
            document: components["schemas"]["DocumentEntryDto"];
            type: components["schemas"]["DocumentCreatedKind"];
        };
        /** @enum {string} */
        DocumentCreatedKind: "document-created";
        DocumentDeletedEvent: {
            documentId: components["schemas"]["DocumentId"];
            previousPath: components["schemas"]["WorkspaceRelativePath"];
            revision: components["schemas"]["Revision"];
            type: components["schemas"]["DocumentDeletedKind"];
            workspaceGeneration: components["schemas"]["WorkspaceGeneration"];
        };
        /** @enum {string} */
        DocumentDeletedKind: "document-deleted";
        DocumentEntryDto: {
            id: components["schemas"]["DocumentId"];
            kind: components["schemas"]["DocumentKind"];
            modifiedAt: components["schemas"]["Rfc3339Utc"];
            name: components["schemas"]["DocumentName"];
            parent: components["schemas"]["WorkspaceRelativePath"];
            path: components["schemas"]["WorkspaceRelativePath"];
            revision: components["schemas"]["Revision"];
            sizeBytes: components["schemas"]["SafeUnsignedInteger"];
        };
        DocumentHistoryPageDto: {
            items: components["schemas"]["HistoryEntryDto"][];
            nextCursor: components["schemas"]["Nullable_PageCursor"];
        };
        DocumentHistorySnapshotDto: {
            contents: components["schemas"]["DocumentContents"];
            createdAt: components["schemas"]["Rfc3339Utc"];
            documentId: components["schemas"]["DocumentId"];
            revision: components["schemas"]["Revision"];
            sizeBytes: components["schemas"]["SafeUnsignedInteger"];
            snapshotId: components["schemas"]["SnapshotId"];
        };
        DocumentId: string;
        /** @enum {string} */
        DocumentKind: "file" | "directory";
        DocumentMovedEvent: {
            document: components["schemas"]["DocumentEntryDto"];
            previousPath: components["schemas"]["WorkspaceRelativePath"];
            type: components["schemas"]["DocumentMovedKind"];
        };
        /** @enum {string} */
        DocumentMovedKind: "document-moved";
        DocumentName: string;
        DocumentPageDto: {
            items: components["schemas"]["DocumentEntryDto"][];
            nextCursor: components["schemas"]["Nullable_PageCursor"];
        };
        DomainEvent: {
            /** @enum {string} */
            type: "workspace-changed";
            workspace: components["schemas"]["WorkspaceDto"];
        } | {
            document: components["schemas"]["DocumentEntryDto"];
            /** @enum {string} */
            type: "document-created";
        } | {
            document: components["schemas"]["DocumentEntryDto"];
            /** @enum {string} */
            type: "document-changed";
        } | {
            document: components["schemas"]["DocumentEntryDto"];
            previousPath: components["schemas"]["WorkspaceRelativePath"];
            /** @enum {string} */
            type: "document-moved";
        } | {
            documentId: components["schemas"]["DocumentId"];
            previousPath: components["schemas"]["WorkspaceRelativePath"];
            revision: components["schemas"]["Revision"];
            /** @enum {string} */
            type: "document-deleted";
            workspaceGeneration: components["schemas"]["WorkspaceGeneration"];
        } | {
            settings: components["schemas"]["SettingsSnapshotDto"];
            /** @enum {string} */
            type: "settings-changed";
        } | {
            config: components["schemas"]["SyncConfigViewDto"];
            /** @enum {string} */
            type: "sync-config-changed";
        } | {
            status: components["schemas"]["SyncStatusDto"];
            /** @enum {string} */
            type: "sync-status-changed";
        };
        /** @enum {string} */
        ErrorCode: "invalid_request" | "invalid_workspace_path" | "invalid_document_name" | "unauthorized" | "initialization_required" | "already_initialized" | "invalid_credentials" | "csrf_rejected" | "authentication_rate_limited" | "authentication_unavailable" | "host_not_allowed" | "origin_not_allowed" | "kernel_not_ready" | "workspace_unavailable" | "workspace_locked" | "document_not_found" | "resource_not_found" | "document_already_exists" | "document_too_large" | "resource_too_large" | "document_invalid_encoding" | "revision_conflict" | "settings_revision_conflict" | "sync_config_revision_conflict" | "invalid_settings_field" | "settings_unavailable" | "sync_config_absent" | "sync_config_invalid" | "sync_not_ready" | "sync_run_unavailable" | "internal_error";
        ErrorDetails: {
            currentRevision?: components["schemas"]["Revision"];
            /** @enum {string} */
            type: "revision-conflict";
        } | {
            issues: components["schemas"]["ValidationIssues"];
            /** @enum {string} */
            type: "validation";
        } | {
            state: components["schemas"]["StartupState"];
            /** @enum {string} */
            type: "startup";
        } | {
            retryAfterSeconds: components["schemas"]["PositiveSafeInteger"];
            /** @enum {string} */
            type: "rate-limit";
        };
        ErrorFrame: {
            code: components["schemas"]["FrameErrorCode"];
            message: string;
            protocolVersion: components["schemas"]["ProtocolVersion"];
            type: components["schemas"]["ErrorFrameKind"];
        };
        /** @enum {string} */
        ErrorFrameKind: "error";
        EventFrame: {
            connectionId: components["schemas"]["ConnectionId"];
            event: components["schemas"]["DomainEvent"];
            protocolVersion: components["schemas"]["ProtocolVersion"];
            resource: components["schemas"]["ResourceRefDto"];
            revision: components["schemas"]["Revision"];
            sequence: components["schemas"]["EventSequence"];
            type: components["schemas"]["EventFrameKind"];
        };
        /** @enum {string} */
        EventFrameKind: "event";
        /** Format: int64 */
        EventSequence: number;
        /** @enum {string} */
        FileDocumentKind: "file";
        FileDocumentName: components["schemas"]["DocumentName"];
        /** Format: double */
        FiniteNumber: number;
        FontFamilyValueDto: {
            family: components["schemas"]["Nullable_String"];
            /** @enum {string} */
            source: "theme";
        } | {
            family: string;
            /** @enum {string} */
            source: "system";
        };
        /** @enum {string} */
        FrameErrorCode: "unauthorized" | "invalid-frame" | "unsupported-version";
        GapFrame: {
            connectionId: components["schemas"]["ConnectionId"];
            protocolVersion: components["schemas"]["ProtocolVersion"];
            reason: components["schemas"]["GapReason"];
            reloadScopes: components["schemas"]["ReloadScope"][];
            sequence: components["schemas"]["EventSequence"];
            type: components["schemas"]["GapFrameKind"];
        };
        /** @enum {string} */
        GapFrameKind: "gap";
        /** @enum {string} */
        GapReason: "buffer-overflow" | "sequence-exhausted";
        HistoryEntryDto: {
            createdAt: components["schemas"]["Rfc3339Utc"];
            documentId: components["schemas"]["DocumentId"];
            revision: components["schemas"]["Revision"];
            sizeBytes: components["schemas"]["SafeUnsignedInteger"];
            snapshotId: components["schemas"]["SnapshotId"];
        };
        /** @enum {string} */
        HostProfile: "desktop" | "server" | "mobile";
        /** Format: int32 */
        HttpStatus: number;
        InitializeServerOwnerRequest: {
            initializationToken: string;
            password: string;
        };
        /** Format: uuid */
        InstanceId: string;
        ListDocumentsQuery: {
            cursor?: components["schemas"]["PageCursor"];
            limit?: components["schemas"]["PageLimit"];
            parent?: components["schemas"]["WorkspaceRelativePath"];
        };
        ListWorkspaceInventoryQuery: {
            cursor?: components["schemas"]["PageCursor"];
            limit?: components["schemas"]["PageLimit"];
            parent?: components["schemas"]["WorkspaceRelativePath"];
        };
        LiveHealthResponse: {
            apiVersion: components["schemas"]["ApiVersion"];
            status: components["schemas"]["LiveStatus"];
        };
        /** @enum {string} */
        LiveStatus: "live";
        MoveDocumentRequest: {
            expectedRevision: components["schemas"]["Revision"];
            name: components["schemas"]["DocumentName"];
            targetParent: components["schemas"]["WorkspaceRelativePath"];
            workspaceGeneration: components["schemas"]["WorkspaceGeneration"];
        };
        Nullable_PageCursor: null | string;
        Nullable_Revision: null | string;
        Nullable_Rfc3339Utc: null | string;
        Nullable_RunId: null | string;
        Nullable_SafeInteger: null | number;
        Nullable_String: null | string;
        Nullable_SyncSafeErrorDto: null | {
            category?: string;
            code: string;
            httpStatus?: components["schemas"]["HttpStatus"];
            method?: string;
            objectId?: string;
            operation: string;
            provider: components["schemas"]["SyncProvider"];
            providerErrorCode?: string;
            relativePath?: components["schemas"]["WorkspaceRelativePath"];
            requestId?: components["schemas"]["RequestId"];
            runId?: components["schemas"]["RunId"];
        };
        Nullable_SyncSummaryDto: null | {
            bytesDownloaded: components["schemas"]["SafeUnsignedInteger"];
            bytesUploaded: components["schemas"]["SafeUnsignedInteger"];
            conflictFiles: components["schemas"]["SafeUnsignedInteger"];
            downloadedFiles: components["schemas"]["SafeUnsignedInteger"];
            scannedFiles: components["schemas"]["SafeUnsignedInteger"];
            skippedFiles: components["schemas"]["SafeUnsignedInteger"];
            uploadedFiles: components["schemas"]["SafeUnsignedInteger"];
        };
        Nullable_SyncTrigger: null | ("app-launch" | "interval" | "manual" | "save" | "settings-exit");
        PageCursor: string;
        /** Format: int32 */
        PageLimit: number;
        PageQuery: {
            cursor?: components["schemas"]["PageCursor"];
            limit?: components["schemas"]["PageLimit"];
        };
        PatchSettingsRequest: {
            expectedRevision: components["schemas"]["Revision"];
            values: components["schemas"]["SettingEntryDto"][];
        };
        PatchSyncConfigRequest: {
            changes: components["schemas"]["SyncConfigChangesDto"];
            expectedRevision: components["schemas"]["Revision"];
        };
        /** Format: int64 */
        PositiveSafeInteger: number;
        /** Format: int32 */
        ProtocolVersion: number;
        ReadyFrame: {
            connectionId: components["schemas"]["ConnectionId"];
            instanceId: components["schemas"]["InstanceId"];
            protocolVersion: components["schemas"]["ProtocolVersion"];
            sequence: components["schemas"]["ReadySequence"];
            snapshotRequired: components["schemas"]["SnapshotRequired"];
            type: components["schemas"]["ReadyFrameKind"];
        };
        /** @enum {string} */
        ReadyFrameKind: "ready";
        ReadyHealthResponse: {
            apiVersion: components["schemas"]["ApiVersion"];
            instanceId: components["schemas"]["InstanceId"];
            status: components["schemas"]["ReadyStatus"];
        };
        /** Format: int32 */
        ReadySequence: number;
        /** @enum {string} */
        ReadyStatus: "ready";
        /** @enum {string} */
        ReloadScope: "workspace" | "documents" | "settings" | "sync-config" | "sync-status";
        /** Format: uuid */
        RequestId: string;
        /** Format: int32 */
        RequestTimeoutSeconds: number;
        ResourceEntryDto: {
            id: components["schemas"]["ResourceId"];
            kind: components["schemas"]["ResourceKind"];
            mediaType: string;
            modifiedAt: components["schemas"]["Rfc3339Utc"];
            name: components["schemas"]["ResourceName"];
            parent: components["schemas"]["WorkspaceRelativePath"];
            path: components["schemas"]["WorkspaceRelativePath"];
            previewable: boolean;
            revision: components["schemas"]["Revision"];
            sizeBytes: components["schemas"]["SafeUnsignedInteger"];
        };
        ResourceId: string;
        /** @enum {string} */
        ResourceKind: "image" | "attachment";
        ResourceName: string;
        ResourceRefDto: {
            id: components["schemas"]["WorkspaceId"];
            /** @enum {string} */
            kind: "workspace";
        } | {
            id: components["schemas"]["DocumentId"];
            /** @enum {string} */
            kind: "document";
        } | {
            /** @enum {string} */
            kind: "settings";
        } | {
            /** @enum {string} */
            kind: "sync-config";
        } | {
            /** @enum {string} */
            kind: "sync-status";
            runId: components["schemas"]["Nullable_RunId"];
        };
        RestoreDocumentHistoryRequest: {
            expectedRevision: components["schemas"]["Revision"];
            workspaceGeneration: components["schemas"]["WorkspaceGeneration"];
        };
        Revision: string;
        Rfc3339Utc: string;
        /** Format: uuid */
        RunId: string;
        RuntimeCapabilitiesDto: {
            documents: boolean;
            history: boolean;
            portableSettings: boolean;
            resources: boolean;
            s3: boolean;
            search: boolean;
            settings: boolean;
            sync: boolean;
            webdav: boolean;
        };
        RuntimeStateDto: {
            capabilities: components["schemas"]["RuntimeCapabilitiesDto"];
            instanceId: components["schemas"]["InstanceId"];
            profile: components["schemas"]["HostProfile"];
            startupState: components["schemas"]["StartupState"];
        };
        /** @enum {string} */
        S3AddressingStyle: "auto" | "path" | "virtual-hosted";
        S3ConfigViewDto: {
            accessKeyId: components["schemas"]["CredentialState"];
            addressingStyle: components["schemas"]["S3AddressingStyle"];
            bucket: string;
            endpointUrl: components["schemas"]["SafeEndpointViewDto"];
            region: string;
            requestTimeoutSeconds: components["schemas"]["RequestTimeoutSeconds"];
            secretAccessKey: components["schemas"]["CredentialState"];
            tlsVerification: components["schemas"]["S3TlsVerification"];
        };
        /** @enum {string} */
        S3TlsVerification: "verify" | "skip";
        SafeEndpointViewDto: {
            redacted: boolean;
            value: components["schemas"]["Nullable_String"];
        };
        /** Format: int64 */
        SafeInteger: number;
        /** Format: int64 */
        SafeUnsignedInteger: number;
        /** @enum {string} */
        SafeValidationMessage: "This field is required." | "This field has an invalid format." | "This field is outside the supported range." | "This field conflicts with another value." | "This field contains an unsafe value.";
        SearchMatchDto: {
            column: components["schemas"]["PositiveSafeInteger"];
            document: components["schemas"]["DocumentEntryDto"];
            line: components["schemas"]["PositiveSafeInteger"];
            preview: string;
        };
        SearchPageDto: {
            items: components["schemas"]["SearchMatchDto"][];
            nextCursor: components["schemas"]["Nullable_PageCursor"];
        };
        SearchQuery: string;
        SearchWorkspaceQuery: {
            cursor?: components["schemas"]["PageCursor"];
            limit?: components["schemas"]["PageLimit"];
            query: components["schemas"]["SearchQuery"];
        };
        ServerAuthenticationStatusDto: {
            initialization: components["schemas"]["ServerInitializationState"];
        };
        ServerFrame: {
            connectionId: components["schemas"]["ConnectionId"];
            instanceId: components["schemas"]["InstanceId"];
            protocolVersion: components["schemas"]["ProtocolVersion"];
            sequence: components["schemas"]["ReadySequence"];
            snapshotRequired: components["schemas"]["SnapshotRequired"];
            /** @enum {string} */
            type: "ready";
        } | {
            connectionId: components["schemas"]["ConnectionId"];
            event: components["schemas"]["DomainEvent"];
            protocolVersion: components["schemas"]["ProtocolVersion"];
            resource: components["schemas"]["ResourceRefDto"];
            revision: components["schemas"]["Revision"];
            sequence: components["schemas"]["EventSequence"];
            /** @enum {string} */
            type: "event";
        } | {
            connectionId: components["schemas"]["ConnectionId"];
            protocolVersion: components["schemas"]["ProtocolVersion"];
            reason: components["schemas"]["GapReason"];
            reloadScopes: components["schemas"]["ReloadScope"][];
            sequence: components["schemas"]["EventSequence"];
            /** @enum {string} */
            type: "gap";
        } | {
            code: components["schemas"]["FrameErrorCode"];
            message: string;
            protocolVersion: components["schemas"]["ProtocolVersion"];
            /** @enum {string} */
            type: "error";
        };
        /** @enum {string} */
        ServerInitializationState: "required" | "initialized" | "unavailable";
        ServerSessionDto: {
            state: components["schemas"]["ServerSessionState"];
        };
        /** @enum {string} */
        ServerSessionState: "authenticated";
        SettingEntryDto: {
            key: components["schemas"]["SettingKey"];
            value: components["schemas"]["SettingValueDto"];
        };
        /** @enum {string} */
        SettingKey: "appearance.mode" | "appearance.lightTheme" | "appearance.darkTheme" | "theme.customCss.light" | "theme.customCss.dark" | "language" | "editor.bodyFontSize" | "editor.contentWidth" | "editor.contentWidthPx" | "editor.fontFamily" | "editor.lineHeight" | "editor.paragraphSpacingPx" | "editor.showWordCount" | "editor.wrapCodeBlocks" | "editor.viewMode" | "files.ignoreRules" | "export.fontFamily" | "export.pdfAuthor" | "export.pdfFooter" | "export.pdfHeader" | "export.pdfHeightMm" | "export.pdfWidthMm" | "export.pdfMarginMm" | "export.pdfMarginPreset" | "export.pdfPageBreakOnH1" | "export.pdfPageSize";
        SettingValueDto: {
            /** @enum {string} */
            type: "boolean";
            value: boolean;
        } | {
            /** @enum {string} */
            type: "integer";
            value: components["schemas"]["SafeInteger"];
        } | {
            /** @enum {string} */
            type: "number";
            value: components["schemas"]["FiniteNumber"];
        } | {
            /** @enum {string} */
            type: "string";
            value: string;
        } | {
            /** @enum {string} */
            type: "nullable-integer";
            value: components["schemas"]["Nullable_SafeInteger"];
        } | {
            /** @enum {string} */
            type: "nullable-string";
            value: components["schemas"]["Nullable_String"];
        } | {
            /** @enum {string} */
            type: "font-family";
            value: components["schemas"]["FontFamilyValueDto"];
        };
        SettingsChangedEvent: {
            settings: components["schemas"]["SettingsSnapshotDto"];
            type: components["schemas"]["SettingsChangedKind"];
        };
        /** @enum {string} */
        SettingsChangedKind: "settings-changed";
        SettingsSnapshotDto: {
            revision: components["schemas"]["Revision"];
            values: components["schemas"]["SettingEntryDto"][];
        };
        /** Format: uuid */
        SnapshotId: string;
        /** @constant */
        SnapshotRequired: true;
        /** @enum {string} */
        StartupState: "starting" | "needs-owner" | "needs-workspace-initialization" | "needs-cloud-binding" | "ready" | "recoverable-error" | "fatal-error";
        /** @enum {string} */
        SyncCompletionState: "idle" | "attempting" | "failed" | "succeeded";
        SyncConfigChangedEvent: {
            config: components["schemas"]["SyncConfigViewDto"];
            type: components["schemas"]["SyncConfigChangedKind"];
        };
        /** @enum {string} */
        SyncConfigChangedKind: "sync-config-changed";
        SyncConfigChangesDto: {
            enabled?: boolean;
            generateConflictDocument?: boolean;
            intervalSeconds?: components["schemas"]["SyncIntervalSeconds"];
            mode?: components["schemas"]["SyncMode"];
            provider?: components["schemas"]["SyncProvider"];
            remoteRoot?: string;
            s3AccessKeyId?: components["schemas"]["CredentialChange"];
            s3AddressingStyle?: components["schemas"]["S3AddressingStyle"];
            s3Bucket?: string;
            s3EndpointUrl?: string;
            s3Region?: string;
            s3RequestTimeoutSeconds?: components["schemas"]["RequestTimeoutSeconds"];
            s3SecretAccessKey?: components["schemas"]["CredentialChange"];
            s3TlsVerification?: components["schemas"]["S3TlsVerification"];
            webdavPassword?: components["schemas"]["CredentialChange"];
            webdavServerUrl?: string;
            webdavUsername?: string;
        };
        /** @enum {string} */
        SyncConfigReadiness: "disabled" | "incomplete" | "ready";
        SyncConfigViewDto: {
            configured: boolean;
            enabled: boolean;
            generateConflictDocument: boolean;
            intervalSeconds: components["schemas"]["SyncIntervalSeconds"];
            issues: components["schemas"]["SyncIssueDto"][];
            mode: components["schemas"]["SyncMode"];
            provider: components["schemas"]["SyncProvider"];
            readiness: components["schemas"]["SyncConfigReadiness"];
            remoteRoot: string;
            revision: components["schemas"]["Revision"];
            s3: components["schemas"]["S3ConfigViewDto"];
            webdav: components["schemas"]["WebDavConfigViewDto"];
        };
        SyncConnectionTestDto: {
            checkedTarget: string;
            configRevision: components["schemas"]["Revision"];
            provider: components["schemas"]["SyncProvider"];
        };
        /** Format: int32 */
        SyncIntervalSeconds: number;
        /** @enum {string} */
        SyncIssueCode: "required" | "invalid-url" | "unsafe-url-components" | "out-of-range" | "invalid-path";
        SyncIssueDto: {
            code: components["schemas"]["SyncIssueCode"];
            field: string;
            message: string;
        };
        /** @enum {string} */
        SyncMode: "automatic" | "startup-exit" | "fully-manual";
        /** @enum {string} */
        SyncProvider: "s3" | "webdav";
        SyncRunAcceptedDto: {
            acceptedAt: components["schemas"]["Rfc3339Utc"];
            configRevision: components["schemas"]["Revision"];
            runId: components["schemas"]["RunId"];
        };
        /** @enum {string} */
        SyncRunCompletionState: "attempting" | "failed" | "succeeded";
        SyncRunStatusDto: {
            acceptedAt: components["schemas"]["Rfc3339Utc"];
            completionState: components["schemas"]["SyncRunCompletionState"];
            configRevision: components["schemas"]["Revision"];
            error: components["schemas"]["Nullable_SyncSafeErrorDto"];
            finishedAt: components["schemas"]["Nullable_Rfc3339Utc"];
            provider: components["schemas"]["SyncProvider"];
            runId: components["schemas"]["RunId"];
            summary: components["schemas"]["Nullable_SyncSummaryDto"];
        };
        SyncSafeErrorDto: {
            category?: string;
            code: string;
            httpStatus?: components["schemas"]["HttpStatus"];
            method?: string;
            objectId?: string;
            operation: string;
            provider: components["schemas"]["SyncProvider"];
            providerErrorCode?: string;
            relativePath?: components["schemas"]["WorkspaceRelativePath"];
            requestId?: components["schemas"]["RequestId"];
            runId?: components["schemas"]["RunId"];
        };
        SyncStatusChangedEvent: {
            status: components["schemas"]["SyncStatusDto"];
            type: components["schemas"]["SyncStatusChangedKind"];
        };
        /** @enum {string} */
        SyncStatusChangedKind: "sync-status-changed";
        SyncStatusDto: {
            activeRunId: components["schemas"]["Nullable_RunId"];
            completionState: components["schemas"]["SyncCompletionState"];
            configRevision: components["schemas"]["Nullable_Revision"];
            error: components["schemas"]["Nullable_SyncSafeErrorDto"];
            lastAttemptAt: components["schemas"]["Nullable_Rfc3339Utc"];
            lastSuccessfulSyncAt: components["schemas"]["Nullable_Rfc3339Utc"];
            lastTrigger: components["schemas"]["Nullable_SyncTrigger"];
            provider: components["schemas"]["SyncProvider"];
            summary: components["schemas"]["Nullable_SyncSummaryDto"];
        };
        SystemVersionResponse: {
            apiVersion: components["schemas"]["ApiVersion"];
            instanceId: components["schemas"]["InstanceId"];
            kernelVersion: string;
        };
        TestSyncConnectionRequest: {
            changes: components["schemas"]["SyncConfigChangesDto"];
            expectedRevision: components["schemas"]["Revision"];
        };
        TriggerSyncRunRequest: {
            expectedConfigRevision: components["schemas"]["Revision"];
        };
        UpdateDocumentRequest: {
            contents: components["schemas"]["DocumentContents"];
            expectedRevision: components["schemas"]["Revision"];
            workspaceGeneration: components["schemas"]["WorkspaceGeneration"];
        };
        /** @enum {string} */
        ValidationField: "request" | "workspaceGeneration" | "parent" | "name" | "kind" | "contents" | "expectedRevision" | "targetParent" | "deletionPolicy" | "cursor" | "limit" | "query" | "snapshotId" | "values" | "changes" | "provider" | "mode" | "remoteRoot" | "intervalSeconds" | "webdav" | "s3" | "endpointUrl" | "username" | "password" | "accessKeyId" | "secretAccessKey" | "bucket" | "region" | "addressingStyle" | "tlsVerification" | "expectedConfigRevision";
        /** @enum {string} */
        ValidationIssueCode: "required" | "invalid-format" | "out-of-range" | "conflict" | "unsafe-value";
        ValidationIssueDto: {
            code: components["schemas"]["ValidationIssueCode"];
            field: components["schemas"]["ValidationField"];
            message: components["schemas"]["SafeValidationMessage"];
        };
        ValidationIssues: components["schemas"]["ValidationIssueDto"][];
        WebDavConfigViewDto: {
            password: components["schemas"]["CredentialState"];
            serverUrl: components["schemas"]["SafeEndpointViewDto"];
            username: string;
        };
        WorkspaceChangedEvent: {
            type: components["schemas"]["WorkspaceChangedKind"];
            workspace: components["schemas"]["WorkspaceDto"];
        };
        /** @enum {string} */
        WorkspaceChangedKind: "workspace-changed";
        WorkspaceDto: {
            displayName: string;
            generation: components["schemas"]["WorkspaceGeneration"];
            id: components["schemas"]["WorkspaceId"];
            readiness: components["schemas"]["WorkspaceReadiness"];
            revision: components["schemas"]["Revision"];
        };
        WorkspaceGeneration: string;
        /** Format: uuid */
        WorkspaceId: string;
        WorkspaceInventoryEntryDto: {
            document: components["schemas"]["DocumentEntryDto"];
            /** @enum {string} */
            entryType: "document";
        } | {
            /** @enum {string} */
            entryType: "resource";
            resource: components["schemas"]["ResourceEntryDto"];
        };
        WorkspaceInventoryPageDto: {
            items: components["schemas"]["WorkspaceInventoryEntryDto"][];
            nextCursor: components["schemas"]["Nullable_PageCursor"];
        };
        /** @enum {string} */
        WorkspaceReadiness: "ready" | "initializing" | "unavailable" | "locked";
        WorkspaceRelativePath: string;
    };
    responses: never;
    parameters: never;
    requestBodies: never;
    headers: never;
    pathItems: never;
}
export type $defs = Record<string, never>;
export interface operations {
    initializeServerOwner: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["InitializeServerOwnerRequest"];
            };
        };
        responses: {
            /** @description Success */
            201: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ServerSessionDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_credentials";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            409: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "already_initialized";
                    };
                };
            };
            /** @description Error */
            429: {
                headers: {
                    /** @description Whole seconds until another authentication attempt is allowed. */
                    "Retry-After": components["schemas"]["PositiveSafeInteger"];
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code: "authentication_rate_limited";
                        details: {
                            retryAfterSeconds: components["schemas"]["PositiveSafeInteger"];
                            /** @enum {string} */
                            type: "rate-limit";
                        };
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable";
                    };
                };
            };
        };
    };
    logoutServerSession: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            204: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "csrf_rejected" | "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable";
                    };
                };
            };
        };
    };
    changeServerOwnerPassword: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["ChangeServerOwnerPasswordRequest"];
            };
        };
        responses: {
            /** @description Success */
            204: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_credentials" | "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "csrf_rejected" | "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            429: {
                headers: {
                    /** @description Whole seconds until another authentication attempt is allowed. */
                    "Retry-After": components["schemas"]["PositiveSafeInteger"];
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code: "authentication_rate_limited";
                        details: {
                            retryAfterSeconds: components["schemas"]["PositiveSafeInteger"];
                            /** @enum {string} */
                            type: "rate-limit";
                        };
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable";
                    };
                };
            };
        };
    };
    getServerSession: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ServerSessionDto"];
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable";
                    };
                };
            };
        };
    };
    createServerSession: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CreateServerSessionRequest"];
            };
        };
        responses: {
            /** @description Success */
            201: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ServerSessionDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_credentials";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            409: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "initialization_required";
                    };
                };
            };
            /** @description Error */
            429: {
                headers: {
                    /** @description Whole seconds until another authentication attempt is allowed. */
                    "Retry-After": components["schemas"]["PositiveSafeInteger"];
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code: "authentication_rate_limited";
                        details: {
                            retryAfterSeconds: components["schemas"]["PositiveSafeInteger"];
                            /** @enum {string} */
                            type: "rate-limit";
                        };
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable";
                    };
                };
            };
        };
    };
    getAuthenticationStatus: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ServerAuthenticationStatusDto"];
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable";
                    };
                };
            };
        };
    };
    listDocuments: {
        parameters: {
            query?: {
                cursor?: components["schemas"]["PageCursor"];
                limit?: components["schemas"]["PageLimit"];
                parent?: components["schemas"]["WorkspaceRelativePath"];
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["DocumentPageDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request" | "invalid_workspace_path";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
    createDocument: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["CreateDocumentRequest"];
            };
        };
        responses: {
            /** @description Success */
            201: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["CreatedDocumentDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_document_name" | "invalid_request" | "invalid_workspace_path";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "csrf_rejected" | "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            409: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_already_exists" | "revision_conflict";
                    };
                };
            };
            /** @description Error */
            413: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_too_large";
                    };
                };
            };
            /** @description Error */
            422: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_invalid_encoding";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
    getDocument: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                documentId: components["schemas"]["DocumentId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["DocumentContentDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            404: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_not_found";
                    };
                };
            };
            /** @description Error */
            422: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_invalid_encoding";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
    updateDocument: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                documentId: components["schemas"]["DocumentId"];
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["UpdateDocumentRequest"];
            };
        };
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["DocumentContentDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "csrf_rejected" | "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            404: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_not_found";
                    };
                };
            };
            /** @description Error */
            409: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "revision_conflict";
                    };
                };
            };
            /** @description Error */
            413: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_too_large";
                    };
                };
            };
            /** @description Error */
            422: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_invalid_encoding";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
    deleteDocument: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                documentId: components["schemas"]["DocumentId"];
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["DeleteDocumentRequest"];
            };
        };
        responses: {
            /** @description Success */
            204: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content?: never;
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "csrf_rejected" | "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            404: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_not_found";
                    };
                };
            };
            /** @description Error */
            409: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "revision_conflict";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
    listDocumentHistory: {
        parameters: {
            query?: {
                cursor?: components["schemas"]["PageCursor"];
                limit?: components["schemas"]["PageLimit"];
            };
            header?: never;
            path: {
                documentId: components["schemas"]["DocumentId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["DocumentHistoryPageDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            404: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_not_found";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
    getDocumentHistory: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                documentId: components["schemas"]["DocumentId"];
                snapshotId: components["schemas"]["SnapshotId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["DocumentHistorySnapshotDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            404: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_not_found";
                    };
                };
            };
            /** @description Error */
            413: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_too_large";
                    };
                };
            };
            /** @description Error */
            422: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_invalid_encoding";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
    restoreDocumentHistory: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                documentId: components["schemas"]["DocumentId"];
                snapshotId: components["schemas"]["SnapshotId"];
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["RestoreDocumentHistoryRequest"];
            };
        };
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["DocumentContentDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "csrf_rejected" | "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            404: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_not_found";
                    };
                };
            };
            /** @description Error */
            409: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "revision_conflict";
                    };
                };
            };
            /** @description Error */
            413: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_too_large";
                    };
                };
            };
            /** @description Error */
            422: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_invalid_encoding";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
    moveDocument: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                documentId: components["schemas"]["DocumentId"];
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["MoveDocumentRequest"];
            };
        };
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["DocumentEntryDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_document_name" | "invalid_request" | "invalid_workspace_path";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "csrf_rejected" | "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            404: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_not_found";
                    };
                };
            };
            /** @description Error */
            409: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_already_exists" | "revision_conflict";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
    createWorkspaceResource: {
        parameters: {
            query: {
                workspaceGeneration: components["schemas"]["WorkspaceGeneration"];
                folder: components["schemas"]["WorkspaceRelativePath"];
                name: components["schemas"]["ResourceName"];
                kind: components["schemas"]["ResourceKind"];
            };
            header?: never;
            path: {
                documentId: components["schemas"]["DocumentId"];
            };
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/octet-stream": string;
                "image/gif": string;
                "image/jpeg": string;
                "image/png": string;
                "image/webp": string;
            };
        };
        responses: {
            /** @description Success */
            201: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ResourceEntryDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "csrf_rejected" | "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            404: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_not_found";
                    };
                };
            };
            /** @description Error */
            409: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "revision_conflict";
                    };
                };
            };
            /** @description Error */
            413: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "resource_too_large";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
    healthLive: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["LiveHealthResponse"];
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
        };
    };
    healthReady: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ReadyHealthResponse"];
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready";
                    };
                };
            };
        };
    };
    listWorkspaceInventory: {
        parameters: {
            query?: {
                cursor?: components["schemas"]["PageCursor"];
                limit?: components["schemas"]["PageLimit"];
                parent?: components["schemas"]["WorkspaceRelativePath"];
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["WorkspaceInventoryPageDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request" | "invalid_workspace_path";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
    openWorkspaceResource: {
        parameters: {
            query: {
                kind: components["schemas"]["ResourceKind"];
            };
            header?: never;
            path: {
                resourceId: components["schemas"]["ResourceId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Exact resource size in bytes. */
                    "Content-Length": number;
                    /** @description Disables content sniffing for untrusted workspace resources. */
                    "X-Content-Type-Options": "nosniff";
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/octet-stream": string;
                    "image/gif": string;
                    "image/jpeg": string;
                    "image/png": string;
                    "image/webp": string;
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            404: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "resource_not_found";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
    getRuntimeState: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["RuntimeStateDto"];
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready";
                    };
                };
            };
        };
    };
    searchWorkspace: {
        parameters: {
            query: {
                query: components["schemas"]["SearchQuery"];
                cursor?: components["schemas"]["PageCursor"];
                limit?: components["schemas"]["PageLimit"];
            };
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SearchPageDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            422: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "document_invalid_encoding";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
    getSettings: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SettingsSnapshotDto"];
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "settings_unavailable";
                    };
                };
            };
        };
    };
    patchSettings: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PatchSettingsRequest"];
            };
        };
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SettingsSnapshotDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "csrf_rejected" | "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            409: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "settings_revision_conflict";
                    };
                };
            };
            /** @description Error */
            422: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_settings_field";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "settings_unavailable";
                    };
                };
            };
        };
    };
    getSyncConfig: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SyncConfigViewDto"];
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            404: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "sync_config_absent";
                    };
                };
            };
            /** @description Error */
            422: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "sync_config_invalid";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable";
                    };
                };
            };
        };
    };
    patchSyncConfig: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["PatchSyncConfigRequest"];
            };
        };
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SyncConfigViewDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "csrf_rejected" | "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            404: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "sync_config_absent";
                    };
                };
            };
            /** @description Error */
            409: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "sync_config_revision_conflict";
                    };
                };
            };
            /** @description Error */
            422: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "sync_config_invalid";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "sync_not_ready";
                    };
                };
            };
        };
    };
    testSyncConnection: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["TestSyncConnectionRequest"];
            };
        };
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SyncConnectionTestDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "csrf_rejected" | "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            404: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "sync_config_absent";
                    };
                };
            };
            /** @description Error */
            409: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "sync_config_revision_conflict";
                    };
                };
            };
            /** @description Error */
            422: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "sync_config_invalid";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "sync_not_ready";
                    };
                };
            };
        };
    };
    triggerSyncRun: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody: {
            content: {
                "application/json": components["schemas"]["TriggerSyncRunRequest"];
            };
        };
        responses: {
            /** @description Success */
            202: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SyncRunAcceptedDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "csrf_rejected" | "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            409: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "sync_config_revision_conflict";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "sync_not_ready" | "sync_run_unavailable";
                    };
                };
            };
        };
    };
    getSyncRun: {
        parameters: {
            query?: never;
            header?: never;
            path: {
                runId: components["schemas"]["RunId"];
            };
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SyncRunStatusDto"];
                };
            };
            /** @description Error */
            400: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "invalid_request";
                    };
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            404: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "resource_not_found";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "sync_not_ready";
                    };
                };
            };
        };
    };
    getSyncStatus: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SyncStatusDto"];
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "sync_not_ready";
                    };
                };
            };
        };
    };
    getSystemVersion: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["SystemVersionResponse"];
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready";
                    };
                };
            };
        };
    };
    getWorkspace: {
        parameters: {
            query?: never;
            header?: never;
            path?: never;
            cookie?: never;
        };
        requestBody?: never;
        responses: {
            /** @description Success */
            200: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["WorkspaceDto"];
                };
            };
            /** @description Error */
            401: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "unauthorized";
                    };
                };
            };
            /** @description Error */
            403: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "host_not_allowed" | "origin_not_allowed";
                    };
                };
            };
            /** @description Error */
            423: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "workspace_locked";
                    };
                };
            };
            /** @description Error */
            500: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "internal_error";
                    };
                };
            };
            /** @description Error */
            503: {
                headers: {
                    /** @description Correlation ID for this response. */
                    "X-Request-Id": string;
                    [name: string]: unknown;
                };
                content: {
                    "application/json": components["schemas"]["ApiErrorEnvelope"] & {
                        /** @enum {string} */
                        code?: "authentication_unavailable" | "kernel_not_ready" | "workspace_unavailable";
                    };
                };
            };
        };
    };
}
