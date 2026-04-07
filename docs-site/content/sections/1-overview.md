---
title: "Why fileloft"
slug: "overview"
weight: 1
---

fileloft is a Rust implementation of the [tus](https://tus.io) resumable upload
protocol. It is designed as a small set of composable crates that you can drop
into an existing Rust HTTP server, or run as a standalone binary in Docker or
any other container runtime.

{{< props >}}
{{< prop title="Framework agnostic" >}}
A protocol core with no transport assumptions, plus thin adapters for Axum,
Actix Web, and Rocket. Bring your own router.
{{< /prop >}}
{{< prop title="Pluggable storage" >}}
A `DataStore` trait with first-party in-memory and filesystem backends. Add
your own for S3, GCS, or anything else.
{{< /prop >}}
{{< prop title="Standalone or embedded" >}}
Use it as a library inside a custom Rust server, or run the prebuilt binary
in Docker when you just need a tus endpoint.
{{< /prop >}}
{{< prop title="Safe by default" >}}
`#![forbid(unsafe_code)]` across the workspace. Conservative defaults for
limits, locking, and checksums.
{{< /prop >}}
{{< /props >}}
