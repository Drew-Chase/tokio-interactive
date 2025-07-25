#![doc = include_str!("../README.md")]
use anyhow::{anyhow, Result};
use log::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fmt::{Debug, Formatter};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::Mutex;
// Change this line

/// A static, lazily-initialized process pool used to manage asynchronous interactive processes.
///
/// This global variable utilizes the `OnceLock` to ensure it is initialized only once during
/// the program's lifetime. It holds an `Arc<Mutex<HashMap<u32, AsynchronousInteractiveProcess>>>`
/// to allow safe concurrent access and modification of the process pool.
///
/// - `OnceLock`: Provides thread-safe, one-time initialization of the process pool.
/// - `Arc`: Ensures the `Mutex<HashMap<...>>` can be shared across threads safely.
/// - `Mutex`: Protects the `HashMap` from concurrent modification, ensuring thread safety.
/// - `HashMap<u32, AsynchronousInteractiveProcess>`: Maps process IDs (`u32`) to
///   corresponding `AsynchronousInteractiveProcess` instances.
///
/// Usage:
///
/// The process pool is intended to store and manage asynchronous interactive processes by their IDs.
/// Access needs to ensure proper locking with the `Mutex` guard to guarantee thread safety.
///
/// Example initialization:
/// ```rust
/// PROCESS_POOL.get_or_init(|| Arc::new(Mutex::new(HashMap::new())));
/// ```
///
/// Example access:
/// ```rust
/// if let Some(pool) = PROCESS_POOL.get() {
///     let mut guard = pool.lock().unwrap();
///     guard.insert(process_id, new_process);
/// }
/// ```
/// Ensure that any operations on the pool respect the locking mechanism provided by the `Mutex`.
///
/// Once initialized, `PROCESS_POOL` cannot be re-initialized or re-assigned.
static PROCESS_POOL: OnceLock<Arc<Mutex<HashMap<u32, AsynchronousInteractiveProcess>>>> = OnceLock::new();

/// Represents a handle to a process identified by its process ID (PID).
///
/// The `ProcessHandle` struct is used to encapsulate the process identifier (PID)
/// of a running process. It is designed to be lightweight and can be cloned
/// or debugged as required.
///
/// # Fields
/// - `pid`: A `u32` value that represents the process ID of the targeted process.
///
/// # Traits
/// - `Debug`: Allows instances of `ProcessHandle` to be formatted using the `{:?}` formatter
///   for debugging purposes.
/// - `Clone`: Allows the `ProcessHandle` to be cloned, creating a new instance with the
///   same `pid`.
///
/// # Example
/// ```
/// let handle = ProcessHandle { pid: 1234 };
/// println!("{:?}", handle); // Outputs: ProcessHandle { pid: 1234 }
/// let cloned_handle = handle.clone();
/// println!("{:?}", cloned_handle); // Outputs: ProcessHandle { pid: 1234 }
/// ```
#[derive(Debug, Clone)]
pub struct ProcessHandle {
    pid: u32,
}

/// `AsynchronousInteractiveProcess` is a structure that represents a non-blocking,
/// interactive process. It provides metadata and mechanisms to interact with a
/// process asynchronously using sender and receiver channels.
///
/// # Fields
///
/// * `pid` - An optional process ID (`pid`) of the interactive process. If the process
///   is not yet started, this field will remain `None`.
///
/// * `filename` - The name of the executable file that represents the interactive process.
///   This is required to identify and initiate the process.
///
/// * `arguments` - A vector containing the arguments to pass to the executable file
///   when launching the process.
///
/// * `working_directory` - The directory where the interactive process will be executed.
///   This is represented as a `PathBuf`.
///
/// * `sender` - An optional `Sender` channel used to send messages or data to the
///   interactive process. This field is skipped during serialization and deserialization
///   because it holds runtime-related data.
///
/// * `receiver` - An optional `Receiver` channel used to receive messages or data
///   from the interactive process. This field is also skipped during serialization
///   and deserialization for the same reasons as `sender`.
///
/// * `input_queue` - A deque (double-ended queue) that maintains an in-memory buffer
///   of strings representing input for the interactive process. This field is skipped
///   during serialization as it only contains transient runtime-related data.
///
/// # Trait Implementations
///
/// - `Debug`: Allows instances of this struct to be formatted and logged for debugging purposes.
/// - `Serialize`: Makes the struct serializable, excluding fields marked with `#[serde(skip)]`.
/// - `Deserialize`: Allows deserialization to create instances of this struct from serialized data.
/// - `Default`: Provides a default implementation where optional fields are set to `None`,
///   collections are empty, and `filename` is an empty string.
///
/// # Example
/// ```rust
/// use std::path::PathBuf;
/// use std::collections::VecDeque;
/// use serde::{Serialize, Deserialize};
/// use crossbeam_channel::{Sender, Receiver};
///
/// let process = AsynchronousInteractiveProcess {
///     pid: None,
///     filename: "my_program".to_string(),
///     arguments: vec!["--help".to_string()],
///     working_directory: PathBuf::from("/path/to/dir"),
///     sender: None,
///     receiver: None,
///     input_queue: VecDeque::new(),
/// };
///
/// println!("{:?}", process);
/// ```
///
/// This structure is ideal for scenarios where processes need to be controlled
/// asynchronously with multiple input or output channels.
#[derive(Serialize, Deserialize, Default)]
pub struct AsynchronousInteractiveProcess {
    pub pid: Option<u32>,
    pub filename: String,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
    #[serde(skip)]
    sender: Option<Sender<String>>,
    #[serde(skip)]
    receiver: Option<Receiver<String>>,
    #[serde(skip)]
    input_queue: VecDeque<String>,
    #[serde(skip)]
    exit_callback: Option<Arc<dyn Fn(i32) + Send + Sync>>,
}

