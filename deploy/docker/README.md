# QingYu single-user Docker packaging

This directory contains the first runnable Kernel image for the confirmed deployment model: one Docker deployment owns one user and one persistent `/data` volume. Static same-origin Web delivery is implemented, but the browser bundle is not yet wired exclusively to `KernelClient`.

## Current boundary

The image now has a real server process boundary:

- a Node + pnpm build stage produces `apps/web/dist`;
- a Rust build stage produces the locked release `qingyu-kernel` binary;
- the final image runs `qingyu-kernel server --public-origin <exact HTTPS origin>` as UID/GID `10001:10001`;
- the final image carries the Web build at `/opt/qingyu/web`.

The Kernel exposes its authenticated JSON/WebSocket API and health routes on `0.0.0.0:3210`. The same process serves `/opt/qingyu/web`, including real assets and the SPA fallback, so no Node or Vite server is present in the runtime image. Unknown `/api` routes stay JSON and never fall through to the Web entrypoint.

`compose.contract.yaml` keeps the service behind the next `web-kernel-runtime-required` profile, and `verify-runtime.sh --status` exits 78 with that blocker. The served bundle still contains the legacy browser-local runtime until the Web entrypoint is switched to the server `KernelClient`; the profile remains an integration fixture rather than a production deployment recommendation.

Run the packaging check with Ruby/Psych (the YAML parser bundled with Ruby):

```sh
deploy/docker/verify-contract.sh
```

The verifier parses Compose as YAML, freezes the only Docker parser directive and every stage instruction, and freezes the complete `.dockerignore` policy. Its tracked-input fallback manifest must exactly match Git whenever repository metadata is available, so archive verification remains reproducible without allowing a tracked `COPY` descendant to disappear from the context. The Web build stage ends by running `verify-web-dist.mjs`; no later instruction may modify `apps/web/dist` before the final image copies that verified layer. The artifact verifier streams directory entries and rejects symlinks, hard links, non-regular files, executable mode bits, executable binary magic, unsupported extensions, excessive depth, more than 10,000 total nodes, more than 16 MiB of logical-path metadata, files larger than 16 MiB, and distributions larger than 128 MiB. Retained files must keep one stable device, inode, link count, size, mode, modification time, and change time before, during, and after inspection.

The runtime stage executes the independent `verify-final-web-assets.sh` scanner after copying `/opt/qingyu/web`, and the built-image probe executes the same scanner again. This keeps hard-link, node/path, depth, extension, mode, magic, and size limits active on the actual final filesystem instead of trusting only the earlier build-stage path. Run the adversarial contract and artifact mutation suite with:

```sh
deploy/docker/test-verify-contract-mutations.sh
```

If a usable Docker daemon is present, `verify-contract.sh` additionally builds the final stage and inspects its configured user, entrypoint, complete Web asset tree, Kernel artifact, and absence of Node toolchain executables. Without a usable daemon it reports final-image evidence as pending. Neither result proves the remaining Web `KernelClient` migration.

## Fixed runtime contract

- `/data` is the only persistent mount. The Kernel owns `/data/workspace`, `/data/config`, `/data/state`, and `/data/logs`; no command or environment variable can relocate them.
- `/tmp/qingyu` is the only disposable cache path. Compose supplies it as a UID/GID `10001:10001` tmpfs.
- The final image and Compose service run as numeric UID/GID `10001:10001`, drop all capabilities, enable `no-new-privileges`, and use a read-only root filesystem.
- Only container port 3210 is exposed. Compose binds it to `127.0.0.1:3210`, so a same-host TLS reverse proxy is the intended external ingress.
- `QINGYU_PUBLIC_ORIGIN` is required at process launch. It is not secret, but it must be the exact canonical HTTPS origin accepted by the Kernel, for example `https://notes.example.com` or `https://notes.example.com:8443`. Do not include a trailing slash, path, query, user information, or an explicit default `:443` port.
- `QINGYU_SERVER_INITIALIZATION_TOKEN` is optional after initialization and is passed only through the container environment. It never enters a build argument, image environment value, Compose literal, or Compose default.
- Both runtime inputs use Compose's value-free environment pass-through. The entrypoint fails closed if `QINGYU_PUBLIC_ORIGIN` is absent or empty; the Kernel validates its exact HTTPS form.

