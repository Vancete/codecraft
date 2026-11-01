//! Files opened from the system's file manager (or the command line) while CodeCraft is already
//! running open in a tab of the running window instead of a second one.
//!
//! The running app listens on a localhost port and writes the port and a random token to a file
//! in its settings folder; a new instance given a file sends the path there with the token and
//! exits. A stale file (the app crashed) just means the new instance starts normally.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

fn instance_file() -> Option<PathBuf> {
    eframe::storage_dir("codecraft").map(|d| d.join("instance"))
}

/// Hands `path` to the running instance; true when it took it.
pub fn forward(path: &Path) -> bool {
    let send = || -> Option<()> {
        let info = std::fs::read_to_string(instance_file()?).ok()?;
        let (port, token) = info.trim().split_once(' ')?;
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port.parse::<u16>().ok()?));
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        writeln!(stream, "{token}\n{}", path.display()).ok()?;
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).ok()?;
        (reply.trim() == "ok").then_some(())
    };
    send().is_some()
}

/// Starts taking paths from new instances; each arrives on the returned channel, waking `ctx`.
pub fn listen(ctx: egui::Context) -> Option<Receiver<PathBuf>> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    let token = format!(
        "{:016x}{:016x}",
        RandomState::new().build_hasher().finish(),
        RandomState::new().build_hasher().finish()
    );
    let file = instance_file()?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).ok()?;
    }
    std::fs::write(&file, format!("{port} {token}")).ok()?;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut lines = BufReader::new(&stream).lines();
            let (Some(Ok(sent)), Some(Ok(path))) = (lines.next(), lines.next()) else {
                continue;
            };
            if sent != token {
                continue;
            }
            if tx.send(PathBuf::from(path)).is_err() {
                break;
            }
            let _ = (&stream).write_all(b"ok\n");
            ctx.request_repaint();
        }
    });
    Some(rx)
}
