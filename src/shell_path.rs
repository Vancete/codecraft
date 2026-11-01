//! macOS apps opened from the Finder or the Dock get a bare PATH (`/usr/bin:/bin:/usr/sbin:/sbin`),
//! without Homebrew's folders, where git, gh, glab and editors' commands usually are. The
//! commands CodeCraft starts get the PATH of the user's login shell instead.

use std::ffi::OsString;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Folders added in any case (Homebrew on Apple silicon, then on Intel).
const FALLBACK: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin"];

/// The PATH for the terminal's shells, when it differs from its own (macOS only).
pub fn path() -> Option<&'static OsString> {
    static PATH: OnceLock<Option<OsString>> = OnceLock::new();
    PATH.get_or_init(|| {
        if cfg!(target_os = "macos") {
            login_path()
        } else {
            None
        }
    })
    .as_ref()
}

fn login_path() -> Option<OsString> {
    let own = std::env::var_os("PATH").unwrap_or_default();
    let shell = shell_path().unwrap_or_default();
    let joined = merge([&shell, &own]);
    (joined != own).then_some(joined)
}

/// Every folder of `paths` once, in order, then the fallbacks.
fn merge<const N: usize>(paths: [&OsString; N]) -> OsString {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let candidates = paths
        .iter()
        .flat_map(|p| std::env::split_paths(p).collect::<Vec<_>>())
        .chain(FALLBACK.iter().map(PathBuf::from));
    for dir in candidates {
        if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

/// `$PATH` as the user's login shell sets it, if it answers within a few seconds.
fn shell_path() -> Option<OsString> {
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    let mut child = Command::new(shell)
        .args(["-l", "-c", "printf %s \"$PATH\""])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    let out = out.trim();
    (!out.is_empty()).then(|| out.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_kept_once_in_order() {
        let shell = std::env::join_paths(["/opt/homebrew/bin", "/usr/bin"]).unwrap_or_default();
        let own = std::env::join_paths(["/usr/bin", "/bin"]).unwrap_or_default();
        let merged: Vec<PathBuf> = std::env::split_paths(&merge([&shell, &own])).collect();
        assert_eq!(
            merged,
            ["/opt/homebrew/bin", "/usr/bin", "/bin", "/usr/local/bin"].map(PathBuf::from)
        );
    }
}
