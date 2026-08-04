# Contributing

Thanks for helping improve QingYu. Contributions can be product polish, Markdown editing fixes, file reliability, desktop/mobile/runtime integration fixes, documentation, tests, or issue triage.

## Development Setup

Use `pnpm` for all JavaScript workflows. The repository is a pnpm workspace; the desktop app lives in `apps/desktop`, the Server Web application and browser runtimes live in `apps/web`, the public site lives in `apps/site`, and shared TypeScript packages live in `packages/`.

```bash
pnpm install
pnpm tauri dev
```

Common commands:

- `pnpm dev` starts only the frontend Vite development server.
- `pnpm tauri dev` launches the real desktop app development workflow.
- `pnpm --filter @markra/web dev` starts the Server Web development server.
- `pnpm test` runs package tests.
- `pnpm typecheck:test` runs package type checks for test builds.
- `pnpm build` builds all packages that define a build script.
- `pnpm brand:verify` checks brand and product copy rules.
- `pnpm tauri ...` forwards Tauri commands to the desktop app.

## Project Layout

- `apps/desktop` contains the Tauri desktop shell.
- `apps/web` contains the Server Web application plus browser runtime adapters.
- `apps/site` contains the public site, download surface, and product-facing web pages.
- `packages/app` contains the shared React app surface and product UI.
- `packages/editor` contains Milkdown editor integrations.
- `packages/markdown` contains Markdown parsing and asset/path helpers.
- `packages/scripts` contains shared repository and release scripts.
- `packages/shared` contains cross-cutting types, i18n, and small pure utilities.
- `packages/ui` contains reusable UI primitives.

Keep reusable code inside the package that owns the responsibility. Avoid putting desktop-only bridge code in shared packages.

## Testing

Add focused tests when changing product behavior, editor behavior, file reliability, sync behavior, or platform integration.

Text-only documentation, copy, comment, and static help text changes do not need unit tests. For configuration or packaging changes, use the smallest relevant build or integration check instead of adding tests just for the config file.

Before opening a pull request, run the smallest useful verification for your change. Common checks are:

```bash
pnpm test
pnpm typecheck:test
pnpm build
```

Documentation-only, copy-only, and static text changes do not require unit tests, but still review links, headings, and command snippets before submitting.

## Code Style

- Keep changes small and focused.
- Prefer existing patterns and local helpers over new abstractions.
- Use Tailwind CSS for app styling where practical.
- Prefer `lucide-react` icons for UI controls.
- Do not add a new dependency unless the current stack cannot reasonably handle the job.
- Do not use the TypeScript `void` keyword or operator.
- Keep Markdown files portable and avoid proprietary document formats.

## Pull Requests

Good pull requests usually include:

- A short summary of the user-facing change.
- The files or packages affected.
- The verification command you ran and its result.
- Screenshots or video for visible UI changes.
- Any follow-up work or known limitations.

If your change touches desktop, web, mobile, Docker, or MCP behavior, call out the runtime difference explicitly.

## Releases And Changelog

Release versions are bumped through:

```bash
pnpm release
```

The release bump config updates package versions, syncs desktop metadata, updates the Cargo lockfile, and runs `conventional-changelog` so `CHANGELOG.md` is included in the release changes.

To regenerate the full changelog history from tags:

```bash
pnpm changelog:all
```

Use Conventional Commit subjects such as `feat(app): add quick open` or `fix(editor): preserve selection` so the changelog generator can classify entries correctly.
