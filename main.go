package main

import (
	"fmt"
	"os"
	"strings"
	"time"
)

func main() {
	fmt.Println("Go program starting...")

	// Test 1: Read initial content of fake file
	content, err := os.ReadFile("/fake/test.txt")
	if err != nil {
		fmt.Printf("Error reading file: %v\n", err)
		os.Exit(1)
	}
	expected := "Hello from fake filesystem!\nThis is intercepted content."
	if string(content) != expected {
		fmt.Printf("ERROR: Expected content '%s', got '%s'\n", expected, string(content))
		os.Exit(1)
	}
	fmt.Printf("✓ Initial read successful: %s\n", strings.ReplaceAll(string(content), "\n", "\\n"))

	// Test 2: Write new content to fake file
	newContent := "Modified content from Go!\nWrite operation successful."
	err = os.WriteFile("/fake/test.txt", []byte(newContent), 0644)
	if err != nil {
		fmt.Printf("Error writing file: %v\n", err)
		os.Exit(1)
	}
	fmt.Println("✓ Write operation completed")

	// Test 3: Read back the written content to verify
	content, err = os.ReadFile("/fake/test.txt")
	if err != nil {
		fmt.Printf("Error reading file after write: %v\n", err)
		os.Exit(1)
	}
	if string(content) != newContent {
		fmt.Printf("ERROR: After write, expected '%s', got '%s'\n", newContent, string(content))
		os.Exit(1)
	}
	fmt.Printf("✓ Content verified after write: %s\n", strings.ReplaceAll(string(content), "\n", "\\n"))

	// Test 4: Test write to second fake file
	content2 := "Data written to second file!"
	err = os.WriteFile("/another/fake/file.txt", []byte(content2), 0644)
	if err != nil {
		fmt.Printf("Error writing second file: %v\n", err)
		os.Exit(1)
	}

	// Test 5: Read second file to verify write worked
	readContent2, err := os.ReadFile("/another/fake/file.txt")
	if err != nil {
		fmt.Printf("Error reading second file: %v\n", err)
		os.Exit(1)
	}
	if string(readContent2) != content2 {
		fmt.Printf("ERROR: Second file expected '%s', got '%s'\n", content2, string(readContent2))
		os.Exit(1)
	}
	fmt.Printf("✓ Second file write/read successful: %s\n", string(readContent2))

	// Small delay to make tracing easier to observe
	time.Sleep(100 * time.Millisecond)
	fmt.Println("✓ All tests passed - Go program finished successfully")
}
