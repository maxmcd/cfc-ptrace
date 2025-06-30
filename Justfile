
run: compile_go compile_rust
     ./target/release/cfc-ptrace ./cfc-ptrace.bin


compile_go:
    go build -o cfc-ptrace.bin .

compile_rust:
    cargo build --release

