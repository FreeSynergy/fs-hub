//! The real `fs-hub` binary as a child process: to kill it without any shutdown.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use fs_hub_client::{HubSession, HubStatus, connect_at};

use super::notes_manifest;

/// The running binary. Gone with this value, also when a test panics: a child left behind would keep
/// the output of `cargo test` open.
pub struct Hub(Child);

impl Hub {
    pub fn id(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn start(socket: &Path, data: &Path) -> Hub {
    let child = Command::new(env!("CARGO_BIN_EXE_fs-hub"))
        .arg("--socket")
        .arg(socket)
        .arg("--data")
        .arg(data)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    Hub(child)
}

pub async fn connect(socket: &Path, name: &str) -> HubSession {
    for _ in 0..200 {
        if socket.exists()
            && let Ok(HubStatus::Connected(s)) = connect_at(socket, notes_manifest(name)).await
        {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the hub did not come up");
}

pub fn kill(hub: Hub) {
    drop(hub);
}
