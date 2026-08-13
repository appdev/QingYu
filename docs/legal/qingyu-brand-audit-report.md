# QingYu Brand and Upstream-Name Audit Report

Audit date: 2026-08-13

## Result

The executable QingYu brand audit reports zero prohibited product-surface uses. The distributed application no longer presents SiYuan as the current product, support channel, website, download source, application update source, privacy-policy provider, user-agreement provider, or product Logo.

The audit covers tracked and untracked non-ignored source files that are relevant to product surfaces. It rejects upstream application update services and package names, upstream support/service links in README, legal, Electron, language, and guide surfaces, SiYuan product-title markup, and legacy upstream Logo consumers. Fixture tests prove that broad directory allowlists are not accepted.

## Removed or replaced risk surfaces

- SiYuan version lookup, release URLs, application-package download, checksum, installer launch, and Electron multi-workspace update coordination.
- Check-update and automatic application-package controls in the About settings.
- B3log, LD246, liuyun.io, and SiYuan release/download/support links from Electron windows, menus, language resources, API examples, and built-in guides.
- The SiYuan Logo symbol and every product or function consumer of that symbol.
- Four packaged SiYuan guide notebooks, replaced under the same mounted box/root IDs by deterministic QingYu guides.
- Upstream privacy and agreement references, replaced by QingYu policies signed by the individual GitHub developer `appdev`.

## Allowed remaining upstream-name categories

| Category | Representative paths | Reason and boundary |
| --- | --- | --- |
| Go module and imports | `kernel/go.mod`, `kernel/**/*.go` | Public module identity and upstream dependency paths; changing them would break compilation and compatibility. |
| Runtime/API symbols | `app/src/**`, including `window.siyuan`, `exitSiYuan`, URI helpers, types, IPC and serialized fields | Existing plugin, host, clipboard, configuration, and public API compatibility. The values shown to users use QingYu branding. |
| Legacy link and User-Agent reads | `app/src/util/pathName.ts`, `app/src/util/functions.ts` | Read-only compatibility for existing `siyuan://`, `web+siyuan://`, and old native containers. Newly generated links use `qingyu://`. |
| Language key names | `app/appearance/langs/*.json` such as `whatsNewInSiYuan` | Stable internal key compatibility; translated values identify the product as QingYu/轻语/輕語. |
| Engineering trace links and comments | `app/src/**`, `kernel/**`, `docs/**` | Historical issue, commit, implementation, and imported-upstream evidence; not exposed as QingYu support or service endpoints. |
| Format and guide attribution | `app/guide-src/*.json`, generated `app/guide/**/*.sy` | Necessary factual attribution paired in the same guide with a clear non-official, no-affiliation, no-authorization, and no-endorsement statement. |
| Copyright, license, and modified-version attribution | `LICENSE`, `NOTICE.md`, README files, `docs/legal/terms.*.md` | Required preservation of upstream rights and AGPL attribution. |
| Historical changelogs | `app/changelogs/**` | Generated historical upstream release records; project rules prohibit hand-editing. They are not used as QingYu update or support endpoints. |
| Legacy content parsing | `kernel/api/extension.go`, `kernel/model/file.go` | Reads existing LD246/liuyun article links embedded in user content; does not advertise those sites as QingYu services. |

## Product URLs approved by this audit

- Website: https://apkdv.com/
- Source: https://github.com/appdev/QingYu
- Developer identity: https://github.com/appdev
- Contact: mailto:lengyue@apkdv.com

## Verification evidence

- `cd app && pnpm test`: 209 tests passed.
- `cd app && pnpm run lint`: TypeScript typecheck and ESLint passed.
- `cd kernel && go test ./model ./api ./conf ./util -count=1`: all four packages passed.
- `python3 scripts/check-lang-keys.py`: 21 language files complete; 1,922 expected keys; zero unexpected keys.
- Guide, legal-page, and PNG-icon generators pass `--check` without modifying the tree.
- Brand-audit fixture tests pass and the real repository audit reports zero violations.

## Residual risk and external review

- ICNS and ICO assets were regenerated from the root `logo.png` with the available macOS tools; the cross-platform Node generator verifies all shipped PNG sizes, while byte-for-byte ICNS/ICO reproduction is platform-tool dependent.
- Windows/Linux installer UI, mobile repositories, published websites, release artifacts, app stores, and remote services were not built, deployed, or inspected in this local task.
- This engineering audit is not a trademark clearance search or legal opinion. Before commercial public release, conduct formal searches for “轻语”, “QingYu”, the Logo, and related identifiers in each target market, and have a qualified lawyer review the policies and California-law agreement.
