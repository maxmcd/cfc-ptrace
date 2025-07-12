# CFC-Ptrace

A ptrace-based virtual filesystem that intercepts filesystem syscalls and redirects them to a WebSocket-backed filesystem. Originally designed for Cloudflare D1/Durable Objects integration.

## How It Works

This program uses Linux ptrace to watch any executable and intercept its filesystem operations. When the program tries to access files, those operations get redirected to a custom WebSocket filesystem instead of the real filesystem. The virtual filesystem stores data in a SQLite database.

## Usage

```bash
# Build the Rust tracer
cargo build --release

# Run any executable under the virtual filesystem
./target/release/cfc-ptrace <executable> [args...]
```

## Example with Go Test Program

The repository includes a simple Go test program that demonstrates the virtual filesystem:

```bash
# Build the test program
go build -o cfc-ptrace.bin .

# Run under the virtual filesystem
./target/release/cfc-ptrace ./cfc-ptrace.bin
```

Example output:
```
Starting WebSocket server...
Go program starting...
Working directory: /home/maxm/go/src/github.com/maxmcd/cfc-ptrace
openat: /home/maxm/go/src/github.com/maxmcd/cfc-ptrace/fs/test.txt
write: 0x7ffd12345678
read: 0x7ffd12345678
close: 1000
✓ All tests passed - Go program finished successfully
Process exited with status 0
```

## Architecture

The Rust program forks into two processes. The parent process uses ptrace to monitor the child process. When the child makes filesystem syscalls, the parent handles them through a WebSocket server that communicates with a SQLite-backed filesystem. This allows programs to run normally while their file operations are redirected to a virtual filesystem that can be hosted remotely or backed by cloud storage.