impl Debug for AsynchronousInteractiveProcess {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AsynchronousInteractiveProcess {{ pid: {:?}, filename: {:?}, arguments: {:?}, working_directory: {:?} }}",
            self.pid, self.filename, self.arguments, self.working_directory
        )
    }
}

impl ProcessHandle {
    /// Receives an output message asynchronously from the process associated with the instance's `pid`
    /// if available.
    ///
    /// This function attempts to fetch a message by accessing the process pool and checking for a
    /// message in the associated process's receiver.
    ///
    /// The process follows these steps:
    /// 1. Check if the global process pool (`PROCESS_POOL`) is initialized.
    /// 2. Attempt to acquire the lock on the pool asynchronously.
    /// 3. Look for the process associated with `self.pid`.
    /// 4. If the process has a receiver channel, attempt to get a message using `try_recv`.
    /// 5. If a message is found, return it as `Ok(Some(String))`.
    /// 6. If no message is received, retry up to `MAX_RETRIES` times, with a delay of `RETRY_DELAY_MS`
    ///    milliseconds between each retry.
    ///
    /// If no message is received after all retries, the function returns `Ok(None)`.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(String))` - If a message is successfully received from the receiver channel.
    /// * `Ok(None)` - If no message is received after all retries.
    /// * `Err(Error)` - If an error occurs during the process, such as:
    ///   - The process pool is not initialized
    ///   - The process with the specified PID is not found
    ///   - The receiver channel is not available
    ///   - The receiver channel is disconnected
    ///
    /// # Behavior
    ///
    /// - This function uses `tokio::time::sleep` for introducing delays between retries and works
    ///   asynchronously.
    /// - The function uses constants `MAX_RETRIES` (10) and `RETRY_DELAY_MS` (10) to control the
    ///   retry behavior.
    /// - The function properly propagates errors that occur during the process.
    ///
    /// # Example
    ///
    /// ```rust
    /// match instance.receive_output().await {
    ///     Ok(Some(output)) => println!("Received output: {}", output),
    ///     Ok(None) => println!("No output received."),
    ///     Err(e) => eprintln!("Error receiving output: {}", e),
    /// }
    /// ```
    ///
    /// # Note
    ///
    /// - The function assumes the existence of a global process pool (`PROCESS_POOL`) which is safe
    ///   to access concurrently using mutex-based locking.
    /// - The function makes use of `tokio::sync::Mutex` to handle concurrent executions across
    ///   asynchronous tasks.
    /// Default timeout for receive_output in milliseconds
    const DEFAULT_TIMEOUT_MS: u64 = 100;

