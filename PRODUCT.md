# QingYu Product And Brand

## Register

product

## One-Line Positioning

QingYu is a quiet, local-first Markdown notes app for simple, practical recording.

## Product Purpose

QingYu helps people record, edit, and organize Markdown notes without leaving the document surface. On desktop and browser-file surfaces, notes remain ordinary files users can place and open with other tools. On mobile, notes live in a fixed application-managed workspace backed by the same Kernel document model. On Server Web/Docker, one deployment owns one user and one persistent `/data/workspace` managed by Kernel.

The core experience should be immediate: open a desktop file or folder, enter the managed mobile workspace, use browser file handles, or sign in to a self-hosted Server Web instance, then write in a polished document view and save standard Markdown. File management, search, history, sync, and runtime-available export support the note instead of becoming a separate system to maintain.

## Users

- People who want a focused place for everyday notes, drafts, journals, and technical documents.
- Markdown users who value durable plain-text files and freedom from proprietary formats.
- Users who prefer local storage, open-source software, and predictable file behavior.
- People who need folders, tabs, outline navigation, tables, images, search, sync, and runtime-appropriate export without a heavy knowledge-management model.

## Brand Promise

Open a note and start writing. Your Markdown stays portable, your workspace stays under your control, and optional storage features run only when you configure them.

## Brand Personality

- Quiet: the interface steps back and lets the document carry the screen.
- Practical: every visible feature should help users record, find, organize, protect, or share notes.
- Reliable: saving, file operations, history, and restoration should behave predictably.
- Native: QingYu should feel at home on macOS, Windows, Linux, Android, and iOS rather than like one desktop surface stretched everywhere.
- Open: the product should feel transparent, auditable, and free of lock-in.

## Experience Principles

### Recording First

The note is the primary object. Toolbars, file trees, settings, search, and status indicators should support recording without competing for attention.

### Local And Portable

Markdown files remain ordinary files on disk where the runtime exposes the filesystem directly. In managed runtimes, the workspace is still Markdown-backed and should keep image paths, save state, history, and file operations predictable and recoverable.

### Simple By Default

A new desktop or browser-file user should be able to open a file and write without configuration. A new mobile user should be able to enter the managed workspace and write without choosing a filesystem root. A new Server Web owner should be able to initialize the instance, sign in, and land in the fixed workspace without learning an extra project system. Advanced editing, sync, and runtime-specific export should remain available without crowding the default view.

### Native Where It Matters

Window chrome, menus, shortcuts, drag regions, file dialogs, mobile Back, keyboard avoidance, lifecycle persistence, web authentication, and platform-specific layout should respect the host runtime. Platform polish is part of the product, not decoration.

### Quiet Capability

Useful features can be deep, but their surfaces should remain composed. Prefer progressive reveal, compact controls, and familiar affordances over promotional panels or instructional clutter.

## Visual Direction

- Use a restrained product interface: tinted neutrals, soft borders, clear focus states, and one accent for active or primary state.
- Prefer density with air: compact controls are useful, while the writing surface needs room for long documents.
- Keep panels and chrome light in visual weight so the note remains the visual anchor.
- Use system-feeling typography and iconography. Lucide icons are appropriate for controls, but labels and layout should still feel native.
- Let platform differences be explicit when they improve trust: macOS can reserve traffic-light space, Windows should use native title and menu behavior, Linux should remain simple and predictable, and mobile should favor full-screen compact flows over desktop panels.

## Voice And Copy

- Direct, calm, and specific.
- Prefer short verbs: open, write, save, find, move, sync, export.
- Avoid hype, mascot language, vague productivity promises, and inflated claims.
- Do not over-explain visible controls inside the app. Tooltips and accessible labels are enough for standard commands.
- Error copy should name the failed action and the next useful step when one exists.

## Anti-References

QingYu should not feel like:

- A marketing dashboard with oversized cards, decorative gradients, and feature copy everywhere.
- A heavy IDE where file management and panels dominate the page.
- A proprietary note app that hides files behind an opaque account or workspace model.
- A complicated personal knowledge system that requires ongoing maintenance before it becomes useful.
- A document suite that replaces portable Markdown with an opaque format.

## Product Boundaries

- Do not require an account, hosted workspace, or remote service for local desktop, mobile, or browser-file note work.
- Do not describe Server Web as a browser file editor. It is a single-user notes service with authenticated Kernel HTTP/WebSocket transport and a persistent `/data` volume.
- Do not bury Markdown portability behind custom document formats.
- Do not make destructive file or document changes without confirmation.
- Do not turn ordinary note recording into a setup-heavy organizational methodology.
- Do not expose desktop-only concepts such as arbitrary folder selection, MCP, Pandoc, updater controls, or independent settings windows on mobile unless the mobile runtime actually owns them.
- Do not expose browser-file concepts such as local file handles or IndexedDB authority inside Server Web unless that runtime actually owns them.
- Do not introduce visual systems that only work on one platform family.

## Success Signals

- A new desktop or browser-file user can open a Markdown file and start writing without setup; a new mobile user can enter the managed workspace and start writing without setup; a new Server Web owner can initialize, sign in, and use the fixed workspace.
- A Markdown user recognizes the document as normal Markdown, not a locked editor format.
- Files, folders, search, history, sync, and runtime-available export behave predictably.
- The app feels calm during everyday note work.
- Platform-specific details feel intentional rather than patched around.
