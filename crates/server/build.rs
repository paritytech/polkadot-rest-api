// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Build script that copies `docs/dist` into `OUT_DIR` so that `include_dir!`
//! works both for local workspace builds and for `cargo publish` verification
//! (where the package tarball cannot reference paths outside the crate root).

use std::path::Path;
use std::{env, fs};

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("docs_dist");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let src = Path::new(&manifest_dir).join("../../docs/dist");

    println!("cargo:rerun-if-changed=../../docs/dist");

    fs::create_dir_all(&dest).unwrap();

    if src.is_dir() {
        for entry in fs::read_dir(&src).unwrap() {
            let entry = entry.unwrap();
            let file_name = entry.file_name();
            fs::copy(entry.path(), dest.join(&file_name)).unwrap();
        }
    } else {
        // Source not available (e.g. building from a crates.io tarball).
        // Write a minimal placeholder so include_dir! still compiles.
        fs::write(
            dest.join("index.html"),
            "<html><body>Documentation not available in this build.</body></html>",
        )
        .unwrap();
    }
}
