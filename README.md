# Workmesh

Standalone distribution of workmesh. Clone and build without downloading other Mesh applications.

```sh
git clone https://github.com/luis-lobo-ar/workmesh.git
cd workmesh
cargo build --locked --release -p workmesh -p workmesh-mcp
cargo test --locked -p mesh-config -p mesh-id -p mesh-store -p mesh-testkit -p workmesh -p workmesh-core -p workmesh-mcp -p workmesh-mcp-server -p workmesh-render -p workmesh-tools
```

Linux x86_64 binaries and checksums are available under [Releases](https://github.com/luis-lobo-ar/workmesh/releases). Extract the archive and add its `bin/` directory to PATH. Snapshot releases are prereleases built from the exact source revision below.

Product source and manuals: [opt/workmesh](opt/workmesh/). [Standalone guide](DISTRIBUTION.html). Shared crates under `lib/` are generated from the same Mesh revision and are included only when this product needs them.

Authoritative development: the Mesh monorepo. This repository receives generated snapshots. Open issues here; proposed changes must be integrated into Mesh by its maintainer before the next snapshot. Do not edit the generated shared crates independently.

Source commit: `8991833d58325eaf3ee63b68e6c0655bee6b9c28`. Full file hashes and exporter provenance: [mesh-export.json](mesh-export.json). No private monorepo history, active task records or session data are included.

MIT licensed Mesh code; third-party assets retain the notices in LICENSES.txt and their original licenses.

Install from source: `cargo install --locked --path opt/workmesh/crates/workmesh-cli` and `cargo install --locked --path opt/workmesh/crates/workmesh-mcp`. MCP: run `workmesh-mcp` from your project directory.
