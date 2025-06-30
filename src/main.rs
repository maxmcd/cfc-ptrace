use nix::sys::ptrace;
use nix::sys::signal::Signal;
use nix::sys::wait::{wait, WaitStatus};
use nix::unistd::{fork, ForkResult, Pid};
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::os::unix::process::CommandExt;
use std::process::{exit, Command};

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

#[derive(Debug, Clone)]
struct FakeFile {
    content: Vec<u8>,
    position: usize,
    path: String,
}

struct FakeFileSystem {
    files: HashMap<String, Vec<u8>>,
    open_files: HashMap<i32, FakeFile>,
    next_fd: i32,
}

impl FakeFileSystem {
    fn new() -> Self {
        let mut files = HashMap::new();
        files.insert(
            "/fake/test.txt".to_string(),
            "Hello from fake filesystem!\nThis is intercepted content."
                .as_bytes()
                .to_vec(),
        );
        files.insert(
            "/another/fake/file.txt".to_string(),
            "Another fake file!\nPtrace interception working."
                .as_bytes()
                .to_vec(),
        );

        Self {
            files,
            open_files: HashMap::new(),
            next_fd: 1000, // Start fake fds at 1000
        }
    }

    fn open_file(&mut self, path: &str) -> Option<i32> {
        if let Some(content) = self.files.get(path) {
            let fd = self.next_fd;
            self.next_fd += 1;

            self.open_files.insert(
                fd,
                FakeFile {
                    content: content.clone(),
                    position: 0,
                    path: path.to_string(),
                },
            );

            Some(fd)
        } else {
            None
        }
    }

    fn read_file(&mut self, fd: i32, count: usize) -> Option<Vec<u8>> {
        if let Some(fake_file) = self.open_files.get_mut(&fd) {
            let available = fake_file.content.len().saturating_sub(fake_file.position);
            let to_read = count.min(available);

            if to_read == 0 {
                return Some(Vec::new());
            }

            let data = fake_file.content[fake_file.position..fake_file.position + to_read].to_vec();
            fake_file.position += to_read;
            Some(data)
        } else {
            None
        }
    }

    fn write_file(&mut self, fd: i32, data: &[u8]) -> Option<usize> {
        if let Some(fake_file) = self.open_files.get_mut(&fd) {
            // For WriteFile operations, we typically replace the entire content
            // This matches Go's os.WriteFile behavior which truncates and writes
            fake_file.content = data.to_vec();
            fake_file.position = data.len();

            // Update the master file content using the stored path
            let path = fake_file.path.clone();
            self.files.insert(path, fake_file.content.clone());

            Some(data.len())
        } else {
            None
        }
    }

    fn seek_file(&mut self, fd: i32, offset: i64, whence: i32) -> Option<i64> {
        if let Some(fake_file) = self.open_files.get_mut(&fd) {
            let new_pos = match whence {
                0 => offset as usize,                                         // SEEK_SET
                1 => fake_file.position.saturating_add(offset as usize),      // SEEK_CUR
                2 => fake_file.content.len().saturating_add(offset as usize), // SEEK_END
                _ => return None,
            };

            fake_file.position = new_pos;
            Some(new_pos as i64)
        } else {
            None
        }
    }

    fn close_file(&mut self, fd: i32) -> bool {
        self.open_files.remove(&fd).is_some()
    }

    fn is_fake_fd(&self, fd: i32) -> bool {
        self.open_files.contains_key(&fd)
    }
}

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

