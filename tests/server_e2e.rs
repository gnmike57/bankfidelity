use dual_core_pdf_pipeline::app::audit::AuditLog;
use dual_core_pdf_pipeline::app::runtime::Runtime;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Wait for a TCP listener to accept connections on `port`, with bounded
/// retries instead of a blind `thread::sleep`.
fn wait_for_server(port: u16, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("Server did not start on port {port} within {:?}", timeout);
}

#[test]
fn test_headless_server_e2e() {
    // Pick a random available port to avoid collisions with other tests or
    // services. We briefly bind to port 0, record the assigned port, then
    // drop the listener so the server can re-bind it.
    let port = {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("failed to bind ephemeral port");
        listener.local_addr().unwrap().port()
    };

    // SAFETY: `set_var` is deprecated since Rust 1.80 because it is not
    // thread-safe. We call it here *before* spawning any threads that read
    // the PORT variable. The server reads PORT exactly once at startup. No
    // other test should set PORT concurrently because each test uses its own
    // random port. This is acceptable in a test-only context.
    unsafe {
        std::env::set_var("PORT", port.to_string());
    }

    let cfg = Arc::new(dual_core_pdf_pipeline::app::config::AppConfig::default());
    let runtime_dir = tempfile::tempdir().unwrap();
    let audit = AuditLog::open(runtime_dir.path()).unwrap();
    let (_runtime, job_tx, res_rx) = Runtime::start(audit, cfg.clone());

    // Spawn the server in the background. The server blocks on
    // `listener.incoming()` which is a blocking iterator. The server thread is
    // detached and terminates with the test process; the real runtime remains
    // alive for the full test scope.
    //
    // The resource impact is bounded: one thread + one TCP port per test
    // invocation, automatically reclaimed at process exit.
    thread::spawn(move || {
        let _ = dual_core_pdf_pipeline::app::server::run_server(job_tx, res_rx, cfg);
    });

    // Wait for the server to be ready with retry-based synchronization
    // instead of a blind sleep.
    wait_for_server(port, Duration::from_secs(5));

    let base = format!("http://127.0.0.1:{port}");

    // Test 1: Liveness probe
    let health_resp =
        reqwest::blocking::get(format!("{base}/health")).expect("Failed to fetch /health");
    assert_eq!(health_resp.status(), 200);
    assert_eq!(health_resp.text().unwrap(), r#"{"status":"ok"}"#);

    // Test 2: Readiness probe
    let ready_resp =
        reqwest::blocking::get(format!("{base}/readyz")).expect("Failed to fetch /readyz");
    assert_eq!(ready_resp.status(), 200);
    assert_eq!(ready_resp.text().unwrap(), r#"{"status":"ready"}"#);

    // Test 3: Root HTML landing page
    let root_resp = reqwest::blocking::get(format!("{base}/")).expect("Failed to fetch /");
    assert_eq!(root_resp.status(), 200);
    let html = root_resp.text().unwrap();
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("Headless Backend Mode"));

    // Cleanup: explicitly dropping the handles here ends the test scope.
    // The worker thread exits when its job_rx sender (owned by the server
    // thread) is dropped. The server thread is detached and will be killed
    // when the test binary exits.
}
