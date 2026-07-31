# QingYu single-user Docker packaging

This directory contains the runnable Kernel image and precompiled runtime-bundle contract for the confirmed deployment model: one Docker deployment owns one user and one persistent `/data` volume. The browser application now uses the server `KernelClient`; final live Linux acceptance remains pending until the target-host matrix is captured.

## Current boundary

The local/CI source-build image has a real server process boundary:

- a Node + pnpm build stage produces `apps/web/dist`;
- a Rust build stage produces the locked release `qingyu-kernel` binary;
- the final image runs `qingyu-kernel server --public-origin <exact HTTP or HTTPS origin>` as UID/GID `10001:10001`;
- the final image carries the Web build at `/opt/qingyu/web`.

The Kernel exposes its authenticated JSON/WebSocket API and health routes on `0.0.0.0:3210`. The same process serves `/opt/qingyu/web`, including real assets and the SPA fallback, so no Node or Vite server is present in the runtime image. Unknown `/api` routes stay JSON and never fall through to the Web entrypoint.

`compose.contract.yaml` is the local/CI source-build fixture behind the `local-source-build` profile. It intentionally retains `build:` and the root Dockerfile. It is not the test-server deployment path. `verify-runtime.sh --status` now reports `READY(runtime-ready)` together with `PENDING(final-live-linux-acceptance)`; readiness describes the packaging contract, not a completed Docker/Linux run.

`runtime-bundle.compose.yaml` is the release template. `package-runtime-bundle.sh` packages it as `compose.yaml` beside the precompiled Kernel, passive Web distribution, runtime-only Dockerfile, scripts, metadata, and `SHA256SUMS`. The packaged Compose file has no `build:` section, source path, toolchain, default image, public-origin literal, or initialization-token literal. The recipient must supply an explicit prebuilt image reference through `QINGYU_SERVER_IMAGE`.

Run the packaging check with Ruby/Psych (the YAML parser bundled with Ruby):

```sh
deploy/docker/verify-contract.sh
```

The verifier parses Compose as YAML, freezes the only Docker parser directive and every stage instruction, and freezes the complete `.dockerignore` policy. Its tracked-input fallback manifest must exactly match Git whenever repository metadata is available, so archive verification remains reproducible without allowing a tracked `COPY` descendant to disappear from the context. The Web build stage ends by running `verify-web-dist.mjs`; no later instruction may modify `apps/web/dist` before the final image copies that verified layer. The artifact verifier streams directory entries and rejects symlinks, hard links, non-regular files, executable mode bits, executable binary magic, unsupported extensions, excessive depth, more than 10,000 total nodes, more than 16 MiB of logical-path metadata, files larger than 16 MiB, and distributions larger than 128 MiB. Retained files must keep one stable device, inode, link count, size, mode, modification time, and change time before, during, and after inspection.

The runtime stage executes the independent `verify-final-web-assets.sh` scanner after copying `/opt/qingyu/web`, and the built-image probe executes the same scanner again. This keeps hard-link, node/path, depth, extension, mode, magic, and size limits active on the actual final filesystem instead of trusting only the earlier build-stage path. Run the adversarial contract and artifact mutation suite with:

```sh
deploy/docker/test-verify-contract-mutations.sh
```

If a usable Docker daemon is present, `verify-contract.sh` additionally builds the local/CI final stage and inspects its configured user, entrypoint, complete Web asset tree, Kernel artifact, and absence of Node toolchain executables. Without a usable daemon it reports final-image evidence as pending. This check does not replace final live Linux acceptance.

## Fixed runtime contract

- `/data` is the only persistent mount. The Kernel owns `/data/workspace`, `/data/config`, `/data/state`, and `/data/logs`; no command or environment variable can relocate them.
- `/tmp/qingyu` is the only disposable cache path. Compose supplies it as a UID/GID `10001:10001` tmpfs.
- The final image and Compose service run as numeric UID/GID `10001:10001`, drop all capabilities, enable `no-new-privileges`, and use a read-only root filesystem. Compose allows 35 seconds before forced termination so the Kernel's 30-second drain deadline can finish and report its outcome.
- Only container port 3210 is exposed. Compose binds it to `127.0.0.1:3210` by default for a same-host TLS reverse proxy. Set `QINGYU_PUBLISHED_ADDRESS` explicitly when direct HTTP must listen on another host interface; this value controls Compose port publishing and is not passed into the container.
- `QINGYU_PUBLIC_ORIGIN` is required at process launch. It is not secret, but it must be the exact canonical browser-visible HTTP or HTTPS origin accepted by the Kernel, for example `http://192.168.0.172:3210`, `https://notes.example.com`, or `https://notes.example.com:8443`. Do not include a trailing slash, path, query, user information, or an explicit default `:80`/`:443` port.
- `QINGYU_SERVER_INITIALIZATION_TOKEN` is optional after initialization and is passed only through the container environment. It never enters a build argument, image environment value, Compose literal, or Compose default.
- Both container runtime inputs use Compose's value-free environment pass-through. The entrypoint fails closed if `QINGYU_PUBLIC_ORIGIN` is absent or empty; the Kernel validates its canonical HTTP/HTTPS form and exact authority.

