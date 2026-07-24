# QingYu Copy Rebrand Design

## Goal

Replace every user-visible legacy brand reference with the approved localized product name while preserving technical identifiers and compatibility-sensitive paths.

## Approved Naming Matrix

| Context | Product name | AI feature name |
| --- | --- | --- |
| Simplified Chinese (`zh-CN`, `zh-Hans`) | 轻语 | 轻语 AI |
| Traditional Chinese (`zh-TW`, `zh-Hant`) | 輕語 | 輕語 AI |
| Every other language and default metadata | QingYu | QingYu AI |

The capitalization `QingYu` is canonical for every non-Chinese user-visible surface.

## User-Visible Scope

The replacement includes:

- all eleven frontend locale files, including settings, update states, AI labels, errors, accessibility labels, and menu copy;
- native Rust menu labels, application-menu titles, About metadata, diagnostics that users can see, and related expectations;
- Tauri `productName`, default HTML titles, web runtime titles, and release artifact display names;
- macOS localized `CFBundleDisplayName` and `CFBundleName`, using 轻语 for Simplified Chinese, 輕語 for Traditional Chinese, and QingYu elsewhere;
- README files, product and design documents, privacy and contribution documents, issue templates, user-facing release workflow text, and all historical changelog prose;
- tests and fixtures that assert user-visible product names.

The default packaged product and artifact name is `QingYu`. Chinese macOS display names and language-aware in-app/native menu copy use their localized names. Static pre-bootstrap HTML titles remain `QingYu` until document-specific titles replace them.

## Protected Technical Identifiers

The following remain unchanged:

- pnpm package names and imports under `@markra/*`;
- the workspace package name, Rust crate/module identifiers, and internal function/type names;
- Tauri identifier `dev.markra.app`;
- event names, menu IDs, window labels, storage keys, logging targets, and protocol names containing `markra`;
- `.markraignore`, `.markra-sync`, existing settings/data locations, and synchronization metadata names;
- the `markra` terminal command and its installer paths;
- GitHub organization/repository URLs, website URLs, updater endpoints, Product Hunt identifiers, Homebrew tap/cask slug, and `APP_SLUG`;
- historical Git links, commit links, issue links, and filenames whose lowercase identifier is part of an external or compatibility contract.

These exclusions prevent the copy rebrand from changing update continuity, data discovery, shell compatibility, package resolution, or external links.

## Implementation Approach

Use a controlled replacement rather than a repository-wide substitution:

1. Replace brand words in locale values according to the naming matrix without renaming translation keys.
2. Add a localized application name to the native menu-label model so the app menu and About surface follow the active language while internal menu IDs remain stable.
3. Update default package/display metadata and release product-name fallbacks to `QingYu`, retaining lowercase slugs and identifiers.
4. Update documentation prose by language, including historical changelog prose, while leaving commands and URLs unchanged.
5. Audit every residual legacy-name occurrence and retain only occurrences proven to be technical identifiers.

No broad refactor or package rename is included.

## Verification

- Add a brand-copy verifier that scans tracked user-facing text for the standalone legacy product name and reports exact file/line failures. The verifier must not reject lowercase technical identifiers.
- Add focused tests for the naming matrix, native menu application names/About metadata, web title defaults, release product-name fallbacks, and verifier failure behavior.
- Run the existing i18n completeness tests and update affected user-facing expectations.
- Run native Rust menu tests, release-script tests, the full pnpm test suite, and the production build.
- Build or inspect desktop package metadata to confirm the default product name is `QingYu` and macOS localized bundle-name resources contain 轻语/輕語/QingYu as designed.
- Finish with a residual `rg` audit so every remaining lowercase `markra` occurrence is attributable to an approved technical category.

## Success Criteria

- A Simplified Chinese user sees 轻语 wherever the product name is shown.
- A Traditional Chinese user sees 輕語 wherever localized product copy is available.
- Every other language sees QingYu.
- No user-visible standalone legacy product name remains, including historical prose.
- Existing installations retain their identifier, settings, sync metadata, command, update URLs, and external links.
