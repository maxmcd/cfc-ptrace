import { assertEquals, assertThrows } from "@std/assert";
import { FileStats, newFS } from "./fs.ts";

Deno.test(function chunkedWriteReadTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });
  const CHUNK_SIZE = 1024;
  const totalSize = CHUNK_SIZE * 2 + 123;
  const data = new Uint8Array(totalSize);
  for (let i = 0; i < totalSize; i++) {
    data[i] = i % 256;
  }

  // Test basic full write/read
  fs.write("bigfile.bin", 0, data);
  let readBack = fs.read("bigfile.bin", 0, totalSize);
  assertEquals(readBack, data);

  // Test write/read across first chunk boundary
  const crossChunkData = new Uint8Array(1024);
  for (let i = 0; i < crossChunkData.length; i++) {
    crossChunkData[i] = i % 256;
  }
  fs.write("bigfile.bin", CHUNK_SIZE - 100, crossChunkData);
  readBack = fs.read("bigfile.bin", CHUNK_SIZE - 100, 1024);
  assertEquals(readBack, crossChunkData);

  // Test write/read within single chunk
  const smallData = new Uint8Array(100);
  for (let i = 0; i < smallData.length; i++) {
    smallData[i] = i % 256;
  }
  fs.write("bigfile.bin", CHUNK_SIZE + 1000, smallData);
  readBack = fs.read("bigfile.bin", CHUNK_SIZE + 1000, 100);
  assertEquals(readBack, smallData);

  // Test write/read across second chunk boundary
  const endData = new Uint8Array(2048);
  for (let i = 0; i < endData.length; i++) {
    endData[i] = i % 256;
  }
  fs.write("bigfile.bin", CHUNK_SIZE * 2 - 1000, endData);
  readBack = fs.read("bigfile.bin", CHUNK_SIZE * 2 - 1000, 2048);
  assertEquals(readBack, endData);
});

Deno.test(function overwriteTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Write initial data
  const initialData = new Uint8Array([1, 2, 3, 4, 5]);
  fs.write("test.bin", 10, initialData);

  // Overwrite part of it
  const overwriteData = new Uint8Array([99, 100]);
  fs.write("test.bin", 11, overwriteData);

  // Read back and verify
  const result = fs.read("test.bin", 10, 5);
  assertEquals(result, new Uint8Array([1, 99, 100, 4, 5]));
});

Deno.test(function sparseFileTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Write data at a large offset (creates sparse file)
  const sparseOffset = 1024 * 1024; // 1MB offset
  const data = new Uint8Array([42, 43, 44]);
  fs.write("sparse.bin", sparseOffset, data);

  // Read back the data
  const result = fs.read("sparse.bin", sparseOffset, 3);
  assertEquals(result, data);
});

Deno.test(function exactChunkBoundaryTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });
  const CHUNK_SIZE = 1024;

  // Write exactly at chunk boundary
  const boundaryData = new Uint8Array([255, 254, 253]);
  fs.write("boundary.bin", CHUNK_SIZE, boundaryData);

  // Read back
  const result = fs.read("boundary.bin", CHUNK_SIZE, 3);
  assertEquals(result, boundaryData);

  // Write that ends exactly at chunk boundary
  const endBoundaryData = new Uint8Array(100);
  endBoundaryData.fill(123);
  fs.write("boundary.bin", CHUNK_SIZE - 100, endBoundaryData);

  const endResult = fs.read("boundary.bin", CHUNK_SIZE - 100, 100);
  assertEquals(endResult, endBoundaryData);
});

Deno.test(function multipleFilesTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Create multiple files with different data
  const file1Data = new Uint8Array([1, 1, 1, 1]);
  const file2Data = new Uint8Array([2, 2, 2, 2]);
  const file3Data = new Uint8Array([3, 3, 3, 3]);

  fs.write("file1.bin", 0, file1Data);
  fs.write("file2.bin", 0, file2Data);
  fs.write("file3.bin", 0, file3Data);

  // Verify each file has correct data
  assertEquals(fs.read("file1.bin", 0, 4), file1Data);
  assertEquals(fs.read("file2.bin", 0, 4), file2Data);
  assertEquals(fs.read("file3.bin", 0, 4), file3Data);
});

