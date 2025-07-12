import { newFS } from "./fs.ts";

type FSRequest = {
  id: string;
  operation: "read";
  path: string;
  size: number;
  offset: number;
} | {
  id: string;
  operation: "write";
  path: string;
  offset: number;
};

interface FSResponse {
  id: string;
  success: boolean;
  fd?: number;
  bytes_read?: number;
  bytes_written?: number;
  position?: number;
  error?: string;
}

interface Response {
  fs: FSResponse;
  data?: Uint8Array;
}

class WebSocketFilesystemClient {
  private ws: WebSocket | null = null;
  private fs = newFS();

  connect(url: string): Promise<void> {
    return new Promise((resolve, reject) => {
      this.ws = new WebSocket(url);

      this.ws.onopen = () => {
        console.log("Connected to WebSocket server");
        resolve();
      };

      this.ws.onerror = (error) => {
        console.error("WebSocket error:", error);
        reject(error);
      };

      this.ws.onmessage = async (event) => {
        if (event.data instanceof Blob) {
          const arrayBuffer = await event.data.arrayBuffer();
          // Parse unified binary message: [json_len(4 bytes)][json][binary_data]
          const data = new Uint8Array(arrayBuffer);
          if (data.length >= 4) {
            const jsonLen = new DataView(arrayBuffer).getUint32(0, true); // little endian

            if (data.length >= 4 + jsonLen) {
              const jsonBytes = data.slice(4, 4 + jsonLen);
              const binaryData = data.slice(4 + jsonLen);

              try {
                const jsonStr = new TextDecoder().decode(jsonBytes);
                const request: FSRequest = JSON.parse(jsonStr);
                const response = await this.handleRequest(
                  request,
                  binaryData.length > 0 ? binaryData : undefined,
                );
                this.sendResponse(
                  response.fs,
                  request.operation === "read" ? response.data : undefined,
                );
              } catch (e) {
                console.error("Failed to handle unified message:", e);
              }
            }
          }
        }
      };

      this.ws.onclose = () => {
        console.log("WebSocket connection closed");
      };
    });
  }

  async handleRequest(
    request: FSRequest,
    binaryData?: Uint8Array,
  ): Promise<Response> {
    console.log("handleRequest", request, binaryData?.length || 0);
    try {
      switch (request.operation) {
        case "read": {
          const result = this.fs.read(
            request.path,
            request.size,
            request.offset,
          );
          return {
            fs: {
              id: request.id,
              success: true,
              bytes_read: result.length,
            },
            data: result,
          };
        }
        case "write": {
          this.fs.write(
            request.path,
            request.offset,
            binaryData!,
          );
          return {
            fs: {
              id: request.id,
              success: true,
              bytes_written: binaryData!.length,
            },
          };
        }
        default:
          return {
            fs: {
              id: (request as { id: string }).id,
              success: false,
              error: "Unknown operation",
            },
          };
      }
    } catch (error) {
      return {
        fs: {
          id: request.id,
          success: false,
          error: error instanceof Error ? error.message : "Unknown error",
        },
      };
    }
  }

  private sendResponse(response: FSResponse, binaryData?: Uint8Array): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      throw new Error("WebSocket not connected");
    }
    console.log("sendResponse", response, binaryData?.length || 0);
    // Create unified binary message: [json_len(4 bytes)][json][binary_data]
    const jsonStr = JSON.stringify(response);
    const jsonBytes = new TextEncoder().encode(jsonStr);
    const jsonLen = jsonBytes.length;

    const totalLen = 4 + jsonLen + (binaryData?.length || 0);
    const message = new Uint8Array(totalLen);

    // Write JSON length (little endian)
    new DataView(message.buffer).setUint32(0, jsonLen, true);

    // Write JSON
    message.set(jsonBytes, 4);

    // Write binary data if present
    if (binaryData) {
      message.set(binaryData, 4 + jsonLen);
    }

    this.ws.send(message.buffer);
  }

  async start(): Promise<void> {
    await this.connect("ws://127.0.0.1:8080");
  }
}

// Main execution
if (import.meta.main) {
  const client = new WebSocketFilesystemClient();

  console.log("Starting filesystem client...");
  try {
    await client.start();
    console.log("Filesystem client running and waiting for requests...");

    // Keep the process alive
    setInterval(() => {}, 1000);
  } catch (error) {
    console.error("Failed to start client:", error);
    Deno.exit(1);
  }
}
