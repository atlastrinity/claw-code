fn main() {
    use sysinfo::System;
    let mut sys = System::new_all();
    let current_pid = std::process::id();
    println!("Current PID: {}", current_pid);
    
    let targets = ["claw", "xcodebuildmcp", "mcp-server-macos-use", "claw-analog", "claw-rag-service"];
    
    for (pid, process) in sys.processes() {
        if pid.as_u32() == current_pid {
            continue;
        }
        let name_str = process.name().to_string_lossy().to_lowercase();
        for target in &targets {
            if name_str.contains(target) {
                println!("Would kill: {}", name_str);
                // process.kill();
                break;
            }
        }
    }
}
