---
title: "Run as a standalone binary"
slug: "binary"
weight: 3
---

If you do not need to embed fileloft in an existing Rust service, you can run
the prebuilt binary as a self-contained tus server. A separate Docker image is
published for each storage backend.

### Image variants

| Tag | Backend | Base |
| --- | --- | --- |
| `latest`, `fs` | Local filesystem | `debian:bookworm-slim` |
| `s3` | Amazon S3 / S3-compatible (MinIO, R2, …) | `debian:bookworm-slim` |
| `gcs` | Google Cloud Storage | `debian:bookworm-slim` |
| `azure` | Azure Blob Storage | `debian:bookworm-slim` |

All images are available from `ghcr.io/sound-systems/fileloft`.

### Common configuration

Every variant reads these environment variables:

| Variable | Default | Description |
| --- | --- | --- |
| `FILELOFT_BIND` | `0.0.0.0:8080` | Address the HTTP server binds to. |
| `FILELOFT_MAX_SIZE` | _unset_ (no limit) | Maximum allowed upload size, in bytes. |
| `FILELOFT_BASE_PATH` | `/files/` | URL path the tus endpoints are mounted under. |
| `RUST_LOG` | `info` | Tracing filter (e.g. `debug`, `fileloft_server=trace`). |

---

### Filesystem (default)

```bash
docker run --rm \
  -p 8080:8080 \
  -v fileloft-data:/var/lib/fileloft \
  ghcr.io/sound-systems/fileloft:latest
```

The server stores uploads under `/var/lib/fileloft`. Mount a volume or host
path there to persist data across restarts.

| Variable | Default | Description |
| --- | --- | --- |
| `FILELOFT_DATA_DIR` | `/var/lib/fileloft` | Directory used by the filesystem store. |

---

### Amazon S3

```bash
docker run --rm \
  -p 8080:8080 \
  -e FILELOFT_S3_BUCKET=my-uploads \
  -e AWS_ACCESS_KEY_ID \
  -e AWS_SECRET_ACCESS_KEY \
  -e AWS_REGION=us-east-1 \
  ghcr.io/sound-systems/fileloft:s3
```

Authentication uses the standard AWS SDK credential chain: environment
variables, `~/.aws/credentials`, IMDS, web identity, etc.

| Variable | Default | Description |
| --- | --- | --- |
| `FILELOFT_S3_BUCKET` | _(required)_ | S3 bucket name. |
| `FILELOFT_S3_PREFIX` | _empty_ | Object key prefix (e.g. `uploads/`). |
| `FILELOFT_S3_ENDPOINT` | _unset_ | Custom endpoint for S3-compatible services (MinIO, R2). |
| `FILELOFT_S3_REGION` | _from SDK config_ | Override the signing region. |
| `FILELOFT_S3_FORCE_PATH_STYLE` | `false` | Set to `true` for path-style addressing (often needed for MinIO). |

Standard AWS SDK variables (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`,
`AWS_REGION`, `AWS_PROFILE`, etc.) are also respected.

---

### Google Cloud Storage

```bash
docker run --rm \
  -p 8080:8080 \
  -e FILELOFT_GCS_BUCKET=my-uploads \
  -v /path/to/keyfile.json:/credentials.json:ro \
  -e GOOGLE_APPLICATION_CREDENTIALS=/credentials.json \
  ghcr.io/sound-systems/fileloft:gcs
```

Authentication uses Application Default Credentials. On GCE/GKE the attached
service account is used automatically. Outside Google Cloud, mount a service
account key file and set `GOOGLE_APPLICATION_CREDENTIALS`.

| Variable | Default | Description |
| --- | --- | --- |
| `FILELOFT_GCS_BUCKET` | _(required)_ | GCS bucket name. |
| `FILELOFT_GCS_PREFIX` | _empty_ | Object name prefix (e.g. `uploads/`). |

---

### Azure Blob Storage

```bash
docker run --rm \
  -p 8080:8080 \
  -e FILELOFT_AZURE_CONTAINER=my-uploads \
  -e AZURE_STORAGE_CONNECTION_STRING \
  ghcr.io/sound-systems/fileloft:azure
```

The Azure image supports two authentication modes:

1. **Connection string** — set `FILELOFT_AZURE_CONNECTION_STRING` or
   `AZURE_STORAGE_CONNECTION_STRING`.
2. **Default credential** — set `FILELOFT_AZURE_ACCOUNT` (or
   `AZURE_STORAGE_ACCOUNT`) and let the Azure Identity SDK resolve credentials
   (managed identity, Azure CLI, environment variables).

| Variable | Default | Description |
| --- | --- | --- |
| `FILELOFT_AZURE_CONTAINER` | _(required)_ | Blob container name. |
| `FILELOFT_AZURE_PREFIX` | _empty_ | Blob name prefix (e.g. `uploads/`). |
| `FILELOFT_AZURE_CONNECTION_STRING` | _unset_ | Azure Storage connection string (takes priority). |
| `FILELOFT_AZURE_ACCOUNT` | _unset_ | Storage account name (used with default credentials). |

---

### Building from source

The repository includes a multi-stage `Dockerfile`. Select a backend with the
`BACKEND` build arg:

```bash
docker build --build-arg BACKEND=s3 -t fileloft:s3 .
```

Or use the Makefile targets:

```bash
make docker-build-fs       # builds :latest and :fs
make docker-build-s3       # builds :s3
make docker-build-gcs      # builds :gcs
make docker-build-azure    # builds :azure
make docker-build-all      # builds all variants
```

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