    /// Helper function to try receiving a message
    async fn try_receive_message(
        pid: u32,
        pool: &mut tokio::sync::MutexGuard<'_, HashMap<u32, AsynchronousInteractiveProcess>>,
    ) -> Result<Option<String>> {
        if let Some(process) = pool.get_mut(&pid) {
            if let Some(receiver) = &mut process.receiver {
                match receiver.try_recv() {
                    Ok(msg) => return Ok(Some(msg)),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        return Err(anyhow!("Channel disconnected for process {}", pid));
                    }
                }
            } else {
                return Err(anyhow!("Receiver not available for process {}", pid));
            }
        } else {
            return Err(anyhow!("Process {} not found", pid));
        }
        Ok(None)
    }

    pub async fn receive_output(&self) -> Result<Option<String>> {
        // Use the default timeout
        self.receive_output_with_timeout(std::time::Duration::from_millis(Self::DEFAULT_TIMEOUT_MS)).await
    }

    /// Receives an output message asynchronously from the process associated with the instance's `pid`
    /// if available, with a specified timeout.
    ///
    /// This function is similar to `receive_output()` but allows specifying a custom timeout duration
    /// instead of using the default timeout.
    ///
    /// # Arguments
    ///
    /// * `timeout` - A `std::time::Duration` specifying how long to wait for a message before giving up.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(String))` - If a message is successfully received from the receiver channel.
    /// * `Ok(None)` - If no message is received before the timeout expires.
    /// * `Err(Error)` - If an error occurs during the process, such as:
    ///   - The process pool is not initialized
    ///   - The process with the specified PID is not found
    ///   - The receiver channel is not available
    ///   - The receiver channel is disconnected
    ///
    /// # Example
    ///
    /// ```rust
    /// // Wait for up to 5 seconds for a message
    /// match instance.receive_output_with_timeout(std::time::Duration::from_secs(5)).await {
    ///     Ok(Some(output)) => println!("Received output: {}", output),
    ///     Ok(None) => println!("No output received within timeout."),
    ///     Err(e) => eprintln!("Error receiving output: {}", e),
    /// }
    /// ```
    pub async fn receive_output_with_timeout(&self, timeout: std::time::Duration) -> Result<Option<String>> {
        // Define constants for retry parameters
        const RETRY_DELAY_MS: u64 = 10;

        if let Some(process_pool) = PROCESS_POOL.get() {
            // Try to receive a message immediately
            {
                let mut pool = process_pool.lock().await;
                if let Some(msg) = Self::try_receive_message(self.pid, &mut pool).await? {
                    return Ok(Some(msg));
                }
            }

            // If no message is available, retry with delay until timeout is reached
            let start_time = std::time::Instant::now();
            while start_time.elapsed() < timeout {
                tokio::time::sleep(tokio::time::Duration::from_millis(RETRY_DELAY_MS)).await;

                let mut pool = process_pool.lock().await;
                if let Some(msg) = Self::try_receive_message(self.pid, &mut pool).await? {
                    return Ok(Some(msg));
                }
            }

            // No message after timeout
            return Ok(None);
        }

        Err(anyhow!("Process pool not initialized"))
    }

    /// Sends the provided input to a process associated with this instance's PID asynchronously.
    ///
    /// # Arguments
    ///
    /// * `input` - An input of any type that can be converted into a `String`. This is the data
    ///   that will be sent to the respective process's input queue.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the input was successfully sent or queued for sending.
    /// * `Err` - Returns an error in the following cases:
    ///     - If the process pool is not initialized.
    ///     - If the process associated with this PID is not found.
    ///     - If the process was not started or its sender channel is not available.
    ///     - If the sender channel is closed and input cannot be sent.
    ///
    /// # Details
    ///
    /// This function retrieves the process associated with the current instance's `pid`
    /// from a global process pool. If a valid process is found:
    /// - The input is sent via an asynchronous channel to the process.
    /// - If the channel is full, the input is queued for later sending.
    /// - If the channel is closed, an error is returned.
    ///
    /// The function also ensures thread safety by acquiring a lock on the process pool before attempting
    /// any operations related to the process.
    ///
    /// # Errors
    ///
    /// This function propagates several potential issues as errors:
    /// - If the process pool is uninitialized (`PROCESS_POOL.get()` returns `None`).
    /// - If the process associated with the PID is missing in the pool.
    /// - If the sender channel was never initialized or is unavailable.
    /// - If the sender channel is closed and no new messages can be sent.
    ///
    /// # Example
    ///
    /// ```rust
    /// use anyhow::Result;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<()> {
    ///     let manager = ProcessManager::new(1); // Assumes a struct is managing process with ID 1.
    ///     manager.send_input("Some input").await?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Note
    ///
    /// This function expects a global process pool (`PROCESS_POOL`) to be properly initialized before being called.
    /// Additionally, the associated process must have a valid `sender` channel to accept input.
    pub async fn send_input(&self, input: impl Into<String>) -> Result<()> {
        let input_str = input.into();
        if let Some(process_pool) = PROCESS_POOL.get() {
            let mut pool = process_pool.lock().await;
            if let Some(process) = pool.get_mut(&self.pid) {
                if let Some(sender) = &process.sender {
                    match sender.try_send(input_str.clone()) {
                        Ok(_) => Ok(()),
                        Err(e) => match e {
                            tokio::sync::mpsc::error::TrySendError::Full(_) => {
                                process.input_queue.push_back(input_str);
                                Ok(())
                            }
                            tokio::sync::mpsc::error::TrySendError::Closed(_) => Err(anyhow!("Failed to send input: channel closed")),
                        },
                    }
                } else {
                    Err(anyhow!("Process not started or sender not available"))
                }
            } else {
                Err(anyhow!("Process not found"))
            }
        } else {
            Err(anyhow!("Process pool not initialized"))
        }
    }

    /// Checks if the process associated with the current instance is running.
    ///
    /// This method attempts to determine if a process with the `pid` of the current instance
    /// exists in the global `PROCESS_POOL`.
    ///
    /// # Returns
    /// * `true` - If the process with the associated `pid` is currently present in the global process pool.
    /// * `false` - If the process is not found in the global process pool or if the pool is not initialized.
    ///
    /// # Async Behavior
    /// This method is asynchronous because it acquires a lock on the `PROCESS_POOL`.
    ///
    /// # Notes
    /// - The `PROCESS_POOL` must be initialized before calling this function.
    ///   If `PROCESS_POOL` is not set, the function will return `false`.
    /// - The `PROCESS_POOL` is expected to be a globally accessible, asynchronous, and thread-safe
    ///   data structure that tracks active processes.
    ///
    /// # Example
    /// ```rust
    /// let result = instance.is_process_running().await;
    /// if result {
    ///     println!("Process is running.");
    /// } else {
    ///     println!("Process is not running.");
    /// }
    /// ```
    pub async fn is_process_running(&self) -> bool {
        if let Some(process_pool) = PROCESS_POOL.get() {
            let pool = process_pool.lock().await;
            return pool.contains_key(&self.pid);
        }
        false
    }

    /// Asynchronously terminates a process identified by its `pid`.
    ///
    /// This method performs the following operations tailored to the target operating system:
    ///
    /// - **Windows**: Opens a handle to the process using its process ID (`pid`) and forcefully terminates it
    ///   using the `TerminateProcess` function from the WinAPI.
    /// - **Linux**: Utilizes the `kill` system call with the `SIGKILL` signal to forcefully terminate the process.
    ///
    /// ## Platform-specific Notes:
    ///
    /// - On **Windows**, the process is identified and terminated using the `OpenProcess` and `TerminateProcess`
    ///   functions from the WinAPI.
    /// - On **Linux**, the `kill` system call is used with the signal `SIGKILL` (9) to ensure the process is terminated.
    ///
    /// # Errors
    ///
    /// - Returns an error if the process termination fails (on Linux) due to system call errors or invalid process IDs.
    ///   On failure, the error contains details about the `pid` and the last OS error encountered.
    ///
    /// # Safety
    ///
    /// This method uses unsafe code blocks to interact with system APIs (`libc` on Linux, WinAPI on Windows). Ensure
    /// that the provided `pid` corresponds to a valid process, and consider the implications of forcefully
    /// terminating processes.
    ///
    /// # Example
    ///
    /// ```rust
    /// let process_manager = SomeProcessManager::new(pid); // Example struct containing the pid
    /// if let Err(e) = process_manager.kill().await {
    ///     eprintln!("Failed to terminate process: {}", e);
    /// }
    /// ```
    /// Attempts to gracefully shut down the process, falling back to forceful termination if needed.
    ///
    /// This method first tries to gracefully shut down the process by:
    /// - On Linux: Sending a SIGTERM signal
    /// - On Windows: Sending a WM_CLOSE message to the main window
    ///
    /// If the process doesn't exit within the specified timeout, it will forcefully terminate
    /// the process using the `kill()` method.
    ///
    /// # Arguments
    ///
    /// * `timeout` - A `std::time::Duration` specifying how long to wait for the process to exit
    ///   gracefully before forcefully terminating it.
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the process was successfully shut down (either gracefully or forcefully).
    /// * `Err(Error)` - If an error occurred during the shutdown process.
    ///
    /// # Example
    ///
    /// ```rust
    /// // Try to shut down gracefully, waiting up to 5 seconds before force killing
    /// if let Err(e) = process.shutdown(std::time::Duration::from_secs(5)).await {
    ///     eprintln!("Failed to shut down process: {}", e);
    /// }
    /// ```
    pub async fn shutdown(&self, timeout: std::time::Duration) -> Result<()> {
        // First, try to gracefully shut down the process
        let graceful_shutdown_result = self.graceful_shutdown().await;

        // If graceful shutdown failed or isn't implemented for this platform, log it but continue
        if let Err(e) = &graceful_shutdown_result {
            debug!("Graceful shutdown attempt failed: {}", e);
            return self.kill().await;
        }

        // Wait for the process to exit for the specified timeout
        let start_time = std::time::Instant::now();
        while start_time.elapsed() < timeout {
            if !self.is_process_running().await {
                return Ok(());
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // If we're here, the process didn't exit gracefully within the timeout
        // Fall back to forceful termination
        debug!("Process did not exit gracefully within timeout, forcing termination");
        self.kill().await
    }

    /// Attempts to gracefully shut down the process without forceful termination.
    ///
    /// This method is platform-specific:
    /// - On Linux: Sends a SIGTERM signal to request graceful termination
    /// - On Windows: First tries to send a Ctrl+C event to the process using GenerateConsoleCtrlEvent.
    ///   If that fails, it attempts to send an "exit" command to the process via stdin.
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the graceful shutdown signal was successfully sent.
    /// * `Err(Error)` - If an error occurred while sending the graceful shutdown signal.
    ///
    /// # Notes
    ///
    /// - A successful return doesn't guarantee the process will actually exit.
    ///   Use `shutdown()` with a timeout to ensure the process exits.
    /// - On Windows, the GenerateConsoleCtrlEvent function works with process groups, not individual
    ///   processes, so it may not work for all processes. The fallback "exit" command approach
    ///   works for many command-line applications that accept such commands.
    /// - For GUI applications on Windows, neither approach may work. In such cases, the `shutdown()`
    ///   method will fall back to forceful termination after the timeout.
    async fn graceful_shutdown(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            unsafe {
                // First, try to send Ctrl+C event to the process
                // CTRL_C_EVENT = 0, CTRL_BREAK_EVENT = 1
                // Note: GenerateConsoleCtrlEvent works with process groups, not individual processes
                // For console applications, we can try sending Ctrl+C to the process
                let result = winapi::um::wincon::GenerateConsoleCtrlEvent(0, self.pid);
                if result == 0 {
                    return Err(anyhow!("Failed to send Ctrl+C to process {}: {}", self.pid, std::io::Error::last_os_error()));
                }
            }
            return Ok(());
        }

        #[cfg(target_os = "linux")]
        {
            unsafe {
                // Use the kill system call with SIGTERM (15) to request graceful termination
                let result = libc::kill(self.pid as libc::pid_t, libc::SIGTERM);
                if result != 0 {
                    return Err(anyhow!("Failed to send SIGTERM to process {}: {}", self.pid, std::io::Error::last_os_error()));
                }
            }
            return Ok(());
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            return Err(anyhow!("Graceful shutdown not implemented for this platform"));
        }
    }

    /// Forcefully terminates the process immediately.
    ///
    /// This method should be used as a last resort when a process needs to be terminated
    /// immediately. For a more graceful approach, consider using `shutdown()` first.
    ///
    /// # Returns
    ///
    /// * `Ok(())` - If the process was successfully terminated.
    /// * `Err(Error)` - If an error occurred during the termination process.
    ///
    /// # Example
    ///
    /// ```rust
    /// if let Err(e) = process.kill().await {
    ///     eprintln!("Failed to terminate process: {}", e);
    /// }
    /// ```
    pub async fn kill(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            unsafe {
                // PROCESS_TERMINATE (0x00010000) access right is required to terminate a process
                let handle = winapi::um::processthreadsapi::OpenProcess(0x00010000, 0, self.pid);
                if handle.is_null() {
                    return Err(anyhow!("Failed to open process {}: {}", self.pid, std::io::Error::last_os_error()));
                }

                // Attempt to terminate the process
                let result = winapi::um::processthreadsapi::TerminateProcess(handle, 0);

                // Always close the handle to prevent resource leaks
                let close_result = winapi::um::handleapi::CloseHandle(handle);

                // Check if termination was successful
                if result == 0 {
                    return Err(anyhow!("Failed to terminate process {}: {}", self.pid, std::io::Error::last_os_error()));
                }

                // Check if handle was closed successfully
                if close_result == 0 {
                    warn!("Failed to close process handle for process {}: {}", self.pid, std::io::Error::last_os_error());
                    // We don't return an error here as the process was terminated successfully
                }
            }
        }
        #[cfg(target_os = "linux")]
        {
            unsafe {
                // Use the kill system call with SIGKILL (9) to forcefully terminate the process
                let result = libc::kill(self.pid as libc::pid_t, libc::SIGKILL);
                if result != 0 {
                    return Err(anyhow!("Failed to kill process {}: {}", self.pid, std::io::Error::last_os_error()));
                }
            }
        }

        Ok(())
    }
}

