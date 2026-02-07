fn main() {
    tauri_build::build();

    // On some macOS versions, the Swift Concurrency runtime is not present in the
    // system dyld cache, but ScreenCaptureKit's Swift glue still links against
    // `@rpath/libswift_Concurrency.dylib`.
    //
    // During `cargo tauri dev`, the app runs as a raw binary in `target/<profile>/`,
    // so we copy the dylib next to the binary (a path dyld already searches) to
    // avoid a crash at launch.
    #[cfg(target_os = "macos")]
    copy_swift_concurrency_runtime_near_binary();
}

#[cfg(target_os = "macos")]
fn copy_swift_concurrency_runtime_near_binary() {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const LIB_NAME: &str = "libswift_Concurrency.dylib";

    fn read_xcode_select_path() -> Option<PathBuf> {
        let out = Command::new("xcode-select").arg("-p").output().ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout);
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        Some(PathBuf::from(s))
    }

    fn profile_dir_from_out_dir(out_dir: &Path) -> Option<PathBuf> {
        // OUT_DIR typically ends like:
        //   .../target/debug/build/<crate-hash>/out
        // We want:
        //   .../target/debug
        out_dir
            .ancestors()
            .find(|p| p.file_name().is_some_and(|n| n == "build"))
            .and_then(|build_dir| build_dir.parent())
            .map(|p| p.to_path_buf())
    }

    fn find_lib_in_toolchain(dev_dir: &Path) -> Option<PathBuf> {
        let toolchain = dev_dir.join("Toolchains/XcodeDefault.xctoolchain/usr/lib");

        // Common locations across Xcode installs.
        let direct_candidates = [
            toolchain.join("swift/macosx").join(LIB_NAME),
            toolchain.join("swift-5.5/macosx").join(LIB_NAME),
        ];
        for p in direct_candidates {
            if p.exists() {
                return Some(p);
            }
        }

        // If the Swift version directory is different (swift-5.6, swift-5.7, ...),
        // scan and pick the first one that contains the dylib.
        if let Ok(entries) = fs::read_dir(&toolchain) {
            for ent in entries.flatten() {
                let path = ent.path();
                let name = ent.file_name();
                let name = name.to_string_lossy();
                if !path.is_dir() {
                    continue;
                }
                if !name.starts_with("swift-") {
                    continue;
                }
                let cand = path.join("macosx").join(LIB_NAME);
                if cand.exists() {
                    return Some(cand);
                }
            }
        }

        None
    }

    let out_dir = match env::var("OUT_DIR") {
        Ok(v) => PathBuf::from(v),
        Err(_) => return,
    };

    let profile_dir = match profile_dir_from_out_dir(&out_dir) {
        Some(p) => p,
        None => return,
    };

    let developer_dir = env::var("DEVELOPER_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(read_xcode_select_path)
        .unwrap_or_else(|| PathBuf::from("/Applications/Xcode.app/Contents/Developer"));

    let src = match find_lib_in_toolchain(&developer_dir) {
        Some(p) => p,
        None => {
            // Don't fail the build; the app will fail to launch with a clear dyld
            // error, but this keeps CI and non-SCK builds moving.
            println!(
                "cargo:warning=Could not locate {LIB_NAME} in the Xcode toolchain (DEVELOPER_DIR={}). ScreenCaptureKit capture may fail to launch.",
                developer_dir.display()
            );
            return;
        }
    };

    let dest = profile_dir.join(LIB_NAME);
    let should_copy = match (fs::metadata(&src), fs::metadata(&dest)) {
        (Ok(sm), Ok(dm)) => sm.len() != dm.len(),
        (Ok(_), Err(_)) => true,
        _ => true,
    };

    if should_copy {
        if let Err(err) = fs::copy(&src, &dest) {
            println!(
                "cargo:warning=Failed to copy {LIB_NAME} to {}: {err}",
                dest.display()
            );
        }
    }
}
