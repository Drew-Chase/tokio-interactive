use log::{debug, error, info, LevelFilter};
use tokio_interactive::AsynchronousInteractiveProcess;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::env_logger::builder().format_timestamp(None).filter_level(LevelFilter::Debug).init();
    info!("Broadcast system example - multiple receivers from one process");
    
    let pid = AsynchronousInteractiveProcess::new("cargo")
        .with_argument("run")
        .with_argument("--release")
        .with_working_directory("./examples/test_server")
        .process_exit_callback(|exit_code| {
            info!("[SERVER]: Process exited with code {}", exit_code);
        })
        .start()
        .await?;

    // Create multiple receivers for the same process - this demonstrates the broadcast system
    let mut receiver1 = AsynchronousInteractiveProcess::get_process_by_pid(pid).await.expect("Process not found");
    let mut receiver2 = AsynchronousInteractiveProcess::get_process_by_pid(pid).await.expect("Process not found");
    let mut receiver3 = AsynchronousInteractiveProcess::get_process_by_pid(pid).await.expect("Process not found");
    
    info!("Created 3 receivers for the same process - all will receive the same messages");

    // Spawn multiple reader threads - each will receive the same messages
    let reader1 = tokio::spawn(async move {
        let mut i = 0;
        while receiver1.is_process_running().await && i < 15 {
            if let Ok(Some(output)) = receiver1.receive_output().await {
                debug!("[RECEIVER 1]: {}", output);
                i += 1;
            }
        }
        info!("Receiver 1 finished");
    });

    let reader2 = tokio::spawn(async move {
        let mut i = 0;
        while receiver2.is_process_running().await && i < 15 {
            if let Ok(Some(output)) = receiver2.receive_output().await {
                debug!("[RECEIVER 2]: {}", output);
                i += 1;
            }
        }
        info!("Receiver 2 finished");
    });

    let reader3 = tokio::spawn(async move {
        let mut i = 0;
        while receiver3.is_process_running().await && i < 15 {
            if let Ok(Some(output)) = receiver3.receive_output().await {
                debug!("[RECEIVER 3]: {}", output);
                i += 1;
            }
        }
        info!("Receiver 3 finished");
    });

    // Control thread that sends input to the process
    let control_thread = tokio::spawn(async move {
        let mut control_handle = AsynchronousInteractiveProcess::get_process_by_pid(pid).await.expect("Process not found");
        let mut i = 0;
        
        while control_handle.is_process_running().await {
            // Send input to the process every few iterations
            if i % 5 == 0 {
                if let Err(e) = control_handle.send_input("echo this").await {
                    error!("[CONTROL ERROR]: {}", e);
                }
            }

            if i > 10 {
                // Safely shutdown the process after 10 iterations
                control_handle.send_input("exit").await.unwrap();
                break;
            }
            
            i += 1;
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    });

    // Wait for all threads to complete
    let _ = tokio::join!(reader1, reader2, reader3, control_thread);

    Ok(())
}
