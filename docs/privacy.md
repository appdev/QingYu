# Privacy And Data Flow

QingYu is local-first. You can open, edit, and save Markdown without an account, cloud workspace, or QingYu-hosted storage service.

This document explains what stays local and what can leave the device when optional features are configured.

## By Default

- Markdown files are ordinary files in the current desktop workspace, the Docker `/data/workspace` volume, or the mobile application-managed workspace.
- Desktop, Server Web/Docker, Android, and iOS read and write application settings and durable UI state through the same Kernel AppConfig service.
- The official Docker/Web client does not use browser storage or process memory as the authoritative source for settings, open documents, recent files, drafts, or layout.
- Current-notebook sync runs only when the application-level configuration is enabled and a permitted trigger runs.
- QingYu does not provide an account system or hosted document storage.

## Local Data

QingYu may store these items locally:

- editor preferences, theme choices, keyboard shortcuts, export settings, recent files, workspace layout, open tabs, draft state, file-tree sort settings, and the local Pandoc path in Kernel AppConfig `settings.json`
- the selected desktop workspace authorization and path in `primary-workspace.json`; this is desktop-only bootstrap metadata and contains no UI layout, recent files, drafts, or normal settings
- WebDAV or S3-compatible synchronization settings and credentials in `sync-config.json`
- synchronization manifests, status, staging, and quarantined conflicts below `sync-state/`
- the desktop-only device MCP policy in `mcp.json`
- desktop MCP IPC, audit, and other runtime-only state stored below the app-data `mcp-runtime/` directory

Durable configuration files are grouped under one platform ConfigRoot: the QingYu application-data directory on desktop, `/data/config` in Docker, and the application sandbox configuration directory on mobile. They remain separate typed documents because AppConfig, synchronization secrets, MCP policy, and desktop pre-Kernel workspace selection have different validation and security rules. Operational manifests, journals, checkpoints, locks, and recovery metadata remain under the platform state root; Docker uses `/data/state` and never stores a second `settings.json` or `sync-config.json` there.

WebDAV and S3-compatible credentials are intentionally stored as plaintext in `sync-config.json`, together with the endpoint, account, remote path, storage choice, and trigger policy. Anyone or any tool that can read the application's private data may read those credentials. Local AppConfig state, `sync-config.json`, `mcp.json`, `primary-workspace.json`, sync operational state, and `mcp-runtime/` are never included in QingYu synchronization. The application is unreleased, so development-era `local-state.json`, browser settings, Plugin Store data, and old settings locations are not imported, migrated, or used as fallbacks.

## Current-Notebook And Standalone Resources

QingYu does not use a separate remote image uploader. Resources follow the current document context:

- In a document below the current notebook directory, pasted, dropped, imported, and downloaded resources are copied into that directory's root lowercase `assets/` directory, whether synchronization is enabled or disabled. If synchronization is enabled, they transfer through the same engine as notes and other ordinary notebook files.
- In a standalone saved document, existing local dropped or imported resources remain filesystem references. Clipboard resources are copied to an `assets/` directory adjacent to the document.
- An unsaved standalone document must be saved before a clipboard resource can be stored. Existing local references and remote URLs remain references.

When you explicitly add an image from an internet URL to a current-notebook document, QingYu requests that image so it can be stored with the note. The request goes only to that URL and any redirects it returns. A standalone file never starts or retargets synchronization.

## Current-Notebook Synchronization

WebDAV and S3-compatible synchronization is optional and disabled by default. Desktop users select or switch the current notebook directory, and one application-level provider configuration follows that desktop selection. Choosing another desktop directory switches the current notebook while keeping the provider configuration. QingYu does not support a temporary external-folder session. Opening or focusing a standalone desktop file does not change the current notebook, provider configuration, status, or trigger policy.

Desktop synchronization can upload, download, delete, and preserve conflict copies for ordinary files below the current notebook. Its remote path is `notes/<directory-name>/`; portable `settings.json` uses the separate remote `app/` namespace. On a new desktop device, the remote catalog can list notebook directory names and downloads only the one the user selects.

Android and iOS are permanently bound to their own application-managed fixed workspace. Mobile synchronization uses that fixed workspace and the device's one application-level provider configuration; it does not expose a named-workspace catalog, a local workspace/root selector, or a remote notebook/root selector. Enabling synchronization on any supported client requires an available workspace and complete settings for the selected storage service.

QingYu excludes `.qingyu/` and the legacy `.markra-sync/` directory from its own synchronization, file tree, workspace search, and watcher so stale configuration or secrets cannot be uploaded. Neither directory is read, migrated, rewritten, or deleted. These exclusions do not control Git, cloud-drive clients, backup tools, or other third-party software.

Portable settings can include allowlisted appearance, theme, language, editor, file-ignore, keyboard-shortcut, and export preferences. They are validated before application and can synchronize independently from note content. UI layout, open tabs, drafts, recent files, file-tree sort, Pandoc path, MCP policy, device-specific paths, credentials, manifests, runtime endpoints, audit data, installed theme packages, and extension directories remain local. Replacing portable settings preserves these local AppConfig fields.

## Desktop MCP

Desktop MCP is optional and disabled by default. One device-local policy controls permissions, confirmation and dry-run behavior, operation limits, and auditing for every MCP client on that device. Document tools are limited to the current notebook directory; a standalone file does not retarget that authority. Without an available current notebook and ready child Kernel, document, application-settings, and sync tools fail closed while the device-local MCP policy remains editable in QingYu settings.

MCP clients connect to the bundled stdio bridge, which forwards requests over private local IPC to the QingYu desktop host. QingYu does not open an MCP HTTP/TCP listener for external clients and does not use the operating-system credential store for that transport. The desktop host keeps policy, confirmation, and audit enforcement, then calls its child Kernel over authenticated loopback HTTP with a short-lived Bearer credential. MCP clients receive opaque, process-scoped identifiers instead of that credential, direct filesystem access, or absolute paths.

The MCP policy, local IPC endpoints, audit entries, process keys, and workspace handles remain device-local and are not included in application-settings export or synchronization. The canonical desktop policy lives only in `mcp.json`; QingYu does not import it from `local-state.json` or `settings.json`.

## Desktop, Server Web, And Mobile Differences

The desktop app can access native file paths, switch the current notebook directory, open standalone Markdown files, run the local MCP service, and synchronize the current notebook through WebDAV or S3-compatible storage. Its native host keeps only `primary-workspace.json` as pre-Kernel workspace-selection metadata; after Kernel startup it uses the same AppConfig behavior as every other client.

Mobile uses one fixed application-managed workspace and the embedded Kernel. It has no desktop directory picker, Plugin Store state authority, local MCP service, MCP policy file, IPC transport, tool registry, or MCP filesystem authority. Server Web/Docker uses the fixed `/data/workspace` root and authenticated Kernel HTTP/WebSocket contract. A browser is a client of that Docker instance: refreshing it or opening another browser reads the same committed `/data/config/settings.json`; browser IndexedDB is not an authoritative official-runtime store.

## Other Network Access

The desktop or mobile app can access the network when you explicitly add an image from an internet URL or configure current-notebook synchronization. The desktop app can also check for application updates. These features use their configured service endpoints and the runtime's default network behavior.
