//! Embed the built web app (`web/dist`) in the program, so one file is enough.
//! Build it first with `npm run build` in `web/`; without it the program
//! still runs and says what is missing.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    let mut files = vec![];
    walk(&dist, &mut files);
    files.sort();
    let mut code = String::from("/// The web app's files: (path below the site root, contents).\npub static ASSETS: &[(&str, &[u8])] = &[\n");
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
        let rel = f.strip_prefix(&dist).unwrap().to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
        let abs = f.canonicalize().unwrap().to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
        let abs = abs.trim_start_matches("//?/");
        writeln!(code, "    ({rel:?}, include_bytes!({abs:?})),").unwrap();
    }
    code.push_str("];\n");
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("assets.rs");
    std::fs::write(out, code).unwrap();
}
