A (wip) ptrace virtual filesystem backed by cloudflare d1 on durable objects.

Run with `just`:

```
go build -o cfc-ptrace.bin .
cargo build --release
    Finished `release` profile [optimized] target(s) in 0.02s
./target/release/cfc-ptrace ./cfc-ptrace.bin
openat: /sys/kernel/mm/transparent_hugepage/hpage_pmd_size
Go program starting...
openat: /fake/test.txt
  -> will intercept this openat
  -> overriding return value with fake fd: 1000
read: fd=1000, count=512
  -> writing 56 bytes to child memory
read: fd=1000, count=456
  -> writing 0 bytes to child memory
close: fake fd=1000
✓ Initial read successful: Hello from fake filesystem!\nThis is intercepted content.
openat: /fake/test.txt
  -> will intercept this openat
  -> overriding return value with fake fd: 1001
write: fd=1001, count=53
  -> wrote 53 bytes to fake file
close: fake fd=1001
✓ Write operation completed
openat: /fake/test.txt
  -> will intercept this openat
  -> overriding return value with fake fd: 1002
read: fd=1002, count=512
  -> writing 53 bytes to child memory
read: fd=1002, count=459
  -> writing 0 bytes to child memory
close: fake fd=1002
✓ Content verified after write: Modified content from Go!\nWrite operation successful.
openat: /another/fake/file.txt
  -> will intercept this openat
  -> overriding return value with fake fd: 1003
write: fd=1003, count=28
  -> wrote 28 bytes to fake file
close: fake fd=1003
openat: /another/fake/file.txt
  -> will intercept this openat
  -> overriding return value with fake fd: 1004
read: fd=1004, count=512
  -> writing 28 bytes to child memory
read: fd=1004, count=484
  -> writing 0 bytes to child memory
close: fake fd=1004
✓ Second file write/read successful: Data written to second file!
✓ All tests passed - Go program finished successfully
Process exited with status 0
```
