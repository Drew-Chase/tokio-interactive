//! # Asynchronous Interactive Process Management
//!
//! This module provides functionalities for handling and interacting with asynchronous interactive processes.
//! It employs a shared process pool and uses asynchronous mechanisms such as Tokio channels to manage communication
//! with processes.
//!
//! ## Key Components
//!
//! - **`ProcessHandle`**: A struct representing a handle to an active process. Used to interact with and manage a process, such as sending input, receiving output, checking if the process is running, or killing the process.
//! - **`AsynchronousInteractiveProcess`**: Represents a configurable and asynchronously manageable interactive process. Allows setting up the executable, arguments, working directory, and managing input and output streams.
//!
//! ## Static Objects
//!
//! - `PROCESS_POOL`: A global, lazily initialized container (`OnceLock`) to manage all processes as a shared resource using a `HashMap`. Each process is identified and stored using its PID (process ID).
//!
//! ## Structs
//!
//! ### `ProcessHandle`
//!
//! This struct provides methods to interact with an existing process, identified by its PID.
//!
//! #### Fields
//! - `pid (u32)`: The process ID.
//!
//! #### Methods
//! - `receive_output`: Reads and returns a message from the process's output stream, if available.
//! - `send_input`: Sends input data to the process's input stream.
//! - `is_process_running`: Checks if the process is currently running.
//! - `kill`: Forcefully terminates the associated process using OS-specific commands.
//!
//! ### `AsynchronousInteractiveProcess`
//!
//! Represents a process that can be launched and managed asynchronously, with configurable options like arguments and working directory.
//!
//! #### Fields
//! - `pid (Option<u32>)`: The process ID of the running process, if it is currently active.
//! - `filename (String)`: The path to the executable file.
//! - `arguments (Vec<String>)`: Command-line arguments for the process.
//! - `working_directory (PathBuf)`: The working directory where the process will run.
//! - `sender (Option<Sender<String>>)`: Used to send input strings to the process.
//! - `receiver (Option<Receiver<String>>)`: Used to receive output strings from the process.
//! - `input_queue (VecDeque<String>)`: Queue for storing pending inputs that couldn't be directly sent.
//!
//! #### Methods
//! - `new`: Creates a new process instance with the specified executable filename.
//! - `with_arguments`: Adds multiple command-line arguments to the process.
//! - `with_argument`: Adds a single command-line argument to the process.
//! - `with_working_directory`: Sets the working directory for the process.
//! - `start`: Spawns the process asynchronously and initializes its standard input/output channels for interaction.
//!
//! ### OS-Specific Behavior
//!
//! The `kill` method implements OS-specific behavior for terminating processes:
//!
//! - **Windows**: Uses WinAPI functions (`OpenProcess` and `TerminateProcess`) to terminate the process.
//! - **Linux**: Uses the `kill` system call with the `SIGKILL` signal to terminate the process.
//!
//! ## Example Usage
//!
//! ```rust
//! use your_crate::AsynchronousInteractiveProcess;
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut process = AsynchronousInteractiveProcess::new("echo")
//!         .with_argument("Hello, World!".to_string())
//!         .with_working_directory("/tmp");
//!
//!     match process.start().await {
//!         Ok(pid) => println!("Process started with PID: {}", pid),
//!         Err(error) => eprintln!("Failed to start the process: {}", error),
//!     }
//! }
//! ```
//!
//! ## Error Handling
//!
//! - Most functions return a `Result` type. Errors can occur during process creation, communication (e.g., failed input/output operations), or due to process pool initialization issues.
//!
//! ## Dependencies
//!
//! - **Tokio**: Provides asynchronous runtime and mechanisms for interacting with processes and channels.
//! - **Serde**: Enables serialization and deserialization of the process struct for potential configuration management.
//! - **Anyhow**: Simplifies error handling with expressive context.
//! - **Log**: Used for logging errors and warnings during process interactions.
//!
//! ## Notes
//!
//! - The `AsynchronousInteractiveProcess` struct uses channels (`Sender` and `Receiver`) for non-blocking process communication.
//! - Input queuing ensures that pending inputs are not lost when the process's input channel is full.
use anyhow::{Result, anyhow};
use log::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{Receiver, Sender}; // Change this line

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
#[derive(Debug, Serialize, Deserialize, Default)]
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
    /// 5. If a message is found, return it as a `Some(String)`.
    /// 6. If no message is received, retry up to 10 times, with a delay of 10 milliseconds between
    ///    each retry.
    ///
    /// If no message is received after all retries, or if the process or receiver channel is not
    /// found, the function returns `None`.
    ///
    /// # Returns
    ///
    /// * `Some(String)` - If a message is successfully received from the receiver channel.
    /// * `None` - If no message is received after retries or if the process or receiver is unavailable.
    ///
    /// # Behavior
    ///
    /// - This function uses `tokio::time::sleep` for introducing delays between retries and works
    ///   asynchronously.
    /// - If the system does not have the required process pool or if the receiver is unavailable, the
    ///   method will return without further retries.
    ///
    /// # Example
    ///
    /// ```rust
    /// if let Some(output) = instance.receive_output().await {
    ///     println!("Received output: {}", output);
    /// } else {
    ///     println!("No output received.");
    /// }
    /// ```
    ///
    /// # Note
    ///
    /// - The function assumes the existence of a global process pool (`PROCESS_POOL`) which is safe
    ///   to access concurrently using mutex-based locking.
    /// - The function makes use of `tokio::sync::Mutex` to handle concurrent executions across
    ///   asynchronous tasks.
    pub async fn receive_output(&self) -> Option<String> {
        if let Some(process_pool) = PROCESS_POOL.get() {
            {
                let mut pool = process_pool.lock().await;
                if let Some(process) = pool.get_mut(&self.pid) {
                    if let Some(receiver) = &mut process.receiver {
                        match receiver.try_recv() {
                            Ok(msg) => return Some(msg),
                            Err(_) => {}
                        }
                    }
                }
            }

            for _ in 0..10 {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

                let mut pool = process_pool.lock().await;
                if let Some(process) = pool.get_mut(&self.pid) {
                    if let Some(receiver) = &mut process.receiver {
                        match receiver.try_recv() {
                            Ok(msg) => return Some(msg),
                            Err(_) => {}
                        }
                    }
                }
            }
        }
        None
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
                            tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                                Err(anyhow!("Failed to send input: channel closed"))
                            }
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
    pub async fn kill(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            // get handle via pid
            unsafe {
                let handle = winapi::um::processthreadsapi::OpenProcess(0x00010000, 0, self.pid);
                winapi::um::processthreadsapi::TerminateProcess(handle, 0);
            };
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
        command.current_dir(&self.working_directory);

        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());

        let mut child = command.spawn()?;
        let pid = child.id().unwrap_or(0);

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

        tokio::spawn(async move {
            if let Err(e) = child.wait().await {
                error!("Process {} exited with error: {}", pid, e);
            } else {
                debug!("Process {} exited successfully", pid);
            }

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
