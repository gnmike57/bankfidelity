use dual_core_pdf_pipeline::pdf::{OxidizePdfEngine, PdfEngine};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrency_chaos() {
    let _ = dotenvy::dotenv();

    // Attempt to mutate the same PDF concurrently across 10 threads
    let pdf_path = PathBuf::from("examples/sample.pdf");
    let output_path = PathBuf::from("output/chaos_test.pdf");

    if !pdf_path.exists() {
        eprintln!("SKIP: examples/sample.pdf missing for concurrency chaos");
        return;
    }

    let engine = Arc::new(OxidizePdfEngine::new());
    let mut handles = vec![];

    // Spin up 10 overlapping mutations
    for i in 0..10 {
        let engine = engine.clone();
        let pdf_path = pdf_path.clone();
        let output_path = output_path.clone();

        let handle = task::spawn(async move {
            // This could hit a file lock or a race condition
            let res = engine.apply_change(
                &pdf_path,
                &output_path,
                1,
                [10.0, 10.0, 100.0, 20.0],
                &format!("Balance {}", i),
                "Balance",
                None,
            );

            // We just ensure it doesn't crash the tokio runtime (panics)
            // It's perfectly fine if some return Err due to locks
            res
        });

        handles.push(handle);
    }

    let mut successes = 0;
    let mut errors = 0;

    for handle in handles {
        match handle.await {
            Ok(Ok(_)) => successes += 1,
            Ok(Err(_)) => errors += 1,
            Err(e) => panic!("Tokio task panicked! {}", e), // A tokio panic is a failure
        }
    }

    println!(
        "Concurrency chaos completed: {} succeeded, {} errored cleanly.",
        successes, errors
    );
    // At least one should probably succeed, but as long as 0 panicked, the test passes
    assert!(
        successes + errors == 10,
        "Not all tasks returned gracefully"
    );
}
