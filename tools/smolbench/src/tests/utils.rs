use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::time::sleep;

fn get_smoldb_exec() -> PathBuf {
    let smolbench_dir = std::env::current_dir().expect("Failed to get current directory");
    let smoldb_dir = smolbench_dir.parent().unwrap().parent().unwrap();
    smoldb_dir.join("target").join("debug").join("smoldb")
}

pub async fn start_peer(
    peer_dir: &Path,
    log_file: &str,
    peer_id: u64,
    port: u32,
    p2p_port: u32,
    bootstrap: Option<String>,
) -> std::process::Child {
    let smoldb_path = get_smoldb_exec();
    let log_path = peer_dir.join(log_file); // ToDo: Add logging

    if !smoldb_path.exists() {
        panic!("Smoldb executable not found at {:?}", smoldb_path);
    }

    fs::create_dir_all(&peer_dir).expect("Failed to create peer directory");

    // Create and open log file
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .expect("Failed to open log file");

    let mut smoldb = Command::new(smoldb_path);
    let http_url = format!("http://0.0.0.0:{port}");
    let p2p_url = format!("http://0.0.0.0:{p2p_port}");

    let cmd = smoldb
        .arg("--peer-id")
        .arg(peer_id.to_string())
        .arg("--url")
        .arg(http_url.clone())
        .arg("--p2p-url")
        .arg(p2p_url);

    if let Some(bootstrap) = bootstrap {
        cmd.arg("--bootstrap").arg(bootstrap);
    }

    let child = cmd
        .current_dir(&peer_dir)
        .stdout(Stdio::from(
            log_file
                .try_clone()
                .expect("Failed to clone log file handle"),
        ))
        .stderr(Stdio::from(log_file))
        .spawn()
        .expect("Failed to start Smoldb peer");

    sleep(Duration::from_secs(3)).await; // Wait for the peer to start

    println!("Started Smoldb peer at {http_url} in {peer_dir:?}");

    child
}
