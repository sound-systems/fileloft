---
title: "Run as a standalone binary"
slug: "binary"
weight: 3
---

If you do not need to embed fileloft in an existing Rust service, you can run
the prebuilt binary as a self-contained tus server. It is published as a
Docker image and can run in any container runtime.

### Run with Docker

```bash
docker run --rm \
  -p 8080:8080 \
  -v fileloft-data:/var/lib/fileloft \
  ghcr.io/sound-systems/fileloft:latest
```

The server listens on `:8080` and stores uploads under `/var/lib/fileloft`.
Mount a volume (or a host path) there to persist uploads across restarts.

### Configuration

Configuration is read from environment variables. The defaults are safe for
local development; review them before deploying.

| Variable | Default | Description |
| --- | --- | --- |
| `FILELOFT_BIND` | `0.0.0.0:8080` | Address the HTTP server binds to. |
| `FILELOFT_DATA_DIR` | `/var/lib/fileloft` | Directory used by the filesystem store. |
| `FILELOFT_MAX_SIZE` | _unset_ | Maximum allowed upload size, in bytes. |
| `FILELOFT_BASE_PATH` | `/files` | Path the tus endpoints are mounted under. |

### Verifying it works

Any tus 1.0.0 client will work. For a quick smoke test:

```bash
curl -i -X POST http://localhost:8080/files \
  -H "Tus-Resumable: 1.0.0" \
  -H "Upload-Length: 11" \
  -H "Upload-Metadata: filename aGVsbG8udHh0"
```

A `201 Created` with a `Location` header means the server is healthy and
ready to accept upload chunks.
