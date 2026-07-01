use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// This build script embeds the Railgun sidecar sources (JS + package.json)
/// so they can be extracted at runtime to the user's data directory.
///
/// We deliberately do NOT embed node_modules. After extraction we can run
/// `npm install --production` once if the dependencies are missing.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let sidecars_dir = out_dir.join("railgun_sidecars");
    fs::create_dir_all(&sidecars_dir).expect("failed to create sidecars dir in OUT_DIR");

    // Source locations (relative to the workspace root when building zeus-railgun)
    // These are the current development locations.
    let workspace_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();

    let prover_src = workspace_root.join("crates/zeus-railgun-prover/js-sidecar");
    let waku_src = workspace_root.join("crates/zeus-waku-broadcaster/js-sidecar");

    // Embed only the files we actually need (source + package.json)
    copy_sidecar_files(&prover_src, &sidecars_dir.join("prover"), "prover");
    copy_sidecar_files(&waku_src, &sidecars_dir.join("waku"), "waku");

    println!("cargo:warning=Railgun sidecars embedded into OUT_DIR for runtime extraction");
}

fn copy_sidecar_files(src_dir: &Path, dest_dir: &Path, name: &str) {
    fs::create_dir_all(dest_dir).unwrap_or_else(|_| panic!("failed to create {} dest", name));

    // Copy package.json
    let pkg_src = src_dir.join("package.json");
    if pkg_src.exists() {
        let pkg_dest = dest_dir.join("package.json");
        fs::copy(&pkg_src, &pkg_dest)
            .unwrap_or_else(|_| panic!("failed to copy package.json for {}", name));
        println!("cargo:rerun-if-changed={}", pkg_src.display());
    }

    // Copy src/index.js (the actual sidecar)
    let index_src = src_dir.join("src/index.js");
    if index_src.exists() {
        let src_dest = dest_dir.join("src");
        fs::create_dir_all(&src_dest).ok();
        let index_dest = src_dest.join("index.js");
        fs::copy(&index_src, &index_dest)
            .unwrap_or_else(|_| panic!("failed to copy src/index.js for {}", name));
        println!("cargo:rerun-if-changed={}", index_src.display());
    } else {
        // Some sidecars might have index.js at root
        let index_src_root = src_dir.join("index.js");
        if index_src_root.exists() {
            let index_dest = dest_dir.join("index.js");
            fs::copy(&index_src_root, &index_dest)
                .unwrap_or_else(|_| panic!("failed to copy index.js for {}", name));
            println!("cargo:rerun-if-changed={}", index_src_root.display());
        }
    }

    // If there are other small local files in src/ (not node_modules), copy them too
    let src_subdir = src_dir.join("src");
    if src_subdir.exists() {
        for entry in fs::read_dir(&src_subdir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |e| e == "js" || e == "ts" || e == "mjs") {
                let file_name = path.file_name().unwrap();
                let dest = dest_dir.join("src").join(file_name);
                fs::copy(&path, &dest).ok();
            }
        }
    }
}