impl AsynchronousInteractiveProcess {
    /// Creates a new instance of the struct with default values.
    ///
    /// # Arguments
    ///
    /// * `filename` - A value that can be converted into a `String`. This typically represents the
    ///   name or path of the file associated with the instance.
    ///
    /// # Returns
    ///
    /// Returns a new instance of the struct populated with default fields:
    /// - `pid`: `None` (indicating no process ID is associated yet).
    /// - `filename`: The provided `filename` converted into a `String`.
    /// - `arguments`: An empty vector, representing no initial arguments.
    /// - `working_directory`: Set to the current directory (`"./"`).
    /// - `sender`: `None`, indicating no sender is associated initially.
    /// - `receiver`: `None`, indicating no receiver is associated initially.
    /// - `input_queue`: An empty `VecDeque`, representing no items in the input queue.
    ///
    /// # Example
    ///
    /// ```
    /// let instance = MyStruct::new("example.txt");
    /// assert_eq!(instance.filename, "example.txt");
    /// assert!(instance.pid.is_none());
    /// assert!(instance.arguments.is_empty());
    /// assert_eq!(instance.working_directory, PathBuf::from("./"));
    /// assert!(instance.sender.is_none());
    /// assert!(instance.receiver.is_none());
    /// assert!(instance.input_queue.is_empty());
    /// ```
    pub fn new(filename: impl Into<String>) -> Self {
        Self {
            pid: None,
            filename: filename.into(),
            arguments: Vec::new(),
            working_directory: PathBuf::from("./"),
            sender: None,
            receiver: None,
            input_queue: VecDeque::new(),
            exit_callback: None,
        }
    }