Deno.test(function largeMultiChunkTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });
  const CHUNK_SIZE = 1024;

  // Create data spanning 5 chunks
  const largeSize = CHUNK_SIZE * 5 + 500;
  const largeData = new Uint8Array(largeSize);
  for (let i = 0; i < largeSize; i++) {
    largeData[i] = (i * 7) % 256; // Some pattern
  }

  fs.write("large.bin", 0, largeData);

  // Read back in various chunk-sized pieces
  for (let i = 0; i < 5; i++) {
    const chunkStart = i * CHUNK_SIZE;
    const chunkEnd = Math.min(chunkStart + CHUNK_SIZE, largeSize);
    const chunkSize = chunkEnd - chunkStart;

    const readChunk = fs.read("large.bin", chunkStart, chunkSize);
    const expectedChunk = largeData.slice(chunkStart, chunkEnd);
    assertEquals(readChunk, expectedChunk);
  }

  // Read the entire file at once
  const entireFile = fs.read("large.bin", 0, largeSize);
  assertEquals(entireFile, largeData);
});

Deno.test(function emptyDataTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Write empty data
  const emptyData = new Uint8Array(0);
  fs.write("empty.bin", 0, emptyData);

  // Read zero bytes
  const result = fs.read("empty.bin", 0, 0);
  assertEquals(result.length, 0);
});

Deno.test(function errorConditionsTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Reading from non-existent file should throw
  assertThrows(
    () => {
      fs.read("nonexistent.bin", 0, 10);
    },
    Error,
    "File not found",
  );

  // Create a file with some data
  fs.write("test.bin", 0, new Uint8Array([1, 2, 3]));

  // Reading beyond available chunks should throw
  assertThrows(
    () => {
      fs.read("test.bin", 10000, 10);
    },
    Error,
    "Chunk not found at index",
  );
});

Deno.test(function partialChunkReadsTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Write some data in the middle of first chunk
  const data = new Uint8Array([10, 20, 30, 40, 50]);
  fs.write("partial.bin", 100, data);

  // Read various portions
  assertEquals(fs.read("partial.bin", 100, 1), new Uint8Array([10]));
  assertEquals(fs.read("partial.bin", 101, 2), new Uint8Array([20, 30]));
  assertEquals(fs.read("partial.bin", 102, 3), new Uint8Array([30, 40, 50]));
  assertEquals(fs.read("partial.bin", 100, 5), data);
});

Deno.test(function sequentialWritesTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Write data sequentially to build up a file
  const chunk1 = new Uint8Array([1, 1, 1, 1]);
  const chunk2 = new Uint8Array([2, 2, 2, 2]);
  const chunk3 = new Uint8Array([3, 3, 3, 3]);

  fs.write("sequential.bin", 0, chunk1);
  fs.write("sequential.bin", 4, chunk2);
  fs.write("sequential.bin", 8, chunk3);

  // Read the entire file
  const result = fs.read("sequential.bin", 0, 12);
  const expected = new Uint8Array([1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3]);
  assertEquals(result, expected);

  // Read overlapping sections
  assertEquals(fs.read("sequential.bin", 2, 4), new Uint8Array([1, 1, 2, 2]));
  assertEquals(fs.read("sequential.bin", 6, 4), new Uint8Array([2, 2, 3, 3]));
});

Deno.test(function writeAtDifferentOffsetsTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Write at various offsets out of order
  fs.write("random.bin", 1000, new Uint8Array([200]));
  fs.write("random.bin", 0, new Uint8Array([100]));
  fs.write("random.bin", 500, new Uint8Array([150]));

  // Verify each write
  assertEquals(fs.read("random.bin", 0, 1), new Uint8Array([100]));
  assertEquals(fs.read("random.bin", 500, 1), new Uint8Array([150]));
  assertEquals(fs.read("random.bin", 1000, 1), new Uint8Array([200]));
});

