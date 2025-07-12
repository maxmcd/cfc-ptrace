use nix::sys::ptrace;
use nix::sys::signal::Signal;
use nix::sys::wait::{wait, WaitStatus};
use nix::unistd::{fork, ForkResult, Pid};
use std::env;
use std::fmt;
use std::os::unix::process::CommandExt;
use std::process::{exit, Command};
use tokio::runtime::Runtime;

mod websocket_fs;
#[cfg(test)]
mod websocket_fs_tests;

use websocket_fs::WebSocketFileSystem;

#[derive(Debug)]
enum PtraceError {
    PtraceOperation(String),
    InvalidAddress,
    BufferTooLarge,
    StringTooLong,
    MemoryWrite,
    MemoryRead,
}

impl fmt::Display for PtraceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PtraceError::PtraceOperation(msg) => write!(f, "Ptrace operation failed: {}", msg),
            PtraceError::InvalidAddress => write!(f, "Invalid memory address"),
            PtraceError::BufferTooLarge => write!(f, "Buffer size exceeds maximum allowed"),
            PtraceError::StringTooLong => write!(f, "String exceeds maximum length"),
            PtraceError::MemoryWrite => write!(f, "Failed to write to child memory"),
            PtraceError::MemoryRead => write!(f, "Failed to read from child memory"),
        }
    }
}

impl std::error::Error for PtraceError {}

const SYS_OPENAT: i64 = 257;
const SYS_READ: i64 = 0;
const SYS_WRITE: i64 = 1;
const SYS_CLOSE: i64 = 3;
const SYS_LSEEK: i64 = 8;

const MAX_STRING_LENGTH: usize = 4096;
const MAX_BUFFER_SIZE: usize = 1024 * 1024; // 1MB

fn run_child(program: &str, args: &[String]) -> Result<(), PtraceError> {
    ptrace::traceme()
        .map_err(|e| PtraceError::PtraceOperation(format!("traceme failed: {}", e)))?;

    let mut cmd = Command::new(program);
    for arg in args {
        cmd.arg(arg);
    }

    let err = cmd.exec();
    eprintln!("Failed to execute program: {}", err);
    exit(1);
}

fn read_string(pid: Pid, addr: u64) -> Result<String, PtraceError> {
    if addr == 0 {
        return Err(PtraceError::InvalidAddress);
    }

    let mut result = Vec::new();
    let mut current_addr = addr;
    let max_iterations = MAX_STRING_LENGTH / 8 + 1;

    for _ in 0..max_iterations {
        let word = ptrace::read(pid, current_addr as *mut std::ffi::c_void)
            .map_err(|_| PtraceError::MemoryRead)? as u64;

        for i in 0..8 {
            let byte = ((word >> (i * 8)) & 0xff) as u8;
            if byte == 0 {
                return Ok(String::from_utf8_lossy(&result).into_owned());
            }

            if result.len() >= MAX_STRING_LENGTH {
                return Err(PtraceError::StringTooLong);
            }

            result.push(byte);
        }
        current_addr += 8;
    }

    Err(PtraceError::StringTooLong)
}

fn read_data_from_child(pid: Pid, addr: u64, count: usize) -> Result<Vec<u8>, PtraceError> {
    if addr == 0 {
        return Err(PtraceError::InvalidAddress);
    }

    if count > MAX_BUFFER_SIZE {
        return Err(PtraceError::BufferTooLarge);
    }

    let mut result = Vec::new();
    let mut current_addr = addr;
    let mut remaining = count;

    while remaining > 0 {
        let word = ptrace::read(pid, current_addr as *mut std::ffi::c_void)
            .map_err(|_| PtraceError::MemoryRead)? as u64;

        let bytes_to_read = remaining.min(8);
        for i in 0..bytes_to_read {
            let byte = ((word >> (i * 8)) & 0xff) as u8;
            result.push(byte);
        }

        remaining -= bytes_to_read;
        current_addr += 8;
    }

    Ok(result)
}