The final image can be built independently when Docker is available:

```sh
docker build --target qingyu-runtime -t qingyu-server:local .
```

That command proves image construction only. Until a reviewed static-Web owner is integrated, do not publish or describe the image as a browser-ready QingYu server.

## TLS reverse proxy and public origin

The public origin is the browser-visible HTTPS authority, not the container's internal HTTP address. A supported deployment must terminate TLS at a reverse proxy, route the Web application and Kernel API under that same origin, and proxy traffic to `127.0.0.1:3210` without rewriting the browser-visible authority.

For example, a site reached as `https://notes.example.com` must launch the Kernel with exactly:

```sh
QINGYU_PUBLIC_ORIGIN=https://notes.example.com
```

TLS certificates and reverse-proxy configuration are intentionally outside this image. The current Compose contract does not provision TLS; it serves the Web assets and Kernel API together on internal port 3210 for a same-origin reverse proxy.

## One-time initialization token

For a new, empty `qingyu-data` volume, provide a cryptographically random `QINGYU_SERVER_INITIALIZATION_TOKEN` containing at least 32 bytes. The browser initialization request proves possession of that token and sets the single owner's password.

After successful initialization:

1. remove the token from the deployment environment or secret manager;
2. recreate the container with the same `/data` volume and without the token;
3. keep only `QINGYU_PUBLIC_ORIGIN` as the required launch input.

The durable owner state in `/data/config` is authoritative, so an initialized volume restarts without the token. If the process stops before initialization commits, retain the same `/data` volume and re-supply the original token to retry. If the outcome is uncertain, do not replace `/data` or silently choose a new token; inspect readiness and owner state first. Supplying a token to an already initialized volume does not reinitialize the owner, but clearing it still minimizes secret lifetime.

Never place the token in the Dockerfile, image labels, Compose YAML, shell history, logs, or files under `/data`.

## Acceptance matrix

| Boundary | Current status | Evidence required before browser release |
| --- | --- | --- |
| Locked Kernel release build | Implemented in the image build stage | `docker build --target qingyu-runtime ...` exits 0. |
| Web production build | Implemented in the image build stage | `/opt/qingyu/web/index.html` exists in the final image. |
| Fixed paths and non-root process | Implemented in image/Compose contract | UID/GID, read-only root, `/data`, and `/tmp/qingyu` probes pass. |
| Kernel server CLI and public origin | Implemented | Exact HTTPS origin starts; missing/non-canonical origin fails closed. |
| First initialization and token-free restart | Kernel capability implemented; container matrix not yet executed here | Empty-volume init, restart, persistence, and secret-leak checks pass in a real container. |
| Kernel live/readiness routes | Implemented by Kernel; image healthcheck intentionally disabled | Real container probes cover starting, uninitialized, ready, and failure states. |
| Web static assets in final image | Implemented | Asset inventory matches the `apps/web` build. |
| Web assets served to the browser | Implemented | Same-origin GET/HEAD, real assets, SPA fallback, API exclusion, exact Host/Origin, CSP, and no-follow checks pass. |
| Web application uses KernelClient only | **Blocked: `web-kernel-runtime-required`** | The browser entrypoint must use the server bootstrap/runtime and must not expose a local-directory picker or IndexedDB workspace owner. |
| TLS ingress | Not included | Reverse-proxy tests prove HTTPS origin, headers, cookies, CSRF, and WebSocket upgrades. |

The runtime verifier remains blocked until the Web `KernelClient` cutover and complete browser/container matrix are executable. Docker being unavailable is an environmental limitation, not a passing runtime result.