The source-build image can be built independently for local/CI verification when Docker is available:

```sh
docker build --target qingyu-runtime -t qingyu-server:local .
```

That command proves image construction only. Do not run it on a runtime-only test server and do not treat it as target-host acceptance.

For a runtime-only release, first build the matching Linux Kernel binary and Web distribution on the trusted build host, then package them:

```sh
deploy/docker/package-runtime-bundle.sh \
  --architecture amd64 \
  --kernel /path/to/prebuilt/qingyu-kernel \
  --web-dist /path/to/apps-web-dist \
  --output /path/to/qingyu-runtime-linux-amd64.tar.gz
deploy/docker/verify-runtime-bundle.sh \
  --archive /path/to/qingyu-runtime-linux-amd64.tar.gz \
  --architecture amd64
```

Record and verify the generated archive sidecar before upload. Upload only the exact archive and checksum; do not upload repository source, Git credentials, or build toolchains. After extraction, build or load the runtime image from the bundle Dockerfile on a trusted image builder, publish or transfer that prebuilt image, set `QINGYU_SERVER_IMAGE` to its explicit reference, and run the bundled `compose.yaml` on the runtime host.

## HTTP direct access and HTTPS reverse proxy

Two ingress modes are supported. In both modes the browser, Web application, HTTP API, CSRF checks, and WebSocket endpoint use one exact origin. The Kernel rejects mismatched `Host` or `Origin` headers instead of trusting forwarded-host metadata.

For direct HTTP on a trusted network, publish port 3210 on the intended interface and make the public origin match the address typed into the browser:

```sh
export QINGYU_PUBLISHED_ADDRESS=0.0.0.0
export QINGYU_PUBLIC_ORIGIN=http://192.168.0.172:3210
```

HTTP is not encrypted. The owner password, one-time initialization token, document contents, session cookies, and S3/WebDAV credentials are transmitted in plaintext and can be read or modified by an on-path observer. Same-origin, CSRF, `HttpOnly`, and `SameSite=Strict` protections do not provide transport confidentiality. Use direct HTTP only on a network whose interception risk you explicitly accept; prefer HTTPS for remote or untrusted networks.

HTTP uses `qingyu_session` and `qingyu_csrf` cookies without `Secure`, because browsers cannot send `Secure` cookies over HTTP. HTTPS uses separate `__Host-qingyu_session` and `__Host-qingyu_csrf` cookies with `Secure`. The server and Web client select only the names for the configured scheme, so credentials are not reused across HTTP and HTTPS profiles.

In HTTPS mode, the public origin is the browser-visible HTTPS authority, not the container's internal HTTP address. The reverse proxy must terminate TLS, route the Web application and Kernel API under that same origin, and proxy traffic to `127.0.0.1:3210` without rewriting the browser-visible authority.

For example, a site reached as `https://notes.example.com` must launch the Kernel with exactly:

```sh
QINGYU_PUBLIC_ORIGIN=https://notes.example.com
```

TLS certificates and reverse-proxy configuration are intentionally outside this image. The Compose contract does not provision TLS; its default loopback bind serves the Web assets and Kernel API together on port 3210 for a same-origin reverse proxy.

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
| Kernel server CLI and public origin | Implemented | Exact canonical HTTP/HTTPS origin starts; missing/non-canonical origin fails closed. |
| First initialization and token-free restart | Kernel capability implemented; container matrix not yet executed here | Empty-volume init, restart, persistence, and secret-leak checks pass in a real container. |
| Kernel live/readiness routes | Implemented by Kernel; image healthcheck intentionally disabled | Real container probes cover starting, uninitialized, ready, and failure states. |
| Web static assets in final image | Implemented | Asset inventory matches the `apps/web` build. |
| Web assets served to the browser | Implemented | Same-origin GET/HEAD, real assets, SPA fallback, API exclusion, exact Host/Origin, CSP, and no-follow checks pass. |
| Web application uses KernelClient only | Runtime contract ready | Browser/runtime tests cover the server bootstrap and ensure Docker mode has no local-directory workspace owner. |
| HTTP direct ingress | Implemented contract | Browser tests prove exact HTTP origin, scheme-specific cookies, CSRF, and WS; live Linux acceptance remains required. |
| TLS ingress | Not included | Reverse-proxy tests prove HTTPS origin, headers, cookies, CSRF, WSS upgrades, and isolation from HTTP cookies. |

The runtime packaging gate is ready. Final live Linux acceptance is still pending until the prebuilt-image/container matrix proves fixed `/data`, initialization, restart persistence, direct HTTP/WS, HTTPS/WSS proxying, HTTP/HTTPS cookie isolation, SIGTERM with a 30-second drain budget, and Linux runtime behavior. Docker being unavailable is an environmental limitation, not passing evidence.
