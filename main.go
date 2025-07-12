package main

import (
	"fmt"
	"log"
	"os"
	"path/filepath"
)

func main() {
	fmt.Println("Go program starting...")

	dir, err := os.Getwd()
	if err != nil {
		fmt.Printf("Error getting working directory: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("Working directory: %s\n", dir)

	firstFakeFile := filepath.Join(dir, "fs", "test.txt")

	f, err := os.Open(firstFakeFile)
	if err != nil {
		log.Panicln(err)
	}
	if n, err := f.Write([]byte("hi")); err != nil {
		log.Panicln(err)
	} else if n < 2 {
		log.Panicln("incorrect length", n)
	}

}
