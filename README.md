# AISPH - Asynchronous Interactive Singleton Process Handler

AISPH is a Rust library that provides a convenient way to start, interact with, and manage external processes asynchronously. It allows bidirectional communication with processes, making it ideal for applications that need to control interactive command-line programs.

## Features

- **Asynchronous Process Management**: Start and manage external processes asynchronously using Tokio
- **Bidirectional Communication**: Send input to and receive output from running processes
- **Process Lifecycle Management**: Check if processes are running and terminate them when needed
- **Cross-Platform Support**: Works on both Windows and Linux
- **Singleton Pattern**: Ensures only one instance of a process is running at a time
- **Error Handling**: Comprehensive error handling using the `anyhow` crate

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
aisph = "0.1.0"
```

## Usage

### Basic Example

```rust
use aisph::AsynchronousInteractiveProcess;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Start a new process
    let pid = AsynchronousInteractiveProcess::new("path/to/executable")
        .with_argument("--some-flag")
        .start()
        .await?;

    // Get a handle to the process
    let process = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
        .expect("Process not found");

    // Send input to the process
    process.send_input("some command").await?;

    // Receive output from the process
    if let Some(output) = process.receive_output().await {
        println!("Process output: {}", output);
    }

    // Check if the process is still running
    if process.is_process_running().await {
        // Kill the process
        process.kill().await?;
    }

    Ok(())
}
```

### Working with Long-Running Processes

For long-running processes, you can spawn a separate task to handle the communication:

```rust
use aisph::AsynchronousInteractiveProcess;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pid = AsynchronousInteractiveProcess::new("path/to/server")
        .start()
        .await?;

    let reader_task = tokio::spawn(async move {
        let process = AsynchronousInteractiveProcess::get_process_by_pid(pid).await
            .expect("Process not found");

        while process.is_process_running().await {
            // Send periodic commands
            process.send_input("status").await?;

            // Process output
            if let Some(output) = process.receive_output().await {
                // Handle output
                println!("Server: {}", output);
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

### Working with Command-Line Arguments

AISPH provides two methods for setting command-line arguments for your processes:

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

A handle for interacting with a running process.

- `receive_output(&self) -> Option<String>`: Receive output from the process
- `send_input(&self, input: impl Into<String>) -> Result<()>`: Send input to the process
- `is_process_running(&self) -> bool`: Check if the process is running
- `kill(&self) -> Result<()>`: Terminate the process

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Author

Drew Chase
