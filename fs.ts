import { DatabaseSync } from "node:sqlite";

const CHUNK_SIZE = 512 * 1024;

export function newFS(options?: { location?: string; chunkSize?: number }) {
  const database = new DatabaseSync(options?.location || "fs.db");
  database.exec(`
    CREATE TABLE IF NOT EXISTS file_chunks (
      file_id INTEGER,
      chunk_index INTEGER,         -- chunk number (offset / CHUNK_SIZE)
      chunk_data BLOB,
      chunk_size INTEGER,          -- actual size (last chunk may be partial)
      PRIMARY KEY (file_id, chunk_index)
    );

    CREATE TABLE IF NOT EXISTS files (
      file_id INTEGER PRIMARY KEY,
      filename TEXT,
      file_size INTEGER,
      created_at TIMESTAMP,
      modified_at TIMESTAMP
    );

    CREATE INDEX IF NOT EXISTS idx_files_lookup
      ON files(filename);

    CREATE INDEX IF NOT EXISTS idx_file_chunks_lookup
      ON file_chunks(file_id, chunk_index);
  `);

  return new FS(database, options?.chunkSize || CHUNK_SIZE);
}

class FS {
  constructor(private database: DatabaseSync, private chunkSize: number) {}

  createFile(path: string) {
    this.database.prepare(
      "INSERT INTO files (filename, file_size, created_at, modified_at) VALUES (?, ?, ?, ?)",
    ).run(path, 0, new Date().toISOString(), new Date().toISOString());
  }

  write(path: string, offset: number, data: Uint8Array) {
    let fileId = this.database.prepare(
      "SELECT file_id FROM files WHERE filename = ?",
    ).get(path) as { file_id: number } | undefined;
    if (!fileId) {
      this.createFile(path);
      fileId = this.database.prepare(
        "SELECT file_id FROM files WHERE filename = ?",
      ).get(path) as { file_id: number };
    }

    let currentOffset = offset;
    let remainingData = data;
    let chunksProcessed = 0;

    while (remainingData.length > 0) {
      const chunkIndex = Math.floor(currentOffset / this.chunkSize);
      const offsetInChunk = currentOffset % this.chunkSize;
      const bytesToWrite = Math.min(
        this.chunkSize - offsetInChunk,
        remainingData.length,
      );
      const newChunkData = remainingData.slice(0, bytesToWrite);

      // Get existing chunk data if it exists
      const existingChunk = this.database.prepare(
        "SELECT chunk_data FROM file_chunks WHERE file_id = ? AND chunk_index = ?",
      ).get(fileId.file_id, chunkIndex) as
        | { chunk_data: Uint8Array }
        | undefined;

      let finalChunkData: Uint8Array;

      if (existingChunk) {
        // Merge new data with existing chunk
        const existing = existingChunk.chunk_data;
        const maxSize = Math.max(existing.length, offsetInChunk + bytesToWrite);
        finalChunkData = new Uint8Array(maxSize);

        // Copy existing data
        finalChunkData.set(existing);

        // Overwrite with new data at the correct offset
        finalChunkData.set(newChunkData, offsetInChunk);
      } else {
        // Create new chunk with appropriate size
        const chunkSize = offsetInChunk + bytesToWrite;
        finalChunkData = new Uint8Array(chunkSize);
        finalChunkData.set(newChunkData, offsetInChunk);
      }

      this.database.prepare(
        "INSERT OR REPLACE INTO file_chunks (file_id, chunk_index, chunk_data, chunk_size) VALUES (?, ?, ?, ?)",
      ).run(fileId.file_id, chunkIndex, finalChunkData, finalChunkData.length);

      currentOffset += bytesToWrite;
      remainingData = remainingData.slice(bytesToWrite);
      chunksProcessed++;
    }

    // Get current file size and update to the maximum of current size and new end position
    const currentFile = this.database.prepare(
      "SELECT file_size FROM files WHERE file_id = ?",
    ).get(fileId.file_id) as { file_size: number };
    const newFileSize = Math.max(currentFile.file_size, offset + data.length);

    this.database.prepare(
      "UPDATE files SET file_size = ?, modified_at = ? WHERE file_id = ?",
    ).run(newFileSize, new Date().toISOString(), fileId.file_id);
  }
  read(path: string, offset: number, size: number) {
    const fileId = this.database.prepare(
      "SELECT file_id FROM files WHERE filename = ?",
    ).get(path) as { file_id: number };
    if (!fileId) {
      throw new Error("File not found");
    }

    // Handle zero-length reads
    if (size === 0) {
      return new Uint8Array(0);
    }

    // Calculate the range of chunks we need
    const startChunkIndex = Math.floor(offset / this.chunkSize);
    const endChunkIndex = Math.floor((offset + size - 1) / this.chunkSize);

    // Fetch all required chunks in a single query
    const chunks = this.database.prepare(
      "SELECT chunk_index, chunk_data FROM file_chunks WHERE file_id = ? AND chunk_index >= ? AND chunk_index <= ? ORDER BY chunk_index",
    ).all(fileId.file_id, startChunkIndex, endChunkIndex) as {
      chunk_index: number;
      chunk_data: Uint8Array;
    }[];

    if (chunks.length === 0) {
      throw new Error(`Chunk not found at index ${startChunkIndex}`);
    }

    const result = new Uint8Array(size);
    let resultOffset = 0;

    for (const chunk of chunks) {
      const chunkStartByte = chunk.chunk_index * this.chunkSize;
      const chunkEndByte = chunkStartByte + chunk.chunk_data.length;

      // Calculate overlap between requested range and this chunk
      const overlapStart = Math.max(offset, chunkStartByte);
      const overlapEnd = Math.min(offset + size, chunkEndByte);

      if (overlapStart < overlapEnd) {
        const chunkOffset = overlapStart - chunkStartByte;
        const overlapSize = overlapEnd - overlapStart;

        const chunkSlice = chunk.chunk_data.slice(
          chunkOffset,
          chunkOffset + overlapSize,
        );
        result.set(chunkSlice, resultOffset);
        resultOffset += overlapSize;
      }
    }

    return result;
  }
}