Deno.test(async function concurrentOperationsTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Test multiple concurrent writes to the same file
  // This will be more useful once you implement async locking
  const promises = [];

  for (let i = 0; i < 10; i++) {
    const data = new Uint8Array([i, i, i, i]);
    const offset = i * 10;
    promises.push(
      Promise.resolve().then(() => {
        fs.write("concurrent.bin", offset, data);
      }),
    );
  }

  await Promise.all(promises);

  // Verify all writes completed correctly
  for (let i = 0; i < 10; i++) {
    const result = fs.read("concurrent.bin", i * 10, 4);
    assertEquals(result, new Uint8Array([i, i, i, i]));
  }
});

Deno.test(function fileSizeGrowthTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Write at offset 0
  fs.write("growth.bin", 0, new Uint8Array([1, 2]));

  // Write at a much larger offset (should grow file size)
  fs.write("growth.bin", 5000, new Uint8Array([99]));

  // Write back in the middle (should not shrink file size)
  fs.write("growth.bin", 100, new Uint8Array([50]));

  // Verify we can read from various positions
  assertEquals(fs.read("growth.bin", 0, 2), new Uint8Array([1, 2]));
  assertEquals(fs.read("growth.bin", 100, 1), new Uint8Array([50]));
  assertEquals(fs.read("growth.bin", 5000, 1), new Uint8Array([99]));
});

Deno.test(function statTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Create a file with some data
  const testData = new Uint8Array([1, 2, 3, 4, 5]);
  fs.write("stat_test.bin", 0, testData);

  // Get file stats
  const stats = fs.stat("stat_test.bin");

  // Verify stats structure and values
  assertEquals(typeof stats.file_id, "number");
  assertEquals(stats.filename, "stat_test.bin");
  assertEquals(stats.file_size, 5);
  assertEquals(typeof stats.created_at, "string");
  assertEquals(typeof stats.modified_at, "string");

  // Verify timestamps are valid ISO strings
  const createdDate = new Date(stats.created_at);
  const modifiedDate = new Date(stats.modified_at);
  assertEquals(
    createdDate instanceof Date && !isNaN(createdDate.getTime()),
    true,
  );
  assertEquals(
    modifiedDate instanceof Date && !isNaN(modifiedDate.getTime()),
    true,
  );

  // Test stat on non-existent file
  assertThrows(
    () => {
      fs.stat("nonexistent.bin");
    },
    Error,
    "File not found",
  );
});

Deno.test(function truncateTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Create a file with data spanning multiple chunks
  const largeData = new Uint8Array(3000);
  for (let i = 0; i < largeData.length; i++) {
    largeData[i] = i % 256;
  }
  fs.write("truncate_test.bin", 0, largeData);

  // Verify initial size
  assertEquals(fs.stat("truncate_test.bin").file_size, 3000);

  // Truncate to smaller size within same chunk
  fs.truncate("truncate_test.bin", 100);
  assertEquals(fs.stat("truncate_test.bin").file_size, 100);

  // Verify data integrity after truncation
  const truncatedData = fs.read("truncate_test.bin", 0, 100);
  assertEquals(truncatedData, largeData.slice(0, 100));

  // Truncate to size crossing chunk boundary
  fs.write("truncate_test.bin", 0, largeData); // Restore original data
  fs.truncate("truncate_test.bin", 1500);
  assertEquals(fs.stat("truncate_test.bin").file_size, 1500);

  const truncatedData2 = fs.read("truncate_test.bin", 0, 1500);
  assertEquals(truncatedData2, largeData.slice(0, 1500));

  // Truncate to size 0
  fs.truncate("truncate_test.bin", 0);
  assertEquals(fs.stat("truncate_test.bin").file_size, 0);

  // Truncate to larger size (should just update metadata)
  fs.truncate("truncate_test.bin", 2000);
  assertEquals(fs.stat("truncate_test.bin").file_size, 2000);

  // Test truncate on non-existent file
  assertThrows(
    () => {
      fs.truncate("nonexistent.bin", 100);
    },
    Error,
    "File not found",
  );
});