    /// Sets the arguments for the current instance by converting a vector of items
    /// implementing `Into<String>` into a `Vec<String>`.
    ///
    /// # Parameters
    /// - `args`: A `Vec` containing items that implement the `Into<String>` trait. These
    ///   items will be converted into `String` and used to set the `arguments` field
    ///   of the instance.
    ///
    /// # Returns
    /// - `Self`: The current instance (`self`) after updating its `arguments` field.
    ///
    /// # Example
    /// ```rust
    /// let instance = MyStruct::new()
    ///     .with_arguments(vec!["arg1", "arg2", String::from("arg3")]);
    /// ```
    ///
    /// This method allows chaining, as it returns the updated instance
    /// after setting the `arguments` field.
    pub fn with_arguments(mut self, args: Vec<impl Into<String>>) -> Self {
        self.arguments = args.into_iter().map(|arg| arg.into()).collect();
        self
    }

    /// Adds an argument to the `arguments` vector and returns the modified instance.
    ///
    /// # Parameters
    ///
    /// * `arg` - A value that implements the `Into<String>` trait, which will be converted into a `String`
    ///   and added to the `arguments` vector.
    ///
    /// # Returns
    ///
    /// Returns the modified instance of the implementing struct (`Self`) with the new argument added.
    ///
    /// # Example
    ///
    /// ```rust
    /// let instance = SomeStruct::new().with_argument("example");
    /// ```
    ///
    /// This adds the string `"example"` to the `arguments` vector of `SomeStruct`.
    pub fn with_argument(mut self, arg: impl Into<String>) -> Self {
        self.arguments.push(arg.into());
        self
    }

