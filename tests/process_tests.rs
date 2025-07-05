use anyhow::Result;
use std::time::Duration;
use tokio_interactive::AsynchronousInteractiveProcess;

#[tokio::test]
async fn test_process_creation() -> Result<()> {
    // Create a new process configuration
    let process = AsynchronousInteractiveProcess::new("echo")
        .with_argument("Hello, World!");

    // Verify the configuration is correct
    assert_eq!(process.filename, "echo");
    assert_eq!(process.arguments, vec!["Hello, World!"]);
    assert!(process.pid.is_none());

    Ok(())
}

#[tokio::test]
async fn test_with_arguments() -> Result<()> {
    // Test with_arguments method
    let process = AsynchronousInteractiveProcess::new("test")
        .with_arguments(vec!["arg1", "arg2", "arg3"]);

    assert_eq!(process.arguments, vec!["arg1", "arg2", "arg3"]);

    // Test that with_arguments replaces existing arguments
    let process = process.with_arguments(vec!["new1", "new2"]);
    assert_eq!(process.arguments, vec!["new1", "new2"]);

    Ok(())
}

#[tokio::test]
async fn test_with_working_directory() -> Result<()> {
    let process = AsynchronousInteractiveProcess::new("test")
        .with_working_directory("./some/path");

    assert_eq!(process.working_directory.to_str().unwrap(), "./some/path");

    Ok(())
}

#[cfg(target_os = "windows")]
#[tokio::test]
async fn test_process_start_and_communication_windows() -> Result<()> {
    // This test uses the Windows command prompt to echo back input
    let mut process = AsynchronousInteractiveProcess::new("cmd.exe")
        .with_arguments(vec!["/C", "echo Hello from Windows & pause"]);

    // Start the process
    let pid = process.start().await?;
    assert!(pid > 0);

    // Get a handle to the process
    let handle = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
        .expect("Process not found");

    // Wait for output
    let output = handle.receive_output_with_timeout(Duration::from_secs(2)).await?;
    assert!(output.is_some());
    let output_str = output.unwrap();
    assert!(output_str.contains("Hello from Windows"));

    // Terminate the process
    handle.kill().await?;

    // Verify the process is no longer running
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!handle.is_process_running().await);

    Ok(())
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn test_process_start_and_communication_linux() -> Result<()> {
    // This test uses the echo command on Linux
    let mut process = AsynchronousInteractiveProcess::new("bash")
        .with_arguments(vec!["-c", "echo Hello from Linux; sleep 1"]);

    // Start the process
    let pid = process.start().await?;
    assert!(pid > 0);

    // Get a handle to the process
    let handle = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
        .expect("Process not found");

    // Wait for output
    let output = handle.receive_output_with_timeout(Duration::from_secs(2)).await?;
    assert!(output.is_some());
    let output_str = output.unwrap();
    assert!(output_str.contains("Hello from Linux"));

    // Wait for the process to exit naturally
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(!handle.is_process_running().await);

    Ok(())
}

#[tokio::test]
async fn test_process_shutdown() -> Result<()> {
    // Create a process that sleeps for 10 seconds
    #[cfg(target_os = "windows")]
    let mut process = AsynchronousInteractiveProcess::new("timeout")
        .with_arguments(vec!["10"]);

    #[cfg(target_os = "linux")]
    let mut process = AsynchronousInteractiveProcess::new("sleep")
        .with_arguments(vec!["10"]);

    // Start the process
    let pid = process.start().await?;

    // Get a handle to the process
    let handle = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
        .expect("Process not found");

    // Verify the process is running
    assert!(handle.is_process_running().await);

    // Attempt graceful shutdown with a short timeout
    handle.shutdown(Duration::from_secs(1)).await?;

    // Verify the process is no longer running
    assert!(!handle.is_process_running().await);

    Ok(())
}

#[cfg(target_os = "windows")]
#[tokio::test]
async fn test_windows_graceful_shutdown() -> Result<()> {
    // Create a Windows process that responds to Ctrl+C or "exit" commands
    // We'll use a cmd.exe process that runs a batch file-like command that:
    // 1. Echoes a message
    // 2. Waits for input
    // 3. Exits gracefully when receiving "exit" command
    let mut process = AsynchronousInteractiveProcess::new("cmd.exe")
        .with_arguments(vec!["/K", "echo Windows test process started. Type 'exit' to quit."]);

    // Start the process
    let pid = process.start().await?;

    // Get a handle to the process
    let handle = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
        .expect("Process not found");

    // Wait for the initial output
    let output = handle.receive_output_with_timeout(Duration::from_secs(2)).await?;
    assert!(output.is_some());
    let output_str = output.unwrap();
    assert!(output_str.contains("Windows test process started"));

    // Verify the process is running
    assert!(handle.is_process_running().await);

    // Get the current time to measure how long shutdown takes
    let start_time = std::time::Instant::now();

    // Attempt graceful shutdown with a reasonable timeout
    // This will internally call graceful_shutdown() first, then fall back to kill() if needed
    let result = handle.shutdown(Duration::from_secs(3)).await;
    assert!(result.is_ok(), "Shutdown failed: {:?}", result);

    // Verify the process is no longer running
    assert!(!handle.is_process_running().await, "Process should have exited after shutdown");

    // Check if shutdown happened quickly (indicating graceful shutdown worked)
    // If it took close to the full timeout, it likely fell back to forceful termination
    let shutdown_duration = start_time.elapsed();
    println!("Shutdown took {:?}", shutdown_duration);

    // The shutdown should be relatively quick if graceful shutdown works
    // But we don't assert on this as it might be flaky in CI environments

    Ok(())
}

#[tokio::test]
async fn test_receive_output_timeout() -> Result<()> {
    // Create a process that outputs something after a delay
    #[cfg(target_os = "windows")]
    let mut process = AsynchronousInteractiveProcess::new("cmd.exe")
        .with_arguments(vec!["/C", "timeout 2 & echo Delayed output"]);

    #[cfg(target_os = "linux")]
    let mut process = AsynchronousInteractiveProcess::new("bash")
        .with_arguments(vec!["-c", "sleep 2; echo Delayed output"]);

    // Start the process
    let pid = process.start().await?;

    // Get a handle to the process
    let handle = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
        .expect("Process not found");

    // Try to receive output with a short timeout (should return None)
    let output = handle.receive_output_with_timeout(Duration::from_millis(100)).await?;
    assert!(output.is_none());

    // Try to receive output with a longer timeout (should return the output)
    let output = handle.receive_output_with_timeout(Duration::from_secs(3)).await?;
    assert!(output.is_some());
    let output_str = output.unwrap();
    assert!(output_str.contains("Delayed output"));

    // Clean up
    handle.kill().await?;

    Ok(())
}
