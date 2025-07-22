use log::{debug, error, info, LevelFilter};
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
        .process_exit_callback(|exit_code| {
            info!("[SERVER]: Process exited with code {}", exit_code);
        })
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
                    error!("[SERVER ERROR]: {}", e);
                }
            }

            if i > 10 {
                // Safely shutdown the process after 10 iterations
                process.send_input("exit").await.unwrap();
            }

            // Read output from the process
            let line = process.receive_output().await;
            if let Ok(Some(output)) = line {
                debug!("[SERVER]: {}", output);
                i += 1;
            }
        }
    });

    reader_thread.await?;

    Ok(())
}
