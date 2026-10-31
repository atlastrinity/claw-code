use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Rust sets SIGPIPE to `SIG_IGN` at startup, so the `print!`/`println!`
/// macros panic when stdout has already been hung up by the downstream reader
/// — `claw config | head`, `claw skills list | grep -q`, and so on. Before the
/// startup guard the child died with "failed printing to stdout: Broken pipe"
/// and exit code 101. The panic hook turns that one panic into a quiet exit,
/// so a consumer that stops reading is treated as done, not as a crash.
///
/// The generated config is deliberately larger than the OS pipe buffer so the
/// child is still blocked mid-write when the read end closes — that makes the
/// broken pipe happen on every run, not only when the child happens to print
/// after we close.
#[test]
fn closed_stdout_is_not_reported_as_a_panic() {
    let home = unique_temp_dir("broken-pipe");
    let config_home = home.join("config");
    std::fs::create_dir_all(&config_home).expect("config home should exist");
    write_bulky_settings(&config_home.join("settings.json"));

    let mut child = Command::new(env!("CARGO_BIN_EXE_claw"))
        .args(["config"])
        .current_dir(&home)
        .env_clear()
        .env("HOME", &home)
        .env("CLAW_CONFIG_HOME", &config_home)
        .env("CLAW_LOG_DIR", home.join("logs"))
        .env("NO_COLOR", "1")
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("claw should launch");

    // Drop the read end before the child finishes emitting its report.
    drop(child.stdout.take());

    let mut stderr = Vec::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_end(&mut stderr);
    }
    let status = child.wait().expect("claw should finish");

    let stderr = String::from_utf8_lossy(&stderr);
    assert!(
        !stderr.contains("panicked at"),
        "a closed stdout must not panic the process; stderr was:\n{stderr}"
    );
    assert!(
        !stderr.contains("Broken pipe"),
        "a hung-up stdout must not surface a broken-pipe error; stderr was:\n{stderr}"
    );
    assert_ne!(
        status.code(),
        Some(101),
        "exit code 101 means a panic escaped instead of a quiet exit; status: {status}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

/// A config whose merged JSON comfortably exceeds the 64 KiB pipe buffer, so
/// `claw config` cannot flush it in one non-blocking write.
fn write_bulky_settings(path: &std::path::Path) {
    let mut servers = String::from("{\n  \"availableMcpServers\": {");
    for index in 0..1_500usize {
        servers.push_str(&format!(
            "\"srv-{index:04}\":{{\"command\":\"npx\",\"args\":[\"-y\",\"pkg-{index:04}\"],\
             \"description\":\"padding to push the merged config past the pipe buffer for run {index}\"}},",
        ));
    }
    servers.push_str("\"pad\":{\"command\":\"x\",\"args\":[\"tail\"]}}}");
    std::fs::write(path, servers).expect("settings should be writable");
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_millis();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "claw-{label}-{}-{millis}-{counter}",
        std::process::id()
    ))
}
