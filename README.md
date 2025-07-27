# tokio-interactive

tokio-interactive is a Rust library that provides a convenient way to start, interact with, and manage external processes asynchronously. It features a **broadcast system** that allows multiple receivers to listen to the same process output simultaneously, making it ideal for applications that need to control interactive command-line programs with multiple monitoring components.

## Features

- **Asynchronous Process Management**: Start and manage external processes asynchronously using Tokio
- **Broadcast System**: Multiple receivers can listen to the same process output simultaneously
- **Bidirectional Communication**: Send input to and receive output from running processes
- **Process Lifecycle Management**: Check if processes are running and terminate them when needed
- **Cross-Platform Support**: Works on both Windows and Linux
- **Singleton Pattern**: Ensures only one instance of a process is running at a time
- **Lag Handling**: Built-in handling for receivers that fall behind (messages may be dropped)
- **Error Handling**: Comprehensive error handling using the `anyhow` crate

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
tokio-interactive = "0.2.0"
```

## Compatibility

- **Rust Version**: This library requires Rust 2021 edition or later.
- **Tokio Version**: Compatible with Tokio 1.46.1 or later.
- **Platform Support**: Windows and Linux are fully supported. Other platforms may work but are not officially supported.

### Dependencies

- **tokio**: For asynchronous runtime and process management
- **anyhow**: For error handling
- **log**: For logging
- **serde**: For serialization/deserialization support
- **winapi**: For Windows-specific process management (Windows only)
- **libc**: For Linux-specific process management (Linux only)

## Usage

### Basic Example

```rust
use tokio_interactive::AsynchronousInteractiveProcess;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Start a new process
    let pid = AsynchronousInteractiveProcess::new("path/to/executable")
        .with_argument("--some-flag")
        .start()
        .await?;

    // Get a handle to the process
    let mut process = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
        .expect("Process not found");

    // Send input to the process
    process.send_input("some command").await?;

    // Receive output from the process (note: requires &mut self)
    match process.receive_output().await {
        Ok(Some(output)) => println!("Process output: {}", output),
        Ok(None) => println!("No output available"),
        Err(e) => eprintln!("Error receiving output: {}", e),
    }

    // Check if the process is still running
    if process.is_process_running().await {
        // Kill the process
        process.kill().await?;
    }

    Ok(())
}
```

### Multiple Receivers Example

The broadcast system allows multiple receivers to listen to the same process output:

```rust
use tokio_interactive::AsynchronousInteractiveProcess;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Start a new process
    let pid = AsynchronousInteractiveProcess::new("path/to/server")
        .start()
        .await?;

    // Create multiple receivers for the same process
    let mut receiver1 = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
        .expect("Process not found");
    let mut receiver2 = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
        .expect("Process not found");
    let mut receiver3 = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
        .expect("Process not found");

    // Spawn tasks for each receiver - all will get the same messages
    let task1 = tokio::spawn(async move {
        while receiver1.is_process_running().await {
            if let Ok(Some(output)) = receiver1.receive_output().await {
                println!("[Receiver 1]: {}", output);
            }
        }
    });

    let task2 = tokio::spawn(async move {
        while receiver2.is_process_running().await {
            if let Ok(Some(output)) = receiver2.receive_output().await {
                println!("[Receiver 2]: {}", output);
            }
        }
    });

    let task3 = tokio::spawn(async move {
        while receiver3.is_process_running().await {
            if let Ok(Some(output)) = receiver3.receive_output().await {
                println!("[Receiver 3]: {}", output);
            }
        }
    });

    // Send some input to generate output
    let mut control_handle = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
        .expect("Process not found");
    control_handle.send_input("status").await?;

    // Wait for all tasks
    let _ = tokio::join!(task1, task2, task3);

    Ok(())
}
```

### Working with Long-Running Processes

For long-running processes, you can spawn a separate task to handle the communication:

```rust
use tokio_interactive::AsynchronousInteractiveProcess;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pid = AsynchronousInteractiveProcess::new("path/to/server")
        .start()
        .await?;

    let reader_task = tokio::spawn(async move {
        let mut process = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
            .expect("Process not found");

        while process.is_process_running().await {
            // Send periodic commands
            process.send_input("status").await?;

            // Process output (note: requires &mut self)
            match process.receive_output().await {
                Ok(Some(output)) => {
                    // Handle output
                    println!("Server: {}", output);
                },
                Ok(None) => {
                    // No output available
                },
                Err(e) => {
                    eprintln!("Error receiving output: {}", e);
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        Ok::<(), anyhow::Error>(())
    });

    // Wait for the reader task to complete
    reader_task.await??;

    Ok(())
}
```

### Exit Callback Example
You can set an exit callback to perform actions when the process exits:

```rust
use tokio_interactive::AsynchronousInteractiveProcess;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pid = AsynchronousInteractiveProcess::new("cargo")
        .with_argument("--version")
        .process_exit_callback(|exit_code| {
            println!("Process exited with code {}", exit_code);
        })
        .start()
        .await?;
    
    // Process will run and callback will be called when it exits
    Ok(())
}
```


### Working with Command-Line Arguments

tokio-interactive provides two methods for setting command-line arguments for your processes:

#### `with_argument`

The `with_argument` method adds a single argument to the existing set of arguments. Each call to `with_argument` appends to the existing arguments list.

For example, calling:
- `process.with_argument("--verbose")`
- Then `process.with_argument("--output=file.txt")`

Would result in the command: `my_program --verbose --output=file.txt`.

The method accepts any type that implements `Into<String>`, so you can pass:
- String literals: `process.with_argument("--config")`
- String objects: `process.with_argument(filename)` where `filename` is a `String`
- Numbers: `process.with_argument(42)` (adds "42" as an argument)
- Any custom type that implements `Into<String>`

#### `with_arguments`

The `with_arguments` method replaces all existing arguments with a new set. This is useful when you want to completely change the arguments rather than adding to them.

For example, if you first call:
- `process.with_argument("--verbose")`

And then call:
- `process.with_arguments(vec!["--quiet", "--log=error.log"])`

The final command would be: `my_program --quiet --log=error.log` (the `--verbose` argument is replaced).

Like `with_argument`, this method accepts any type that implements `Into<String>`, so you can use a vector with mixed types:
- String literals
- String objects
- Numbers
- Any custom type that implements `Into<String>`

#### Combining Both Methods

You can combine both methods in your code. For example:
1. Set initial arguments with `with_arguments(vec!["--mode=normal", "--quiet"])`
2. Add another argument with `with_argument("--input=data.txt")`

This would result in the command: `my_program --mode=normal --quiet --input=data.txt`.

## API Overview

### `AsynchronousInteractiveProcess`

The main struct for creating and managing interactive processes.

- `new(filename: impl Into<String>) -> Self`: Create a new process configuration
- `with_arguments(self, args: Vec<impl Into<String>>) -> Self`: Replace all arguments with a new set
- `with_argument(self, arg: impl Into<String>) -> Self`: Add a single argument to the existing set
- `with_working_directory(self, dir: impl Into<PathBuf>) -> Self`: Set the working directory
- `start(&mut self) -> Result<u32>`: Start the process and return its PID
- `get_process_by_pid(pid: u32) -> Option<ProcessHandle>`: Get a handle to a running process
- `is_process_running(&self) -> bool`: Check if the process is running

### `ProcessHandle`

A handle for interacting with a running process. Each ProcessHandle contains its own broadcast receiver, allowing multiple handles to receive the same process output simultaneously.

**Note**: `ProcessHandle` does not implement `Clone`. To create multiple receivers for the same process, call `get_process_by_pid()` multiple times.

- `receive_output(&mut self) -> Result<Option<String>>`: Receive output from the process with default timeout
- `receive_output_with_timeout(&mut self, timeout: Duration) -> Result<Option<String>>`: Receive output with custom timeout
- `send_input(&self, input: impl Into<String>) -> Result<()>`: Send input to the process
- `is_process_running(&self) -> bool`: Check if the process is running
- `shutdown(&self, timeout: Duration) -> Result<()>`: Gracefully shut down the process with timeout
- `kill(&self) -> Result<()>`: Forcefully terminate the process

### Broadcast System

The library uses a broadcast system that allows multiple receivers to listen to the same process output:

- **Multiple Receivers**: Each call to `get_process_by_pid()` creates a new receiver that subscribes to the same broadcast channel
- **Simultaneous Delivery**: All receivers get the same messages at the same time
- **Lag Handling**: If a receiver falls behind, it may miss messages (lagged messages are dropped)
- **Independent Operation**: Each receiver operates independently and can be used in separate tasks

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Author

Drew Chase
