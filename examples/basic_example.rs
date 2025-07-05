use log::{LevelFilter, info};
use std::time::Duration;
use tokio_interactive::AsynchronousInteractiveProcess;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::env_logger::builder().format_timestamp(None).filter_level(LevelFilter::Debug).init();
    info!("test server example");
    let pid = AsynchronousInteractiveProcess::new("cargo")
        .with_argument("run")
        .with_argument("--release")
        .with_working_directory("./examples/test_server")
        .start()
        .await?;

    // From another thread...
    let reader_thread = tokio::spawn(async move {
        let process = AsynchronousInteractiveProcess::get_process_by_pid(pid).await.expect("Process not found");
        let mut i = 0;
        while process.is_process_running().await {
            // Send input to the process every 5 iterations
            if i % 5 == 0 {
                if let Err(e) = process.send_input("echo this").await {
                    eprintln!("[SERVER ERROR]: {}", e);
                }
            }

            if i > 10 {
                // Safely shutdown the process after 10 iterations
                if let Err(e) = process.shutdown(Duration::from_secs(5)).await {
                    eprintln!("[SERVER ERROR]: {}", e);
                } else {
                    println!("[SERVER]: Process killed after 10 iterations.");
                    break;
                }
            }

            // Read output from the process
            let line = process.receive_output().await;
            if let Ok(Some(output)) = line {
                println!("[SERVER]: {}", output);
                i += 1;
            }
        }
    });

    reader_thread.await?;

    Ok(())
}
