fn main() {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    if let Some(process) = sys.process(sysinfo::get_current_pid().unwrap()) {
        println!("CPU: {}", process.cpu_usage());
        println!("Disk Read: {}", process.disk_usage().read_bytes);
    }
}
