# QingYu single-user Docker contract

This directory records the first packaging contract for the confirmed deployment model: one Docker deployment serves one user through a browser, while one persistent mount at `/data` contains both the local working copy and all server configuration.

## Current phase status

The repository does **not** yet contain a runnable Docker server entrypoint.

The existing `qingyu-kernel serve` binary is the native-host child process used by desktop and mobile shells. It reads a `NativeHostStart` frame from stdin, activates desktop paths, binds an ephemeral `127.0.0.1` port, and treats stdin closure as a lease. The native host also deliberately rejects the `server` profile. Although the fixed server path layout, launch-token contract, and live/ready routes are defined, the remaining blocker is the server HTTP entrypoint and composition that binds them into an externally reachable process.

For that reason:

- the root `Dockerfile` ends in a `scratch` artifact stage with no command, entrypoint, exposed port, or runtime claim;
- `compose.contract.yaml` requires a future server runtime image and hides the service behind the `server-entrypoint-required` profile;
- the Compose healthcheck is disabled. Labels record the required live and ready routes without assuming that a future runtime image contains `curl`, `wget`, or another probe binary;
- `verify-runtime.sh --status` exits with status 78 until the server HTTP entrypoint and composition exist.

Run the dependency-free static contract check with:

```sh
deploy/docker/verify-contract.sh
```

The artifact image can be built when Docker is available, but it is not runnable:

```sh
docker build --target kernel-artifact -t qingyu-kernel-artifact .
```

Do not use a dummy HTTP server or assign a guessed command to make the Compose file appear operational.

## Fixed deployment contract

- The container-side data root is always `/data`; there is no product argument or environment variable that can change it.
- `/data/workspace`, `/data/config`, `/data/state`, and `/data/logs` share the single persistent `qingyu-data` volume. `/tmp/qingyu` remains disposable runtime cache.
- The service runs as numeric UID/GID `10001:10001`, drops all Linux capabilities, enables `no-new-privileges`, and uses a read-only root filesystem. Only `/data` and the disposable `/tmp` tmpfs are writable.
- Container port `3210` is the only published port. The host-side port may be overridden with `QINGYU_SERVER_PORT`.
- `QINGYU_SERVER_INITIALIZATION_TOKEN` uses value-free Compose environment pass-through. It has no image build argument, image environment value, Compose literal, or Compose default.
- `QINGYU_SERVER_INITIALIZATION_TOKEN` must contain at least 32 bytes. It is required only when initializing an empty `/data` volume, is consumed by the one-time initialization owner, and must never be persisted or logged in plaintext. An initialized volume must restart without the variable.
- The restart policy is `unless-stopped`.

The contract file is deliberately not a production Compose file yet. Supplying `QINGYU_SERVER_IMAGE` and enabling its profile does not turn the current native child into a supported server.

## Runtime acceptance matrix

These cases become mandatory when the server workstream delivers the HTTP entrypoint and composition. Until then they are phase-gated, not skipped as passing tests.

| Case | Setup | Required result |
| --- | --- | --- |
| First initialization | Empty `qingyu-data`; `QINGYU_SERVER_INITIALIZATION_TOKEN` containing at least 32 bytes | Server initializes `/data`, becomes ready, and does not expose the token in image metadata, logs, or files. |
| Missing first token | Empty/new `qingyu-data`; token absent | Startup fails closed and readiness never succeeds. |
| Persistent restart | Initialize, create representative workspace/config/state data, replace the container, reuse the same volume, remove the token | Server becomes ready and all representative data remains unchanged. |
| One-time token replay | Initialized volume; provide the old token again | Server does not reinitialize, rotate ownership, or overwrite existing data. The final server policy must explicitly reject or safely ignore the replay. |
| Unwritable data | Mount `/data` read-only or with an incompatible owner | Startup fails before readiness with an actionable error. |
| Liveness | Running initialized server | `/api/v1/health/live` reports process liveness on container port `3210`. |
| Readiness | Starting, uninitialized, locked, and ready states | `/api/v1/health/ready` succeeds only after initialization, path activation, and exclusive workspace ownership complete. |
| Port surface | Inspect the created container | Only container port `3210` is published. |
| Non-root runtime | Inspect the created container and write probes | Effective UID/GID is `10001:10001`; root filesystem writes fail; `/data` and `/tmp` writes succeed. |
| Restart policy | Terminate the initialized process without stopping the service | Compose restarts it and the same `/data` state is retained. |

## Server runtime handoff gate

Runtime packaging must not be declared available until all of the following exist and the matrix above passes against a real container:

1. A server entrypoint that activates `KernelPaths::server()`, listens on the documented container address/port without the native stdin lease, and applies a server-appropriate transport/authentication policy.
2. First-run `QINGYU_SERVER_INITIALIZATION_TOKEN` validation and one-time consumption semantics, including fail-closed handling for an empty volume without a token and token-free restart for initialized data.
3. Stable live/readiness behavior and a real image healthcheck whose probe dependency is present in the runtime image.
4. A minimal runtime image that runs as the non-root UID/GID and has correct `/data` ownership without embedding the initialization token.
5. A runtime verifier that builds the image, starts Compose, exercises the persistence/restart matrix, and removes only its own isolated test resources.
