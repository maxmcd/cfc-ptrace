
run: compile_go compile_rust
    mkdir -p ./fs
    rm -rf ./fs/*
    ./target/release/cfc-ptrace ./cfc-ptrace.bin


compile_go:
    go build -o cfc-ptrace.bin .

compile_rust:
    cargo build --release

