# Privacy And Data Flow

QingYu is local-first. You can open, edit, and save Markdown without an account, cloud workspace, or QingYu-hosted storage service.

This document explains what stays local and what can leave the device when optional features are configured.

## By Default

- Markdown files are ordinary files on disk or browser-selected file handles.
- Desktop settings are stored locally by the Tauri app.
- Web settings are stored in the browser through IndexedDB.
- Current-notebook sync runs only when the application-level configuration is enabled and a permitted trigger runs.
- QingYu does not provide an account system or hosted document storage.

## Local Data

QingYu may store these items locally:

- editor preferences, theme choices, keyboard shortcuts, and export settings
- recent standalone files and notebook directories
- workspace state, open tabs, draft state, file history, and file tree sort settings
- the device-local current-notebook path or managed notebook name in `local-state.json`
- WebDAV or S3-compatible synchronization settings and credentials in `sync-config.json`
- synchronization manifests, status, staging, and quarantined conflicts below `sync-state/`
- application-level MCP policy stored with the portable application settings in `settings.json`
- desktop MCP IPC, audit, and other runtime-only state stored below the app-data `mcp-runtime/` directory

These files live in QingYu's application-data directory, not in the notes workspace. WebDAV and S3-compatible credentials are intentionally stored as plaintext in `sync-config.json`, together with the endpoint, account, remote path, storage choice, and trigger policy. Anyone or any tool that can read the application's private data may read those credentials. `local-state.json`, `sync-config.json`, `sync-state/`, and `mcp-runtime/` are never included in QingYu synchronization.

## Current-Notebook And Standalone Resources

QingYu does not use a separate remote image uploader. Resources follow the current document context:

- In a document below the current notebook directory, pasted, dropped, imported, and downloaded resources are copied into that directory's root lowercase `assets/` directory, whether synchronization is enabled or disabled. If synchronization is enabled, they transfer through the same engine as notes and other ordinary notebook files.
- In a standalone saved document, existing local dropped or imported resources remain filesystem references. Clipboard resources are copied to an `assets/` directory adjacent to the document.
- An unsaved standalone document must be saved before a clipboard resource can be stored. Existing local references and remote URLs remain references.

When you explicitly add an image from an internet URL to a current-notebook document, QingYu requests that image so it can be stored with the note. The request goes only to that URL and any redirects it returns. A standalone file never starts or retargets synchronization.

## Current-Notebook Synchronization

WebDAV and S3-compatible synchronization is optional and disabled by default. One application-level configuration applies to whichever notebook directory is current on that device. Desktop users select or switch that directory; mobile selects one named child below its application-managed `workspaces/` directory. Enabling synchronization requires a valid current notebook and complete settings for the selected storage service.

Choosing another directory switches the current notebook while keeping the same application-level provider configuration. QingYu does not support a temporary external-folder session. Opening or focusing a standalone file does not change the current notebook, provider configuration, status, or trigger policy. Synchronization can upload, download, delete, and preserve conflict copies for ordinary files below the current notebook. Its remote path is `notes/<directory-name>/`; portable `settings.json` uses the separate remote `app/` namespace. On a new device, the remote catalog lists notebook directory names and downloads only the one the user selects.

QingYu excludes `.qingyu/` and the legacy `.markra-sync/` directory from its own synchronization, file tree, workspace search, and watcher so stale configuration or secrets cannot be uploaded. Neither directory is read, migrated, rewritten, or deleted. These exclusions do not control Git, cloud-drive clients, backup tools, or other third-party software.

Portable settings can include the selected theme, custom theme CSS, layout, keyboard shortcuts, export preferences, and the MCP policy. They are validated before application and can synchronize independently from note content. Device-specific paths, recent/local window state, credentials, manifests, runtime endpoints, audit data, installed theme packages, and extension directories remain local in this version.

## Desktop MCP

Desktop MCP is optional and disabled by default. One application-level policy controls permissions, confirmation and dry-run behavior, operation limits, and auditing for every MCP client. Document tools are limited to the current notebook directory; a standalone file does not retarget that authority. Without an available current notebook, document tools fail closed while application settings and sync policy remain editable.

MCP clients connect to the bundled stdio bridge, which forwards requests over private local IPC to the QingYu process. QingYu does not open an MCP HTTP/TCP listener and does not use the operating-system credential store for MCP transport. Document operations are executed by QingYu's application services; MCP clients receive opaque, process-scoped identifiers instead of direct filesystem access or absolute paths.

The portable MCP policy can synchronize with other application settings. Local IPC endpoints, audit entries, process keys, and workspace handles remain device-local runtime state and are not included in settings synchronization.

## Desktop And Web Differences

The desktop app can access native file paths, switch the current notebook directory, open standalone Markdown files, run the local MCP service, and synchronize the current notebook through WebDAV or S3-compatible storage. Mobile can keep multiple named managed notebooks below `workspaces/` while selecting only one current notebook for editing and synchronization. It can edit the portable MCP policy but does not include the local MCP server, IPC transport, tool registry, audit log, or MCP notebook filesystem authority. The web editor runs inside browser permission and CORS limits, so it uses browser file handles, downloads, print-to-PDF, and IndexedDB settings; it does not run current-notebook synchronization or the local MCP service.

## Other Network Access

The desktop or mobile app can access the network when you explicitly add an image from an internet URL or configure current-notebook synchronization. The desktop app can also check for application updates. These features use their configured service endpoints and the runtime's default network behavior.
