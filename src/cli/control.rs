use std::fs;
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

fn state_dir() -> PathBuf {
    std::env::var("BENDCLAW_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_home().join(".evot"))
}

/// Default config file path: `~/.evot/bendclaw.toml`.
pub fn default_config_path() -> PathBuf {
    state_dir().join("bendclaw.toml")
}

fn run_dir() -> PathBuf {
    state_dir().join("run")
}

fn default_log_dir() -> PathBuf {
    dirs_home().join(".evotai").join("logs")
}

fn log_dir() -> PathBuf {
    std::env::var("BENDCLAW_LOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_log_dir())
}

fn pid_file() -> PathBuf {
    run_dir().join("bendclaw.pid")
}

fn log_file() -> PathBuf {
    log_dir().join("bendclaw.out")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn read_pid() -> Option<u32> {
    fs::read_to_string(pid_file())
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()
}

fn write_pid(pid: u32) {
    let dir = run_dir();
    fs::create_dir_all(&dir).ok();
    fs::write(pid_file(), pid.to_string()).ok();
}

fn remove_pid() {
    fs::remove_file(pid_file()).ok();
}

fn is_running(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn kill_process(pid: u32, timeout: Duration) -> bool {
    if !is_running(pid) {
        return true;
    }

    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }

    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !is_running(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
    std::thread::sleep(Duration::from_millis(100));
    !is_running(pid)
}

fn wait_for_port(host: &str, port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let Ok(addrs) = (host, port).to_socket_addrs() else {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        };
        if addrs
            .into_iter()
            .any(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok())
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    false
}

fn parse_bind_addr() -> (String, u16) {
    let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8787".into());
    if let Some((host, port)) = bind.rsplit_once(':') {
        (host.to_string(), port.parse().unwrap_or(8787))
    } else {
        ("0.0.0.0".to_string(), 8787)
    }
}

pub fn cmd_start() {
    if let Some(pid) = read_pid() {
        if is_running(pid) {
            println!("🦞 BendClaw is already running (PID {pid})");
            return;
        }
        remove_pid();
    }

    let log = log_file();
    if let Some(dir) = log.parent() {
        fs::create_dir_all(dir).ok();
    }
    let lf = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .expect("failed to open log file");
    let lf2 = lf.try_clone().expect("failed to clone log file handle");

    let exe = std::env::current_exe().expect("failed to get current executable");

    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("run").stdin(Stdio::null()).stdout(lf).stderr(lf2);

    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let child = cmd.spawn().expect("failed to spawn bendclaw");
    let pid = child.id();
    write_pid(pid);
    drop(child);

    let (host, port) = parse_bind_addr();
    let listen_host = if host == "0.0.0.0" {
        "127.0.0.1"
    } else {
        &host
    };

    print!("🦞 BendClaw starting (PID {pid})...");
    if wait_for_port(listen_host, port, Duration::from_secs(60)) {
        println!(" ready!");
        println!("   Address : http://{host}:{port}");
        println!("   Log     : {}", log.display());
        println!("   PID file: {}", pid_file().display());
    } else {
        println!(" timed out waiting for port {port}");
        println!("   Check log: {}", log.display());
    }
}

pub fn cmd_stop() {
    let Some(pid) = read_pid() else {
        println!("🦞 BendClaw is not running");
        return;
    };

    if !is_running(pid) {
        println!("🦞 BendClaw is not running (stale PID {pid})");
        remove_pid();
        return;
    }

    print!("🦞 Stopping BendClaw (PID {pid})...");
    if kill_process(pid, Duration::from_secs(5)) {
        println!(" stopped");
        remove_pid();
    } else {
        println!(" failed to stop");
    }
}

pub fn cmd_restart() {
    if let Some(pid) = read_pid() {
        if is_running(pid) {
            cmd_stop();
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    cmd_start();
}

pub fn cmd_status() {
    let state = state_dir();
    let log = log_file();

    match read_pid() {
        Some(pid) if is_running(pid) => {
            println!("🦞 BendClaw is running");
            println!("   PID      : {pid}");
            println!("   Log      : {}", log.display());
            println!("   State dir: {}", state.display());
        }
        Some(pid) => {
            println!("🦞 BendClaw is not running (stale PID {pid})");
            remove_pid();
        }
        None => {
            println!("🦞 BendClaw is not running");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_background_log_dir_uses_evotai_logs() {
        assert_eq!(default_log_dir(), dirs_home().join(".evotai").join("logs"));
    }
}
