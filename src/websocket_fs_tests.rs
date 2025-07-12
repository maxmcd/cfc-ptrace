#[cfg(test)]
mod tests {
    use crate::websocket_fs::*;
    use std::time::Duration;
    use tokio::process::Command;
    use tokio::time::sleep;

    /// Test environment that manages server and client lifecycle
    struct TestEnvironment {
        server: Option<WebSocketFileSystem>,
        client_process: Option<tokio::process::Child>,
        port: u16,
    }

    impl TestEnvironment {
        fn new(port: u16) -> Self {
            TestEnvironment {
                server: None,
                client_process: None,
                port,
            }
        }

        async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            use tokio::sync::mpsc;
            use std::sync::Arc;
            
            // Create server
            let temp_dir = tempfile::tempdir()?;
            let cache_dir = temp_dir.path().to_str().unwrap().to_string();
            let server = Arc::new(tokio::sync::Mutex::new(WebSocketFileSystem::new(cache_dir)));

            let port = self.port;
            
            // Create a channel to signal when server is ready to accept connections
            let (server_ready_tx, mut server_ready_rx) = mpsc::channel(1);
            
            // Clone the server for the task
            let server_clone = Arc::clone(&server);
            
            // Start the WebSocket server in a background task
            let server_handle = tokio::spawn(async move {
                // Signal that we're starting the server
                let _ = server_ready_tx.send(()).await;
                
                // This will block until a client connects
                let result = {
                    let mut server_guard = server_clone.lock().await;
                    server_guard.start_server(port).await
                };
                
                match result {
                    Ok(()) => Ok(()),
                    Err(e) => Err(format!("Server error: {}", e)),
                }
            });

            // Wait for server to signal it's starting
            server_ready_rx.recv().await.ok_or("Server start signal failed")?;
            
            // Give the server time to bind to the port
            sleep(Duration::from_millis(500)).await;

            // Now start the Deno client with in-memory database
            let client_process = Command::new("deno")
                .args(&[
                    "run",
                    "-A",
                    "./filesystem_client.ts",
                    "--memory",
                    "--port",
                    &self.port.to_string(),
                ])
                .current_dir("/home/maxm/go/src/github.com/maxmcd/cfc-ptrace")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;

            self.client_process = Some(client_process);

            // Wait for the server to complete its setup and accept the connection
            let server_result = server_handle.await?;
            match server_result {
                Ok(()) => {
                    // Extract server from Arc<Mutex<>>
                    let server = Arc::try_unwrap(server).map_err(|_| "Failed to unwrap server")?;
                    self.server = Some(server.into_inner());
                    println!("Server and client connected successfully on port {}", self.port);
                    Ok(())
                }
                Err(e) => Err(e.into()),
            }
        }

        async fn send_filesystem_request(&mut self, request: FSRequest) -> Result<FSResponseWithBinary, Box<dyn std::error::Error>> {
            if let Some(server) = &self.server {
                server.send_request(request).await
            } else {
                Err("Server not initialized".into())
            }
        }