fn handle_syscall_entry(
    pid: Pid,
    fake_fs: &mut FakeFileSystem,
) -> Result<(bool, Option<String>, usize), PtraceError> {
    let regs = ptrace::getregs(pid)
        .map_err(|e| PtraceError::PtraceOperation(format!("getregs failed: {}", e)))?;

    match regs.orig_rax as i64 {
        SYS_OPENAT => {
            let pathname_addr = regs.rsi;

            match read_string(pid, pathname_addr) {
                Ok(pathname) => {
                    println!("openat: {}", pathname);

                    if fake_fs.files.contains_key(&pathname) {
                        println!("  -> will intercept this openat");
                        return Ok((true, Some(pathname), 0)); // Mark for interception
                    }
                }
                Err(e) => {
                    eprintln!("Failed to read pathname: {}", e);
                }
            }
        }
        SYS_READ => {
            let fd = regs.rdi as i32;
            if fake_fs.is_fake_fd(fd) {
                let buf_addr = regs.rsi;
                let count = regs.rdx as usize;
                println!("read: fd={}, count={}", fd, count);

                if let Some(data) = fake_fs.read_file(fd, count) {
                    let bytes_read = data.len();
                    println!("  -> writing {} bytes to child memory", bytes_read);
                    match write_data_to_child(pid, buf_addr, &data) {
                        Ok(_) => return Ok((true, None, bytes_read)), // Mark for interception, store bytes read
                        Err(e) => eprintln!("Failed to write data to child: {}", e),
                    }
                }
            }
        }
        SYS_WRITE => {
            let fd = regs.rdi as i32;
            if fake_fs.is_fake_fd(fd) {
                let buf_addr = regs.rsi;
                let count = regs.rdx as usize;
                println!("write: fd={}, count={}", fd, count);

                match read_data_from_child(pid, buf_addr, count) {
                    Ok(data) => {
                        if let Some(bytes_written) = fake_fs.write_file(fd, &data) {
                            println!("  -> wrote {} bytes to fake file", bytes_written);
                            return Ok((true, None, bytes_written)); // Mark for interception
                        }
                    }
                    Err(e) => eprintln!("Failed to read data from child: {}", e),
                }
            }
        }
        SYS_LSEEK => {
            let fd = regs.rdi as i32;
            if fake_fs.is_fake_fd(fd) {
                let offset = regs.rsi as i64;
                let whence = regs.rdx as i32;
                println!("lseek: fd={}, offset={}, whence={}", fd, offset, whence);

                if let Some(new_pos) = fake_fs.seek_file(fd, offset, whence) {
                    println!("  -> new position: {}", new_pos);
                    return Ok((true, None, new_pos as usize)); // Mark for interception
                }
            }
        }
        SYS_CLOSE => {
            let fd = regs.rdi as i32;
            if fake_fs.close_file(fd) {
                println!("close: fake fd={}", fd);
                return Ok((true, None, 0)); // Mark for interception
            }
        }
        _ => {}
    }

    Ok((false, None, 0)) // Don't intercept
}

fn handle_syscall_exit(
    pid: Pid,
    fake_fs: &mut FakeFileSystem,
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
            if let Some(path) = pathname {
                if let Some(fake_fd) = fake_fs.open_file(&path) {
                    println!("  -> overriding return value with fake fd: {}", fake_fd);
                    regs.rax = fake_fd as u64;
                    ptrace::setregs(pid, regs).map_err(|e| {
                        PtraceError::PtraceOperation(format!("setregs failed: {}", e))
                    })?;
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

fn run_parent(pid: Pid) -> Result<(), PtraceError> {
    let mut fake_fs = FakeFileSystem::new();
    let mut in_syscall = false;
    let mut should_intercept = false;
    let mut pathname: Option<String> = None;
    let mut bytes_read = 0;

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
                    match handle_syscall_entry(pid, &mut fake_fs) {
                        Ok((intercept, path, read_bytes)) => {
                            should_intercept = intercept;
                            pathname = path;
                            bytes_read = read_bytes;
                            in_syscall = true;
                        }
                        Err(e) => {
                            eprintln!("Error handling syscall entry: {}", e);
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
                    ) {
                        eprintln!("Error handling syscall exit: {}", e);
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
                break;
            }
            Ok(WaitStatus::Signaled(_, signal, _)) => {
                println!("Process killed by signal {:?}", signal);
                break;
            }
            Ok(status) => {
                println!("Other status: {:?}", status);
                ptrace::syscall(pid, None).map_err(|e| {
                    PtraceError::PtraceOperation(format!("syscall continue failed: {}", e))
                })?;
            }
            Err(err) => {
                eprintln!("Wait error: {}", err);
                break;
            }
        }
    }

    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <program> [args...]", args[0]);
        exit(1);
    }

    let program = &args[1];
    let program_args = &args[2..];

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            if let Err(e) = run_child(program, program_args) {
                eprintln!("Child process error: {}", e);
                exit(1);
            }
        }
        Ok(ForkResult::Parent { child }) => {
            if let Err(e) = run_parent(child) {
                eprintln!("Parent process error: {}", e);
                exit(1);
            }
        }
        Err(err) => {
            eprintln!("fork() failed: {}", err);
            exit(1);
        }
    }
}
