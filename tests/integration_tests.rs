use anyhow::Result;
use std::time::Duration;
use tokio_interactive::AsynchronousInteractiveProcess;

/// This test simulates a scenario where one process generates output
/// and another process consumes that output.
#[tokio::test]
async fn test_process_interaction() -> Result<()> {
    // Create and start the first process (producer)
    #[cfg(target_os = "windows")]
    let mut producer = AsynchronousInteractiveProcess::new("cmd.exe")
        .with_arguments(vec!["/C", "echo Producer started & timeout 1 & echo Data1 & timeout 1 & echo Data2"]);

    #[cfg(target_os = "linux")]
    let mut producer = AsynchronousInteractiveProcess::new("bash")
        .with_arguments(vec!["-c", "echo Producer started; sleep 1; echo Data1; sleep 1; echo Data2"]);

    let producer_pid = producer.start().await?;
    let producer_handle = AsynchronousInteractiveProcess::get_process_by_pid(producer_pid).await
        .expect("Producer process not found");

    // Create and start the second process (consumer)
    #[cfg(target_os = "windows")]
    let mut consumer = AsynchronousInteractiveProcess::new("cmd.exe")
        .with_arguments(vec!["/C", "echo Consumer started & timeout 3"]);

    #[cfg(target_os = "linux")]
    let mut consumer = AsynchronousInteractiveProcess::new("bash")
        .with_arguments(vec!["-c", "echo Consumer started; sleep 3"]);

    let consumer_pid = consumer.start().await?;
    let consumer_handle = AsynchronousInteractiveProcess::get_process_by_pid(consumer_pid).await
        .expect("Consumer process not found");

    // Verify both processes are running
    assert!(producer_handle.is_process_running().await);
    assert!(consumer_handle.is_process_running().await);

    // Collect output from producer and send to consumer
    let mut collected_data = Vec::new();

    // Wait for initial output
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Collect data for 3 seconds
    let start_time = std::time::Instant::now();
    while start_time.elapsed() < Duration::from_secs(3) {
        if let Ok(Some(output)) = producer_handle.receive_output_with_timeout(Duration::from_millis(100)).await {
            println!("Producer: {}", output);
            collected_data.push(output.clone());

            // Send the data to the consumer
            consumer_handle.send_input(format!("Received: {}", output)).await?;
        }

        // Check consumer output
        if let Ok(Some(output)) = consumer_handle.receive_output_with_timeout(Duration::from_millis(100)).await {
            println!("Consumer: {}", output);
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Verify we collected the expected data
    assert!(collected_data.len() >= 2);
    assert!(collected_data.iter().any(|s| s.contains("Producer started")));
    assert!(collected_data.iter().any(|s| s.contains("Data1")));

    // Clean up
    producer_handle.kill().await?;
    consumer_handle.kill().await?;

    // Verify both processes are no longer running
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!producer_handle.is_process_running().await);
    assert!(!consumer_handle.is_process_running().await);

    Ok(())
}

/// This test verifies that multiple processes can be managed simultaneously
#[tokio::test]
async fn test_multiple_processes() -> Result<()> {
    // Start multiple processes
    let mut processes = Vec::new();
    let mut handles = Vec::new();

    for i in 0..3 {
        #[cfg(target_os = "windows")]
        let mut process = AsynchronousInteractiveProcess::new("cmd.exe")
            .with_arguments(vec!["/C", &format!("echo Process {} started & timeout {}", i, i + 1)]);

        #[cfg(target_os = "linux")]
        let mut process = AsynchronousInteractiveProcess::new("bash")
            .with_arguments(vec!["-c", &format!("echo Process {} started; sleep {}", i, i + 1)]);

        let pid = process.start().await?;
        processes.push(process);

        let handle = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
            .expect(&format!("Process {} not found", i));
        handles.push(handle);
    }

    // Verify all processes are running
    for (i, handle) in handles.iter().enumerate() {
        assert!(handle.is_process_running().await, "Process {} should be running", i);
    }

    // Wait for output from each process
    for (i, handle) in handles.iter().enumerate() {
        let output = handle.receive_output_with_timeout(Duration::from_secs(2)).await?;
        assert!(output.is_some(), "Process {} should produce output", i);
        let output_str = output.unwrap();
        assert!(output_str.contains(&format!("Process {} started", i)), 
                "Output should contain process start message");
    }

    // Wait for processes to exit naturally
    tokio::time::sleep(Duration::from_secs(4)).await;

    // Verify all processes have exited
    for (i, handle) in handles.iter().enumerate() {
        assert!(!handle.is_process_running().await, "Process {} should have exited", i);
    }

    Ok(())
}

/// This test verifies error handling when trying to interact with non-existent processes
#[tokio::test]
async fn test_error_handling() -> Result<()> {
    // Try to get a handle to a non-existent process
    let non_existent_pid = 999999; // Assuming this PID doesn't exist
    let handle = AsynchronousInteractiveProcess::get_process_by_pid(non_existent_pid).await;
    assert!(handle.is_none(), "Should not find a non-existent process");

    // Test error handling with a process that we start and then kill
    // This simulates a process that has exited unexpectedly

    // Start a short-lived process
    #[cfg(target_os = "windows")]
    let mut process = AsynchronousInteractiveProcess::new("cmd.exe")
        .with_arguments(vec!["/C", "echo Test process"]);

    #[cfg(target_os = "linux")]
    let mut process = AsynchronousInteractiveProcess::new("bash")
        .with_arguments(vec!["-c", "echo Test process"]);

    let pid = process.start().await?;
    let handle = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
        .expect("Process not found");

    // Wait for the process to exit naturally
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Now try to interact with the exited process
    // is_process_running should return false
    assert!(!handle.is_process_running().await, 
            "is_process_running should return false for exited process");

    // receive_output should return an error or None
    let result = handle.receive_output().await;
    if let Ok(output) = &result {
        assert!(output.is_none(), "receive_output should return None for exited process");
    }

    // send_input should return an error
    let result = handle.send_input("test").await;
    assert!(result.is_err(), "send_input should return an error for exited process");

    // kill should return an error (or succeed but the process is already gone)
    let _ = handle.kill().await;

    Ok(())
}
