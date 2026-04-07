//! Fails fast with a clear message if vendored Uppy assets are missing.
//! Generate them with `npm ci && npm run build` in this directory.

use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("static/vendor");
    for name in ["uppy-e2e.js", "uppy-core.min.css", "uppy-dashboard.min.css"] {
        let p = dir.join(name);
        assert!(
            p.is_file(),
            "missing {} — run `npm ci && npm run build` in crates/fileloft-e2e-uppy",
            p.display()
        );
        println!("cargo:rerun-if-changed={}", p.display());
    }
}