fn write_data_to_child(pid: Pid, addr: u64, data: &[u8]) -> Result<(), PtraceError> {
    if addr == 0 {
        return Err(PtraceError::InvalidAddress);
    }

    if data.len() > MAX_BUFFER_SIZE {
        return Err(PtraceError::BufferTooLarge);
    }

    let mut current_addr = addr;

    for chunk in data.chunks(8) {
        let mut word: u64 = 0;
        for (i, &byte) in chunk.iter().enumerate() {
            word |= (byte as u64) << (i * 8);
        }

        ptrace::write(pid, current_addr as *mut std::ffi::c_void, word as i64)
            .map_err(|_| PtraceError::MemoryWrite)?;
        current_addr += 8;
    }

    Ok(())
}

async fn handle_syscall_entry(
    pid: Pid,
    fs: &mut WebSocketFileSystem,
) -> Result<(bool, Option<String>, usize), PtraceError> {
    let regs = ptrace::getregs(pid)
        .map_err(|e| PtraceError::PtraceOperation(format!("getregs failed: {}", e)))?;

    match regs.orig_rax as i64 {
        SYS_OPENAT => {
            let pathname_addr = regs.rsi;
            match read_string(pid, pathname_addr) {
                Ok(pathname) => {
                    println!("openat: {}", pathname);
                    fs.open_file(&pathname).await.map_err(|e| {
                        PtraceError::PtraceOperation(format!("Failed to open file: {}", e))
                    })?;
                }
                Err(e) => {
                    eprintln!("Failed to read pathname: {}", e);
                }
            }
        }
        SYS_READ => {
            println!("read: {:?}", regs.rsi);
        }
        SYS_WRITE => {
            println!("write: {:?}", regs.rsi);
        }
        SYS_LSEEK => {
            println!("lseek: {:?}", regs.rsi);
        }
        SYS_CLOSE => {
            println!("close: {:?}", regs.rsi);
            let fd = regs.rdi as i32;
            if fs.close_file(fd) {
                println!("close: fake fd={}", fd);
                return Ok((true, None, 0)); // Mark for interception
            }
        }
        _ => {}
    }

    Ok((false, None, 0)) // Don't intercept
}

async fn handle_syscall_exit(
    pid: Pid,
    fs: &mut WebSocketFileSystem,
    should_intercept: bool,
    pathname: Option<String>,
    bytes_read: usize,
) -> Result<(), PtraceError> {
    if !should_intercept {
        return Ok(());
    }

    let mut regs = ptrace::getregs(pid)
        .map_err(|e| PtraceError::PtraceOperation(format!("getregs failed: {}", e)))?;

    match regs.orig_rax as i64 {
        SYS_OPENAT => {
            println!("openat exit: {:?}", regs);
            if let Some(path) = pathname {
                match fs.open_file(&path).await {
                    Ok(fake_fd) => {}
                    Err(e) => {
                        eprintln!("Failed to open file {}: {}", path, e);
                        // Return ENOENT (2) to indicate file not found
                        regs.rax = (-2_i64) as u64;
                        ptrace::setregs(pid, regs).map_err(|e| {
                            PtraceError::PtraceOperation(format!("setregs failed: {}", e))
                        })?;
                    }
                }
            }
        }
        SYS_READ => {
            // We already wrote the data in entry, just set the return value
            regs.rax = bytes_read as u64;
            ptrace::setregs(pid, regs)
                .map_err(|e| PtraceError::PtraceOperation(format!("setregs failed: {}", e)))?;
        }
        SYS_WRITE => {
            // We already handled the write in entry, just set the return value
            regs.rax = bytes_read as u64; // bytes_read is repurposed as bytes_written here
            ptrace::setregs(pid, regs)
                .map_err(|e| PtraceError::PtraceOperation(format!("setregs failed: {}", e)))?;
        }
        SYS_LSEEK => {
            // Return the new file position
            regs.rax = bytes_read as u64; // bytes_read is repurposed as new_position here
            ptrace::setregs(pid, regs)
                .map_err(|e| PtraceError::PtraceOperation(format!("setregs failed: {}", e)))?;
        }
        SYS_CLOSE => {
            regs.rax = 0; // Success
            ptrace::setregs(pid, regs)
                .map_err(|e| PtraceError::PtraceOperation(format!("setregs failed: {}", e)))?;
        }
        _ => {}
    }

    Ok(())
}

