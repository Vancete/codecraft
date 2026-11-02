//! What the explorer's context menu does to files: create, rename, copy and move them, send
//! them to the Recycle Bin (or the system's trash), and show them in the file manager.

use std::io;
use std::path::{Path, PathBuf};

/// Why a name can't be a file's: empty, a path, or reserved.
pub fn check_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A name is needed".to_owned());
    }
    if name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(format!("'{name}' isn't a valid name"));
    }
    if cfg!(windows) && name.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return Err(format!("'{name}' has characters Windows doesn't allow"));
    }
    Ok(())
}

/// Creates an empty file `name` in `dir`.
pub fn new_file(dir: &Path, name: &str) -> Result<PathBuf, String> {
    check_name(name)?;
    let path = dir.join(name.trim());
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| describe(&path, e))?;
    Ok(path)
}

/// Creates folder `name` in `dir`.
pub fn new_folder(dir: &Path, name: &str) -> Result<PathBuf, String> {
    check_name(name)?;
    let path = dir.join(name.trim());
    std::fs::create_dir(&path).map_err(|e| describe(&path, e))?;
    Ok(path)
}

/// Renames `path` to `name` in the same folder.
pub fn rename(path: &Path, name: &str) -> Result<PathBuf, String> {
    check_name(name)?;
    let to = path.with_file_name(name.trim());
    if to == path {
        return Ok(to);
    }
    // A change of case only is the same file on Windows and macOS: no "already exists" there.
    let same = to
        .to_string_lossy()
        .eq_ignore_ascii_case(&path.to_string_lossy());
    if to.exists() && !same {
        return Err(format!("{} already exists", display_name(&to)));
    }
    std::fs::rename(path, &to).map_err(|e| describe(path, e))?;
    Ok(to)
}

/// Copies `from` (a file or a folder with everything in it) into `dir`, as "name copy" when the
/// name is taken.
pub fn copy_into(from: &Path, dir: &Path) -> Result<PathBuf, String> {
    if dir.starts_with(from) && from.is_dir() {
        return Err(format!("Can't copy {} into itself", display_name(from)));
    }
    let to = free_name(dir, from);
    copy_all(from, &to).map_err(|e| describe(from, e))?;
    Ok(to)
}

/// Moves `from` into `dir` (a copy, then removing the original, across drives).
pub fn move_into(from: &Path, dir: &Path) -> Result<PathBuf, String> {
    if from.parent() == Some(dir) {
        return Ok(from.to_owned());
    }
    if dir.starts_with(from) {
        return Err(format!("Can't move {} into itself", display_name(from)));
    }
    let to = free_name(dir, from);
    if std::fs::rename(from, &to).is_err() {
        copy_all(from, &to).map_err(|e| describe(from, e))?;
        let removed = if from.is_dir() {
            std::fs::remove_dir_all(from)
        } else {
            std::fs::remove_file(from)
        };
        removed.map_err(|e| describe(from, e))?;
    }
    Ok(to)
}

/// Sends `path` to the Recycle Bin / trash.
pub fn trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| format!("{}: {e}", display_name(path)))
}

/// Shows `path` selected in the system's file manager (Explorer, Finder), or its folder.
pub fn reveal(path: &Path) -> Result<(), String> {
    let shown = if cfg!(windows) {
        let mut select = std::ffi::OsString::from("/select,");
        select.push(path);
        std::process::Command::new("explorer")
            .arg(select)
            .spawn()
            .map(|_| ())
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map(|_| ())
    } else {
        open::that_detached(path.parent().unwrap_or(path))
    };
    shown.map_err(|e| describe(path, e))
}

/// What the file manager is called here, for the menu.
pub fn file_manager() -> &'static str {
    if cfg!(windows) {
        "Reveal in File Explorer"
    } else if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else {
        "Open Containing Folder"
    }
}

/// `name` in `dir`, or "stem copy.ext", "stem copy 2.ext"… when taken, as VS Code names them.
fn free_name(dir: &Path, from: &Path) -> PathBuf {
    let name = display_name(from);
    let to = dir.join(&name);
    if !to.exists() {
        return to;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !from.is_dir() => (stem, format!(".{ext}")),
        _ => (name.as_str(), String::new()),
    };
    (1..)
        .map(|n| {
            let copy = if n == 1 {
                format!("{stem} copy{ext}")
            } else {
                format!("{stem} copy {n}{ext}")
            };
            dir.join(copy)
        })
        .find(|p| !p.exists())
        .unwrap_or(to)
}

fn copy_all(from: &Path, to: &Path) -> io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn describe(path: &Path, e: io::Error) -> String {
    match e.kind() {
        io::ErrorKind::AlreadyExists => format!("{} already exists", display_name(path)),
        _ => format!("{}: {e}", display_name(path)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("codecraft-ops-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn creates_renames_and_refuses_bad_names() {
        let dir = scratch("names");
        let file = new_file(&dir, "a.txt");
        assert_eq!(file.as_deref().ok(), Some(dir.join("a.txt").as_path()));
        assert!(new_file(&dir, "a.txt").is_err());
        assert!(new_file(&dir, "x/y").is_err());
        assert!(new_folder(&dir, "  ").is_err());
        assert!(new_folder(&dir, "sub").is_ok());
        assert!(rename(&dir.join("a.txt"), "sub").is_err());
        assert_eq!(
            rename(&dir.join("a.txt"), "b.txt").ok(),
            Some(dir.join("b.txt"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_as_copy_and_moves() {
        let dir = scratch("copy");
        let sub = dir.join("sub");
        let _ = std::fs::create_dir_all(sub.join("deep"));
        let _ = std::fs::write(sub.join("deep/f.rs"), "x");
        let _ = std::fs::write(dir.join("a.rs"), "a");
        assert_eq!(
            copy_into(&dir.join("a.rs"), &dir).ok(),
            Some(dir.join("a copy.rs"))
        );
        assert_eq!(
            copy_into(&dir.join("a.rs"), &dir).ok(),
            Some(dir.join("a copy 2.rs"))
        );
        assert_eq!(copy_into(&sub, &dir).ok(), Some(dir.join("sub copy")));
        assert!(dir.join("sub copy/deep/f.rs").is_file());
        assert!(copy_into(&sub, &sub.join("deep")).is_err());
        assert!(move_into(&sub, &sub.join("deep")).is_err());
        assert_eq!(
            move_into(&dir.join("a.rs"), &sub).ok(),
            Some(sub.join("a.rs"))
        );
        assert!(!dir.join("a.rs").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