        async fn send_filesystem_request_with_data(&mut self, request: FSRequest, data: &[u8]) -> Result<FSResponseWithBinary, Box<dyn std::error::Error>> {
            if let Some(server) = &self.server {
                server.send_request_with_binary(request, data).await
            } else {
                Err("Server not initialized".into())
            }
        }
    }

    impl Drop for TestEnvironment {
        fn drop(&mut self) {
            if let Some(mut process) = self.client_process.take() {
                let _ = process.kill();
                // Force kill if still running
                std::thread::sleep(std::time::Duration::from_millis(100));
                let _ = process.kill();
            }
        }
    }
    
    impl TestEnvironment {
        async fn cleanup(&mut self) {
            if let Some(mut process) = self.client_process.take() {
                // Try graceful termination first
                let _ = process.kill();
                
                // Wait briefly for graceful termination
                if let Ok(result) = tokio::time::timeout(
                    std::time::Duration::from_millis(500),
                    process.wait()
                ).await {
                    if result.is_ok() {
                        return; // Process terminated gracefully
                    }
                }
                
                // Force kill if still running
                let _ = process.kill();
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(1),
                    process.wait()
                ).await;
            }
        }
    }

    #[tokio::test]
    async fn test_basic_write_operation() {
        let mut env = TestEnvironment::new(8090);
        env.start().await.unwrap();

        let test_data = b"Hello, WebSocket filesystem!";
        let write_request = FSRequest::Write {
            id: uuid::Uuid::new_v4().to_string(),
            path: "/test/write_test.txt".to_string(),
            offset: 0,
            data: test_data.to_vec(),
        };

        let response = env.send_filesystem_request_with_data(write_request, test_data).await.unwrap();
        
        assert!(response.response.success, "Write operation should succeed");
        assert_eq!(response.response.bytes_written, Some(test_data.len()));
        assert!(response.response.error.is_none(), "Should not have error");
        
        // Cleanup
        env.cleanup().await;
    }

    #[tokio::test]
    async fn test_basic_read_operation() {
        let mut env = TestEnvironment::new(8091);
        env.start().await.unwrap();

        // First write some data
        let test_data = b"Data to be read back";
        let write_request = FSRequest::Write {
            id: uuid::Uuid::new_v4().to_string(),
            path: "/test/read_test.txt".to_string(),
            offset: 0,
            data: test_data.to_vec(),
        };

        let write_response = env.send_filesystem_request_with_data(write_request, test_data).await.unwrap();
        assert!(write_response.response.success, "Write should succeed");

        // Now read it back
        let read_request = FSRequest::Read {
            id: uuid::Uuid::new_v4().to_string(),
            path: "/test/read_test.txt".to_string(),
            size: test_data.len(),
            offset: 0,
        };

        let read_response = env.send_filesystem_request(read_request).await.unwrap();
        
        assert!(read_response.response.success, "Read operation should succeed");
        assert_eq!(read_response.response.bytes_read, Some(test_data.len()));
        assert!(read_response.binary.is_some(), "Should have binary data");
        assert_eq!(read_response.binary.unwrap(), test_data);
    }

    #[tokio::test]
    async fn test_read_nonexistent_file() {
        let mut env = TestEnvironment::new(8092);
        env.start().await.unwrap();

        let read_request = FSRequest::Read {
            id: uuid::Uuid::new_v4().to_string(),
            path: "/test/nonexistent_file.txt".to_string(),
            size: 100,
            offset: 0,
        };

        let response = env.send_filesystem_request(read_request).await.unwrap();
        
        assert!(!response.response.success, "Read should fail for nonexistent file");
        assert!(response.response.error.is_some(), "Should have error message");
        assert!(response.response.error.unwrap().contains("File not found"));
    }

    #[tokio::test]
    async fn test_write_then_read_consistency() {
        let mut env = TestEnvironment::new(8093);
        env.start().await.unwrap();

        let test_data = b"Consistency test data - should be exactly the same when read back!";
        let file_path = "/test/consistency_test.txt";

        // Write data
        let write_request = FSRequest::Write {
            id: uuid::Uuid::new_v4().to_string(),
            path: file_path.to_string(),
            offset: 0,
            data: test_data.to_vec(),
        };

        let write_response = env.send_filesystem_request_with_data(write_request, test_data).await.unwrap();
        assert!(write_response.response.success, "Write should succeed");

        // Read data back
        let read_request = FSRequest::Read {
            id: uuid::Uuid::new_v4().to_string(),
            path: file_path.to_string(),
            size: test_data.len(),
            offset: 0,
        };

        let read_response = env.send_filesystem_request(read_request).await.unwrap();
        assert!(read_response.response.success, "Read should succeed");
        assert_eq!(read_response.binary.unwrap(), test_data, "Read data should match written data");
    }

    #[tokio::test]
    async fn test_large_file_operations() {
        let mut env = TestEnvironment::new(8094);
        env.start().await.unwrap();

        // Create a large file (larger than typical chunk size)
        let large_data = vec![0x42u8; 100 * 1024]; // 100KB
        let file_path = "/test/large_file.bin";

        // Write large file
        let write_request = FSRequest::Write {
            id: uuid::Uuid::new_v4().to_string(),
            path: file_path.to_string(),
            offset: 0,
            data: large_data.clone(),
        };

        let write_response = env.send_filesystem_request_with_data(write_request, &large_data).await.unwrap();
        assert!(write_response.response.success, "Large file write should succeed");

        // Read large file back
        let read_request = FSRequest::Read {
            id: uuid::Uuid::new_v4().to_string(),
            path: file_path.to_string(),
            size: large_data.len(),
            offset: 0,
        };

        let read_response = env.send_filesystem_request(read_request).await.unwrap();
        assert!(read_response.response.success, "Large file read should succeed");
        assert_eq!(read_response.binary.unwrap(), large_data, "Large file data should match");
    }

    #[tokio::test]
    async fn test_multiple_file_operations() {
        let mut env = TestEnvironment::new(8095);
        env.start().await.unwrap();

        let test_files = vec![
            ("/test/file1.txt", b"Content of file 1"),
            ("/test/file2.txt", b"Content of file 2"),
            ("/test/file3.txt", b"Content of file 3"),
        ];

        // Write all files
        for (path, data) in &test_files {
            let write_request = FSRequest::Write {
                id: uuid::Uuid::new_v4().to_string(),
                path: path.to_string(),
                offset: 0,
                data: data.to_vec(),
            };

            let response = env.send_filesystem_request_with_data(write_request, *data).await.unwrap();
            assert!(response.response.success, "Write should succeed for {}", path);
        }

        // Read all files back and verify
        for (path, expected_data) in &test_files {
            let read_request = FSRequest::Read {
                id: uuid::Uuid::new_v4().to_string(),
                path: path.to_string(),
                size: expected_data.len(),
                offset: 0,
            };

            let response = env.send_filesystem_request(read_request).await.unwrap();
            assert!(response.response.success, "Read should succeed for {}", path);
            assert_eq!(response.binary.unwrap(), *expected_data, "Data should match for {}", path);
        }
    }

    #[tokio::test]
    async fn test_offset_write_read() {
        let mut env = TestEnvironment::new(8096);
        env.start().await.unwrap();

        let file_path = "/test/offset_test.txt";
        let initial_data = b"Initial data content";
        let offset_data = b"INSERTED";
        let write_offset = 8; // Insert at position 8

        // Write initial data
        let write_request1 = FSRequest::Write {
            id: uuid::Uuid::new_v4().to_string(),
            path: file_path.to_string(),
            offset: 0,
            data: initial_data.to_vec(),
        };

        let response1 = env.send_filesystem_request_with_data(write_request1, initial_data).await.unwrap();
        assert!(response1.response.success, "Initial write should succeed");

        // Write at offset
        let write_request2 = FSRequest::Write {
            id: uuid::Uuid::new_v4().to_string(),
            path: file_path.to_string(),
            offset: write_offset,
            data: offset_data.to_vec(),
        };

        let response2 = env.send_filesystem_request_with_data(write_request2, offset_data).await.unwrap();
        assert!(response2.response.success, "Offset write should succeed");

        // Read back and verify the data was written at correct offset
        let read_request = FSRequest::Read {
            id: uuid::Uuid::new_v4().to_string(),
            path: file_path.to_string(),
            size: initial_data.len().max(write_offset + offset_data.len()),
            offset: 0,
        };

        let read_response = env.send_filesystem_request(read_request).await.unwrap();
        assert!(read_response.response.success, "Read should succeed");
        
        let read_data = read_response.binary.unwrap();
        // Verify offset data was written at correct position
        assert_eq!(&read_data[write_offset..write_offset + offset_data.len()], offset_data);
    }

    #[tokio::test]
    async fn test_sequential_operations() {
        let mut env = TestEnvironment::new(8097);
        env.start().await.unwrap();

        let file_path = "/test/sequential_test.txt";
        let test_data = b"Sequential operation test data";

        // Perform multiple write operations sequentially
        for i in 0..5 {
            let write_request = FSRequest::Write {
                id: uuid::Uuid::new_v4().to_string(),
                path: format!("{}{}", file_path, i),
                offset: 0,
                data: test_data.to_vec(),
            };

            let result = env.send_filesystem_request_with_data(write_request, test_data).await;
            assert!(result.is_ok(), "Write {} should succeed", i);
            assert!(result.unwrap().response.success, "Write {} should be successful", i);
        }

        // Verify all files were written correctly
        for i in 0..5 {
            let read_request = FSRequest::Read {
                id: uuid::Uuid::new_v4().to_string(),
                path: format!("{}{}", file_path, i),
                size: test_data.len(),
                offset: 0,
            };

            let response = env.send_filesystem_request(read_request).await.unwrap();
            assert!(response.response.success, "Read {} should succeed", i);
            assert_eq!(response.binary.unwrap(), test_data, "Data {} should match", i);
        }
    }
}