async fn run_parent(pid: Pid) -> Result<i32, PtraceError> {
    let cache_dir = env::var("CACHE_DIR").unwrap_or_else(|_| "/tmp/cfc-cache".to_string());
    let mut fake_fs = WebSocketFileSystem::new(cache_dir);
    let mut in_syscall = false;
    let mut should_intercept = false;
    let mut pathname: Option<String> = None;
    let mut bytes_read = 0;

    // Start WebSocket server and wait for client
    println!("Starting WebSocket server...");
    fake_fs.start_server(8080).await.map_err(|e| {
        PtraceError::PtraceOperation(format!("Failed to start WebSocket server: {}", e))
    })?;

    wait().map_err(|e| PtraceError::PtraceOperation(format!("initial wait failed: {}", e)))?;

    ptrace::setoptions(pid, ptrace::Options::PTRACE_O_TRACESYSGOOD)
        .map_err(|e| PtraceError::PtraceOperation(format!("setoptions failed: {}", e)))?;

    ptrace::syscall(pid, None)
        .map_err(|e| PtraceError::PtraceOperation(format!("initial syscall failed: {}", e)))?;

    loop {
        match wait() {
            Ok(WaitStatus::Stopped(_, Signal::SIGTRAP)) | Ok(WaitStatus::PtraceSyscall(_)) => {
                if !in_syscall {
                    // Syscall entry
                    match handle_syscall_entry(pid, &mut fake_fs).await {
                        Ok((intercept, path, read_bytes)) => {
                            should_intercept = intercept;
                            pathname = path;
                            bytes_read = read_bytes;
                            in_syscall = true;
                        }
                        Err(e) => {
                            println!("Error handling syscall entry: {}", e);
                        }
                    }
                } else {
                    // Syscall exit
                    if let Err(e) = handle_syscall_exit(
                        pid,
                        &mut fake_fs,
                        should_intercept,
                        pathname.clone(),
                        bytes_read,
                    )
                    .await
                    {
                        println!("Error handling syscall exit: {}", e);
                    }
                    in_syscall = false;
                    should_intercept = false;
                    pathname = None;
                    bytes_read = 0;
                }

                ptrace::syscall(pid, None).map_err(|e| {
                    PtraceError::PtraceOperation(format!("syscall continue failed: {}", e))
                })?;
            }
            Ok(WaitStatus::Stopped(_, signal)) => {
                if signal != Signal::SIGURG {
                    println!("Process stopped by signal {:?}, continuing...", signal);
                }
                ptrace::syscall(pid, Some(signal)).map_err(|e| {
                    PtraceError::PtraceOperation(format!("syscall with signal failed: {}", e))
                })?;
            }
            Ok(WaitStatus::Exited(_, exit_status)) => {
                println!("Process exited with status {}", exit_status);
                return Ok(exit_status);
            }
            Ok(WaitStatus::Signaled(_, signal, _)) => {
                println!("Process killed by signal {:?}", signal);
                return Ok(128 + signal as i32);  // Standard convention for signal termination
            }
            Ok(status) => {
                println!("Other status: {:?}", status);
                ptrace::syscall(pid, None).map_err(|e| {
                    PtraceError::PtraceOperation(format!("syscall continue failed: {}", e))
                })?;
            }
            Err(err) => {
                eprintln!("Wait error: {}", err);
                return Ok(1);  // Return error exit code
            }
        }
    }

    Ok(0)  // Should not reach here, but return success if we do
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <program> [args...]", args[0]);
        exit(1);
    }

    let program = &args[1];
    let program_args = &args[2..];

    let rt = Runtime::new().expect("Failed to create tokio runtime");

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            if let Err(e) = run_child(program, program_args) {
                eprintln!("Child process error: {}", e);
                exit(1);
            }
        }
        Ok(ForkResult::Parent { child }) => {
            match rt.block_on(run_parent(child)) {
                Ok(exit_code) => {
                    exit(exit_code);
                }
                Err(e) => {
                    eprintln!("Parent process error: {}", e);
                    exit(1);
                }
            }
        }
        Err(err) => {
            eprintln!("fork() failed: {}", err);
            exit(1);
        }
    }
}
