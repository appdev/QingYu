# Complete Application Proxy Removal Design

## Summary

QingYu will remove its application-owned network proxy feature completely. This removal covers the desktop settings surface, persisted and portable settings, diagnostics, TypeScript request contracts, updater and web-resource bridges, and Rust HTTP client configuration.

This is not a removal of networking. WebDAV sync, S3 sync, update checks, and web-image downloads remain supported. They will use their existing runtime or HTTP client's default network behavior without a QingYu-specific proxy override.

## Product Boundary

### Remove

- The desktop **Network** settings category.
- HTTP, HTTPS, SOCKS5, and SOCKS5H application proxy controls.
- Proxy enablement, proxy URL, and local-address bypass settings.
- Proxy settings persistence and settings import/export fields.
- Proxy information in diagnostics.
- Proxy settings passed through update, web-resource, WebDAV, and S3 request contracts.
- Native proxy validation and HTTP client customization.
- Tests, translations, exports, and documentation that exist only for the proxy feature.

### Preserve

- WebDAV and S3 project-folder sync providers, configuration, triggers, conflict handling, and status reporting.
- Update checking and installation behavior.
- Web-image download behavior.
- Existing private-network and local-address security checks that are independent of proxy configuration.
- Sync configuration recovery safety copies and settings import/export for all remaining supported settings.
- Normal transport, HTTP status, authentication, synchronization, and security errors.

## Compatibility Policy

There are no users and no compatibility requirement. The removal will not add a migration, deprecated type, hidden compatibility field, or fallback reader for old proxy settings.

Any old unknown key left in a developer-local settings store is ignored because no supported code reads or writes it. Portable settings exports no longer contain a network section, and imports ignore unsupported extra data through the existing schema boundary rather than restoring proxy behavior.

## Architecture

### Settings and Application State

Delete the `NetworkSettings` component and its tests. Remove the `network` category from the desktop settings category type, sidebar, active-section routing, settings-window state, and settings-section exports. Remove all proxy translation keys from every locale and from the typed i18n key union.

Delete the `NetworkSettings` model, defaults, normalization, persistent store accessors, and exports. Remove the network field from portable settings collection, validation, import, and export. Remove proxy fields from diagnostics input and output. Clean the application test harness of network-settings mocks and fixtures.

### TypeScript Request Flow

Networking call sites stop loading stored network settings. Requests for project sync, project configuration connection tests, web resources, and updater checks no longer contain a `network`, `proxy`, or proxy-derived option.

The resulting request flow is:

1. A product feature starts an update, web-image, WebDAV, or S3 operation.
2. The TypeScript runtime sends only the feature-specific request data.
3. The native command creates or uses its default HTTP client.
4. Existing provider and transport handling processes the result.

### Rust Runtime

Delete the native `network` module, `NetworkSettings` structure, proxy URL validation, local-address proxy exclusion logic, and `apply_network_settings` helper.

Remove network fields from native request structures and command boundaries. WebDAV, S3, project-config connection tests, and web-image HTTP clients are constructed without proxy customization. Tests and fixtures no longer serialize network settings into native requests.

The change must not remove provider credentials, endpoint configuration, private-network authorization, redirect protection, URL validation, or error redaction.

### Documentation

Remove proxy instructions and product claims from current documentation. Update forward-looking plans that would otherwise expose or reintroduce `network.proxyUrl`, especially the QingYu MCP plan. Older implementation-history documents may retain historical descriptions, but no active contract, acceptance document, or future plan may define proxy support as a current capability.

## Error Handling

Proxy-specific validation and error messages disappear because no proxy value is accepted. Existing errors remain unchanged for:

- unreachable WebDAV or S3 services;
- invalid provider endpoints;
- HTTP and authentication failures;
- update-service failures;
- unsafe or unauthorized private-network image requests;
- synchronization conflicts and manifest failures.

No new fallback or compatibility error is introduced.

## Testing Strategy

Delete tests whose sole purpose is proxy UI, proxy normalization, proxy persistence, or native proxy validation. Update retained behavior tests so that:

- desktop settings do not expose a Network category;
- portable settings omit the network section while retaining all supported keys;
- diagnostics omit proxy fields;
- updater, web-resource, WebDAV, and S3 requests omit proxy data;
- direct/default HTTP client construction preserves current networking behavior;
- cloud sync provider configuration and execution remain covered.

Run the repository verification gates:

- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml`
- `pnpm test`
- `pnpm typecheck:test`
- `pnpm build`
- `pnpm test:s3-sync:live` only when the required real MinIO environment is configured.

Build and open the latest Tauri debug application. Verify that the desktop settings sidebar has no Network category, that WebDAV/S3 sync remains available, and that no user note or sync configuration is modified during inspection.

## Static Removal Gate

Active product code and current documentation must have zero proxy-feature matches for the removed contracts, including:

- `NetworkSettings`
- `defaultNetworkSettings`
- `normalizeNetworkSettings`
- `getStoredNetworkSettings`
- `saveStoredNetworkSettings`
- `proxyEnabled`
- `proxyUrl`
- `bypassLocalAddresses`
- `apply_network_settings`
- `settings.network.*`
- the desktop `network` settings category

Generic networking terminology, provider endpoints, WebDAV/S3 credentials, private-network security checks, and dependency-internal proxy support are outside this symbol gate.

## Acceptance Criteria

1. The desktop settings sidebar contains no Network category or application proxy controls.
2. No supported settings schema, portable settings file, or diagnostic report exposes proxy configuration.
3. TypeScript and Rust command contracts contain no application proxy fields.
4. The native proxy module and proxy-specific tests are deleted.
5. WebDAV sync, S3 sync, updates, and web-image downloads retain their non-proxy behavior.
6. Current documentation and forward-looking plans cannot reintroduce the removed proxy capability.
7. Static removal scans, automated verification gates, and final desktop runtime inspection pass.