Deno.test(function unlinkTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Create multiple files
  fs.write("file1.bin", 0, new Uint8Array([1, 1, 1]));
  fs.write("file2.bin", 0, new Uint8Array([2, 2, 2]));
  fs.write("file3.bin", 0, new Uint8Array([3, 3, 3]));

  // Verify all files exist
  assertEquals(fs.stat("file1.bin").filename, "file1.bin");
  assertEquals(fs.stat("file2.bin").filename, "file2.bin");
  assertEquals(fs.stat("file3.bin").filename, "file3.bin");

  // Delete one file
  fs.unlink("file2.bin");

  // Verify deleted file is gone
  assertThrows(
    () => {
      fs.stat("file2.bin");
    },
    Error,
    "File not found",
  );

  assertThrows(
    () => {
      fs.read("file2.bin", 0, 1);
    },
    Error,
    "File not found",
  );

  // Verify other files are still accessible
  assertEquals(fs.read("file1.bin", 0, 3), new Uint8Array([1, 1, 1]));
  assertEquals(fs.read("file3.bin", 0, 3), new Uint8Array([3, 3, 3]));

  // Test unlink on non-existent file
  assertThrows(
    () => {
      fs.unlink("nonexistent.bin");
    },
    Error,
    "File not found",
  );
});

Deno.test(function renameTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Create a file with some data
  const testData = new Uint8Array([10, 20, 30, 40, 50]);
  fs.write("original_name.bin", 0, testData);

  // Verify original file exists
  assertEquals(fs.stat("original_name.bin").filename, "original_name.bin");
  assertEquals(fs.read("original_name.bin", 0, 5), testData);

  // Rename the file
  fs.rename("original_name.bin", "new_name.bin");

  // Verify old name is gone
  assertThrows(
    () => {
      fs.stat("original_name.bin");
    },
    Error,
    "File not found",
  );

  assertThrows(
    () => {
      fs.read("original_name.bin", 0, 1);
    },
    Error,
    "File not found",
  );

  // Verify new name exists with same data
  assertEquals(fs.stat("new_name.bin").filename, "new_name.bin");
  assertEquals(fs.read("new_name.bin", 0, 5), testData);

  // Test rename to existing file (should fail)
  fs.write("another_file.bin", 0, new Uint8Array([99]));
  assertThrows(
    () => {
      fs.rename("new_name.bin", "another_file.bin");
    },
    Error,
    "Destination file already exists",
  );

  // Test rename non-existent file
  assertThrows(
    () => {
      fs.rename("nonexistent.bin", "some_name.bin");
    },
    Error,
    "File not found",
  );
});

Deno.test(function syscallIntegrationTest() {
  const fs = newFS({ location: ":memory:", chunkSize: 1024 });

  // Create a file, modify it, and test all operations together
  const initialData = new Uint8Array(2048);
  for (let i = 0; i < initialData.length; i++) {
    initialData[i] = i % 256;
  }

  // Write initial data
  fs.write("integration_test.bin", 0, initialData);

  // Check stats
  let stats = fs.stat("integration_test.bin");
  assertEquals(stats.file_size, 2048);

  // Truncate file
  fs.truncate("integration_test.bin", 1000);
  stats = fs.stat("integration_test.bin");
  assertEquals(stats.file_size, 1000);

  // Verify truncated data is correct
  const truncatedData = fs.read("integration_test.bin", 0, 1000);
  assertEquals(truncatedData, initialData.slice(0, 1000));

  // Rename file
  fs.rename("integration_test.bin", "renamed_test.bin");

  // Verify new name works
  stats = fs.stat("renamed_test.bin");
  assertEquals(stats.filename, "renamed_test.bin");
  assertEquals(stats.file_size, 1000);

  // Verify data is still correct
  const renamedData = fs.read("renamed_test.bin", 0, 1000);
  assertEquals(renamedData, initialData.slice(0, 1000));

  // Finally, delete the file
  fs.unlink("renamed_test.bin");

  // Verify file is completely gone
  assertThrows(
    () => {
      fs.stat("renamed_test.bin");
    },
    Error,
    "File not found",
  );
});
