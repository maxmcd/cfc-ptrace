package main

import (
	"fmt"
	"log"
	"os"
	"path/filepath"
)

func main() {
	fmt.Fprintln(os.Stderr, "Go program starting...")

	dir, err := os.Getwd()
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error getting working directory: %v\n", err)
		os.Exit(1)
	}
	fmt.Fprintf(os.Stderr, "Working directory: %s\n", dir)

	firstFakeFile := filepath.Join(dir, "fs", "test.txt")

	if _, err := os.Open(firstFakeFile); err == nil {
		log.Panicln("file should not exist")
	}

	contents := []byte("hi")
	if err := os.WriteFile(firstFakeFile, contents, 0644); err != nil {
		log.Panicln(err)
	}

	if c, err := os.ReadFile(firstFakeFile); err != nil {
		log.Panicln(err)
	} else if string(c) != string(contents) {
		log.Panicln("contents should be the same")
	}

	os.Exit(0)
}
