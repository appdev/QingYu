# QingYu single-user Docker packaging

This directory contains the first runnable Kernel image for the confirmed deployment model: one Docker deployment owns one user and one persistent `/data` volume. It is not yet a complete browser deployment.

## Current boundary

The image now has a real server process boundary:

- a Node + pnpm build stage produces `apps/web/dist`;
- a Rust build stage produces the locked release `qingyu-kernel` binary;
- the final image runs `qingyu-kernel server --public-origin <exact HTTPS origin>` as UID/GID `10001:10001`;
- the final image carries the Web build at `/opt/qingyu/web`.

The Kernel currently exposes its authenticated JSON/WebSocket API and health routes on `0.0.0.0:3210`, but it does **not** serve `/opt/qingyu/web` or `apps/web/dist`. No Node or Vite server is added to hide that missing product boundary. Consequently, opening port 3210 in a browser does not deliver the QingYu Web application.

`compose.contract.yaml` therefore keeps the service behind the `static-web-serving-required` profile, and `verify-runtime.sh --status` exits 78 with that blocker. The profile is an integration fixture, not a production deployment recommendation.

Run the dependency-light packaging check with:

```sh
deploy/docker/verify-contract.sh
```

A successful contract check means the image and Compose files match this documented packaging boundary. It does not mean the browser server is complete. Docker availability and image construction are separate evidence.

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

The public origin is the browser-visible HTTPS authority, not the container's internal HTTP address. A future supported deployment must terminate TLS at a reverse proxy, serve or route the Web application and Kernel API under that same origin, and proxy Kernel traffic to `127.0.0.1:3210` without rewriting the browser-visible authority.

For example, a site reached as `https://notes.example.com` must launch the Kernel with exactly:

```sh
QINGYU_PUBLIC_ORIGIN=https://notes.example.com
```

TLS certificates and reverse-proxy configuration are intentionally outside this image. The current Compose contract neither provisions TLS nor serves the Web asset directory.

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
| Web assets served to the browser | **Blocked: `static-web-serving-required`** | A reviewed same-origin static/fallback routing owner serves the app without a second development server. |
| TLS ingress | Not included | Reverse-proxy tests prove HTTPS origin, headers, cookies, CSRF, and WebSocket upgrades. |

The runtime verifier must remain blocked until the static Web owner exists and the complete browser/container matrix is executable. Docker being unavailable is an environmental limitation, not a passing runtime result.
