# QingYu Repository Links And Default README Design

## Goal

Make the QingYu downstream repository at `https://github.com/appdev/QingYu` the consistent project identity across the tracked repository, and make Simplified Chinese the default README language shown by GitHub.

## Scope

The implementation will:

- move the current English `README.md` content to `README.en.md`;
- move the current Simplified Chinese `README.zh-CN.md` content to `README.md`;
- update the language switcher in both README files;
- replace tracked references to former project repositories such as `github.com/markrahq/markra` and `github.com/murongg/markra` with the matching `github.com/appdev/QingYu` URL;
- update user-facing links, runtime link constants, repository metadata, release and updater fixtures, tests, documentation, and generated changelog links when they identify the project repository;
- update tests and fixtures that assert the old project repository URL.

## Deliberate Exclusions

The implementation will not redirect unrelated or functional third-party URLs to the repository. This includes badge image providers, contributor and star-history rendering services, dependency and license websites, and the deployed Web editor URL.

The read-only `upstream` Git remote remains `https://github.com/markrahq/markra.git`. Repository policy may continue to name that URL when documenting where upstream changes are fetched, because it describes an external source remote rather than QingYu's public project identity. The `origin` remote remains `https://github.com/appdev/QingYu.git`.

Local paths, package scopes, bundle identifiers, executable names, protocol names, and storage keys containing `markra` are not URLs and are outside this change.

## README Layout

`README.md` becomes the Simplified Chinese landing page. Its language selector labels Simplified Chinese as current and links English to `README.en.md`.

`README.en.md` retains the English content. Its language selector labels English as current and links Simplified Chinese to `README.md`.

Repository downloads, releases, issues, contributors, documentation, license, and star-history links use `appdev/QingYu` where their URL supports a repository parameter or target.

## Repository Link Migration

Every tracked text reference to an old project repository URL will be classified before replacement:

1. Project identity, source, issue, release, compare, commit, or file links move to the equivalent path under `appdev/QingYu`.
2. Test expectations and release fixtures move with the production URL they verify.
3. Historical changelog links keep their path, issue number, commit hash, tag, or comparison range while changing only the repository owner and name.
4. Explicit upstream-integration policy links remain on `markrahq/markra` when they document the actual read-only remote.
5. Third-party functional URLs remain unchanged unless only their repository parameter needs to become `appdev/QingYu`.

## Verification

Verification will include:

- `git diff --check`;
- a repository-wide search confirming no obsolete project-identity links remain outside the documented upstream-policy exception;
- a search confirming `README.md` is Chinese and both language selectors resolve to existing files;
- focused tests for source modules whose URL expectations change;
- the smallest relevant repository test command for any changed runtime or release logic;
- a final diff and status review confirming unrelated files, including `macos-icon.icns`, were not included.

## Success Criteria

- GitHub renders the Simplified Chinese README by default.
- English remains available through `README.en.md`.
- QingYu-owned project links resolve under `https://github.com/appdev/QingYu`.
- Functional third-party services and the Web editor are not broken by indiscriminate URL replacement.
- The read-only upstream/downstream remote policy stays factually correct.
- No unrelated working-tree changes are modified or committed.
