# Remove Daily Note and CalDAV Design

## Outcome

Remove the complete Daily Note feature and the CalDAV calendar service from QingYu. Existing user documents remain ordinary readable documents, historical `custom-dailynote-*` attributes remain untouched, and existing files under the CalDAV storage directory are not deleted or migrated.

## Scope

The Daily Note removal is a vertical deletion. Desktop, mobile, and native application menu entries disappear together with the shortcut, command-palette command, last-selected-notebook storage key, notebook Daily Note settings, WebSocket file-tree event handling, HTTP endpoints, kernel creation logic, CLI command, MCP tool, configuration fields, import behavior, localization, API documentation, and built-in user-guide material.

The CalDAV removal deletes the service registration, `/.well-known/caldav` and `/caldav/*` routes, CalDAV-specific method and CORS handling, Basic Authentication path handling, and the CalDAV model backend. The removed routes receive no disabled compatibility handlers.

## Preserved Boundaries

- Existing documents and their block attributes are not rewritten, migrated, or deleted.
- Existing CalDAV files on disk are not deleted; they become inert because no runtime code loads or serves them.
- CardDAV and WebDAV remain registered and authenticated.
- The shared `github.com/emersion/go-webdav` dependency remains because CardDAV still uses it.
- Shared DAV filesystem helpers remain, but CalDAV type support is removed from their signatures and comments.
- Attribute View date columns, ordinary date values, calendar-shaped icons used by non-calendar UI, templates, and generic Go-template date functions remain.
- Existing unknown Daily Note fields in stored JSON and stale frontend local-storage values receive no migration. Current decoders ignore them, and later normal saves may naturally omit fields no longer present in the schema.

## Components

### Daily Note frontend

Remove `fetchNewDailyNote` and `newDailyNote` from `app/src/util/mount.ts`. Remove all consumers in workspace and mobile menus, global keyboard and command dispatch, command-panel lists, hotkey correction, and the macOS native-menu state/template. Remove `LOCAL_DAILYNOTEID`, the default `dailyNote` keymap entry, and the TypeScript key declaration. Remove notebook configuration controls and request fields for Daily Note paths. Remove the now-unreachable `createdailynote` file-tree event cases.

### Daily Note backend and automation surfaces

Unregister `/api/filetree/createDailyNote`, `/api/block/appendDailyNoteBlock`, and `/api/block/prependDailyNoteBlock`, then delete their handlers. Delete `model.CreateDailyNote`, its Daily Note attribute constant, the `dailynote` Cobra command, and the MCP `dailynote` tool. Remove `DailyNoteSavePath` and `DailyNoteTemplatePath` from `BoxConf`, defaults, notebook-setting normalization, and imported-notebook configuration copying.

### CalDAV service

Delete `kernel/model/caldav.go`. In `kernel/server/serve.go`, remove the CalDAV import, method set, registration call, handler, and CORS branch while preserving WebDAV and CardDAV registration. Remove `/caldav` from Basic Authentication handling in `kernel/model/session.go`. Narrow `kernel/model/dav.go` metadata persistence to `[]*carddav.AddressBook` and retain the shared path, ETag, and CardDAV persistence helpers.

### Documentation and localization

Remove `dailyNote`, `fileTree11`, `fileTree14`, and `fileTree15` from all 21 localization files after their consumers are gone. Remove Daily Note fields from the English, Simplified Chinese, and Japanese API examples and from workspace-format documentation. Remove Daily Note pages, navigation references, CLI/MCP command sections, and developer API guidance from the four shipped guide notebooks. Update the earlier feature-removal documentation so it no longer states that Daily Note remains.

## Data and Compatibility

No user-data cleanup runs. A document created as a Daily Note remains a normal `.sy` document at the same path and keeps any `custom-dailynote-YYYYMMDD` attribute. Stored notebook JSON may still contain old Daily Note keys until the notebook configuration is saved through current code. Stored CalDAV metadata and `.ics` files remain under `data/storage/caldav`, but QingYu no longer registers routes or loads them.

Public Daily Note API clients, the `dailynote` CLI command, the MCP tool, and CalDAV clients lose their endpoints immediately. This is intentional product-surface removal, so no deprecation shim is added.

## Verification

- Route tests assert all three Daily Note HTTP endpoints are absent while representative document APIs remain.
- MCP boundary tests assert `dailynote` is absent while generic document tools remain.
- CLI tests assert no `dailynote` root command exists.
- Native-menu tests assert `dailyNote` is not allowlisted, localized, or rendered.
- Server route tests assert `/caldav` and `/.well-known/caldav` are absent while `/carddav`, `/.well-known/carddav`, and `/webdav` remain.
- Targeted Go tests cover API, MCP, CLI, server, model, and configuration packages.
- Frontend verification uses `cd app && pnpm run lint`; no frontend build is run.
- Localization verification uses `python scripts/check-lang-keys.py`.
- Residual scans reject Daily Note and CalDAV product-code references while allowing historical user-document attributes, inert stored data, CardDAV, ordinary date features, and generic calendar icons.
