//! Copies the example extensions next to the executable (`target/<profile>/extensions`), where
//! CodeCraft looks for extensions.
//!
//! Windows only: embed the app icon and version info into `codecraft.exe`, so Explorer and
//! shortcuts show it. Elsewhere this does nothing; a missing resource compiler is only a warning.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app-icon/codecraft.ico");
    println!("cargo:rerun-if-changed=extensions");
    copy_extensions();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/app-icon/codecraft.ico")
        .set("ProductName", "codecraft")
        .set("FileDescription", "codecraft")
        .set("LegalCopyright", "MIT OR Apache-2.0")
        .set("OriginalFilename", "codecraft.exe")
        .set("InternalName", "codecraft");
    if let Err(e) = res.compile() {
        println!("cargo:warning=codecraft.exe built without its icon: {e}");
    }
}

/// `extensions/*` into the profile's folder: `OUT_DIR` is `target/<profile>/build/<pkg>/out`.
fn copy_extensions() {
    let Some(profile) = std::env::var_os("OUT_DIR")
        .map(std::path::PathBuf::from)
        .and_then(|out| out.ancestors().nth(3).map(Path::to_owned))
    else {
        return;
    };
    if let Err(e) = copy_dir(Path::new("extensions"), &profile.join("extensions")) {
        println!("cargo:warning=example extensions not copied: {e}");
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            println!("cargo:rerun-if-changed={}", entry.path().display());
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