    /// Sets the working directory for the instance and returns the modified instance.
    ///
    /// # Arguments
    ///
    /// * `dir` - A value that can be converted into a `PathBuf`, representing the desired working directory.
    ///
    /// # Returns
    ///
    /// The instance of the struct with the updated working directory.
    ///
    /// # Example
    ///
    /// ```rust
    /// let instance = MyStruct::new()
    ///     .with_working_directory("/path/to/directory");
    /// ```
    pub fn with_working_directory(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_directory = dir.into();
        self
    }

    pub fn process_exit_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(i32) + Send + Sync + 'static,
    {
        self.exit_callback = Some(Arc::new(callback));
        self
    }

    /// Starts a new process based on the configuration stored in the struct, manages
    /// its I/O streams asynchronously, and tracks its lifecycle in a shared process pool.
    ///
    /// # Returns
    /// - `Ok<u32>`: Returns the process ID (PID) if the process starts successfully.
    /// - `Err(std::io::Error)`: Returns an error if any part of the process start operation fails.
    ///
    /// # Process Configuration
    /// - The process is launched using the executable specified in `self.filename`.
    /// - Command-line arguments are passed via `self.arguments`.
    /// - The process will run in the directory specified in `self.working_directory`.
    ///
    /// # Standard I/O Management
    /// - The process's `stdin` is assigned a `tokio::sync::mpsc::channel` for communication.
    /// - `stdout` and `stderr` are read asynchronously. Each line from these streams is sent
    ///   to an `mpsc::channel`, where `stdout`/`stderr` data can be consumed.
    ///
    /// # Shared Process Pool
    /// - The process metadata, including its PID and I/O channels, is stored in a global,
    ///   thread-safe `PROCESS_POOL`.
    /// - The process pool is managed using a `tokio::sync::Mutex` and stores processes in a
    ///   `HashMap` with their PID as the key.
    ///
    /// # Asynchronous I/O
    /// - A background task is spawned to write data from an internal input queue to the process's
    ///   `stdin`.
    /// - Another background task continuously reads and forwards `stdout` and `stderr` messages
    ///   from the process.
    /// - If the process exits or encounters an error, its PID and metadata are removed from the
    ///   process pool.
    ///
    /// # Example Use Case
    /// ```no_run
    /// let mut my_process = MyProcess {
    ///     filename: "my_program".to_string(),
    ///     arguments: vec!["--arg1", "value1".to_string()],
    ///     working_directory: "/path/to/dir".to_string(),
    ///     pid: None,
    ///     sender: None,
    ///     receiver: None,
    ///     input_queue: VecDeque::new(),
    /// };
    ///
    /// let pid = my_process.start().await.unwrap();
    /// println!("Started process with PID: {}", pid);
    /// ```
    ///
    /// # Notes
    /// - The process's `stdin`, `stdout`, and `stderr` are piped to capture and manage
    ///   communication asynchronously.
    /// - Each spawned background task is independently responsible for managing a specific
    ///   stream or part of the process lifecycle.
    /// - Errors during I/O operations or spawning the process are logged using the `log` crate
    ///   macros (e.g., `error!`, `debug!`).
    ///
    /// # Potential Errors
    /// - Failure to spawn the process (e.g., if `self.filename` is invalid or inaccessible).
    /// - Errors during communication with the process's I/O streams (e.g., writing to a closed `stdin`).
    /// - Mutex locking failure on the shared process pool due to an internal inconsistency.
    pub async fn start(&mut self) -> Result<u32> {
        let mut command = Command::new(&self.filename);
        command.args(&self.arguments);

        // Convert UNC path to regular Windows path if needed
        let working_dir = if cfg!(windows) {
            // Remove UNC prefix if present
            let path_str = self.working_directory.to_string_lossy();
            if path_str.starts_with(r"\\?\") {
                PathBuf::from(&path_str[4..]) // Remove the \\?\ prefix
            } else {
                self.working_directory.clone()
            }
        } else {
            self.working_directory.clone()
        };

        // Debug the working directory
        debug!("tokio-interactive: filename = {}", self.filename);
        debug!("tokio-interactive: arguments = {:?}", self.arguments);
        debug!("tokio-interactive: working_directory = {:?}", working_dir);
        debug!("tokio-interactive: working_directory exists = {}", working_dir.exists());
        debug!("tokio-interactive: working_directory is_dir = {}", working_dir.is_dir());

        // Check if working directory is absolute
        debug!("tokio-interactive: working_directory is_absolute = {}", working_dir.is_absolute());

        // Get current directory before setting
        if let Ok(current_dir) = std::env::current_dir() {
            debug!("tokio-interactive: current working directory before setting = {:?}", current_dir);
        }

        command.current_dir(working_dir);

        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());

        // Debug the final command that will be executed
        debug!("tokio-interactive: Final command = {:?}", command);

        let mut child = command.spawn()?;
        let pid = child.id().unwrap_or(0);

        debug!("tokio-interactive: Process spawned with PID = {}", pid);

        let (stdin_sender, mut stdin_receiver) = tokio::sync::mpsc::channel::<String>(100);
        let (stdout_sender, stdout_receiver) = tokio::sync::mpsc::channel::<String>(100);

        if let Some(mut stdin) = child.stdin.take() {
            tokio::spawn(async move {
                while let Some(input) = stdin_receiver.recv().await {
                    if let Err(e) = stdin.write_all(input.as_bytes()).await {
                        error!("Failed to write to process stdin: {}", e);
                        break;
                    }
                    if let Err(e) = stdin.write_all(b"\n").await {
                        error!("Failed to write newline to process stdin: {}", e);
                        break;
                    }
                    if let Err(e) = stdin.flush().await {
                        error!("Failed to flush process stdin: {}", e);
                        break;
                    }
                }
            });
        }

        if let Some(stdout) = child.stdout.take() {
            let stdout_sender_clone = stdout_sender.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                while let Ok(bytes_read) = reader.read_line(&mut line).await {
                    if bytes_read == 0 {
                        break;
                    }
                    if let Err(e) = stdout_sender_clone.send(line.trim_end().to_string()).await {
                        error!("Failed to send stdout message: {}", e);
                        break;
                    }
                    line.clear();
                }
            });
        }

        if let Some(stderr) = child.stderr.take() {
            let stderr_sender = stdout_sender.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                while let Ok(bytes_read) = reader.read_line(&mut line).await {
                    if bytes_read == 0 {
                        break;
                    }
                    if let Err(e) = stderr_sender.send(format!("STDERR: {}", line.trim_end())).await {
                        error!("Failed to send stderr message: {}", e);
                        break;
                    }
                    line.clear();
                }
            });
        }

        let queue_pid = pid;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

                let process_pool = match PROCESS_POOL.get() {
                    Some(pool) => pool,
                    None => break,
                };

                let mut pool = process_pool.lock().await;

                let process = match pool.get_mut(&queue_pid) {
                    Some(p) => p,
                    None => break,
                };

                while let Some(input) = process.input_queue.pop_front() {
                    if let Some(sender) = &process.sender {
                        if let Err(_) = sender.try_send(input.clone()) {
                            process.input_queue.push_front(input);
                            break;
                        }
                    } else {
                        process.input_queue.clear();
                        break;
                    }
                }

                drop(pool);
            }
        });

        let exit_callback = self.exit_callback.clone();
        tokio::spawn(async move {
            match child.wait().await {
                Ok(exit_status) => {
                    let exit_code = exit_status.code().unwrap_or(-1); // Use -1 for unknown exit codes
                    debug!("Process {} exited with code: {}", pid, exit_code);

                    if let Some(exit_callback) = exit_callback {
                        exit_callback(exit_code);
                    }
                }
                Err(e) => {
                    error!("Process {} exited with error: {}", pid, e);

                    // Still trigger callback with error code
                    if let Some(exit_callback) = exit_callback {
                        exit_callback(-1); // or some other error indicator
                    }
                }
            }

            // Cleanup
            if let Some(process_pool) = PROCESS_POOL.get() {
                let mut pool = process_pool.lock().await;
                pool.remove(&pid);
                debug!("Process {} has exited and been removed from the pool", pid);
            }
        });

        let process_pool = PROCESS_POOL.get_or_init(|| Arc::new(Mutex::new(HashMap::new())));

        let mut pool = process_pool.lock().await;
        pool.insert(
            pid,
            Self {
                pid: Some(pid),
                filename: self.filename.clone(),
                arguments: self.arguments.clone(),
                working_directory: self.working_directory.clone(),
                sender: Some(stdin_sender),
                receiver: Some(stdout_receiver),
                input_queue: VecDeque::new(),
                exit_callback: self.exit_callback.clone(),
            },
        );

        self.pid = Some(pid);

        Ok(pid)
    }

    /// Asynchronously retrieves a handle to a process for a given process ID (PID).
    ///
    /// This function checks if a process with the specified PID exists in a shared
    /// process pool. If the process is found, it returns an `Option` containing a
    /// `ProcessHandle` for the process. Otherwise, it returns `None`.
    ///
    /// # Arguments
    ///
    /// * `pid` - A 32-bit unsigned integer representing the process ID of the
    ///           desired process.
    ///
    /// # Returns
    ///
    /// * `Some(ProcessHandle)` - If a process with the given PID is found in the
    ///                           process pool.
    /// * `None` - If the process does not exist in the process pool or if the
    ///            process pool is uninitialized.
    ///
    /// # Example
    ///
    /// ```rust
    /// if let Some(handle) = get_process_by_pid(12345).await {
    ///     println!("Process found with PID: {}", handle.pid);
    /// } else {
    ///     println!("Process not found.");
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// This function will return `None` if the global process pool (`PROCESS_POOL`)
    /// has not been initialized or is unavailable.
    ///
    /// # Note
    ///
    /// This is an asynchronous function and must be awaited to complete its operation.
    pub async fn get_process_by_pid(pid: u32) -> Option<ProcessHandle> {
        let process_pool = PROCESS_POOL.get()?;
        let pool = process_pool.lock().await;
        if pool.contains_key(&pid) { Some(ProcessHandle { pid }) } else { None }
    }

    /// Asynchronously checks if a process identified by its `pid` is currently running.
    ///
    /// # Returns
    /// - `true` if the process with the stored `pid` is found in the `PROCESS_POOL`.
    /// - `false` if the stored `pid` is `None`, the `PROCESS_POOL` is not initialized,
    ///   or the `pid` does not exist in the `PROCESS_POOL`.
    ///
    /// The function first checks if the `pid` instance variable is set. If so, it attempts
    /// to access the global `PROCESS_POOL`. If `PROCESS_POOL` is initialized, it acquires
    /// a lock and checks if the `pid` exists in the pool.
    ///
    /// # Examples
    /// ```rust
    /// let result = my_instance.is_process_running().await;
    /// if result {
    ///     println!("The process is running.");
    /// } else {
    ///     println!("The process is not running.");
    /// }
    /// ```
    ///
    /// # Async Behavior
    /// This function acquires an asynchronous lock on the `PROCESS_POOL` to safely access
    /// its contents and will yield if the lock is currently held elsewhere.
    ///
    /// # Panics
    /// This function may panic if the async lock on the `PROCESS_POOL` fails unexpectedly.
    ///
    /// # Dependencies
    /// - `PROCESS_POOL` should be a globally accessible and lazily initialized structure
    ///   (e.g., using `once_cell` or similar patterns) that maintains a mapping of currently
    ///   active processes.
    pub async fn is_process_running(&self) -> bool {
        if let Some(pid) = self.pid {
            if let Some(process_pool) = PROCESS_POOL.get() {
                let pool = process_pool.lock().await;
                return pool.contains_key(&pid);
            }
        }
        false
    }
}
