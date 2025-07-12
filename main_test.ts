import { assertEquals } from "https://deno.land/std@0.208.0/assert/mod.ts";

Deno.test("Integration test: Rust binary with Go binary via WebSocket", async (t) => {
  await t.step("Setup - clean up previous builds", async () => {
    // Clean up fs directory
    try {
      await Deno.remove("./fs", { recursive: true });
    } catch {
      // Directory might not exist
    }
    await Deno.mkdir("./fs", { recursive: true });
  });

  await t.step("Compile binaries", async () => {
    await Promise.all([
      (async () => {
        const goCompile = new Deno.Command("go", {
          args: ["build", "-o", "cfc-ptrace.bin", "."],
          stdout: "piped",
          stderr: "piped",
        });

        const { code, stderr } = await goCompile.output();

        if (code !== 0) {
          const error = new TextDecoder().decode(stderr);
          throw new Error(`Go compilation failed: ${error}`);
        }

        // Verify the binary was created
        const stat = await Deno.stat("./cfc-ptrace.bin");
        assertEquals(stat.isFile, true);
      })(),

      (async () => {
        const rustCompile = new Deno.Command("cargo", {
          args: ["build", "--release"],
          stdout: "piped",
          stderr: "piped",
        });

        const { code, stderr } = await rustCompile.output();

        if (code !== 0) {
          const error = new TextDecoder().decode(stderr);
          throw new Error(`Rust compilation failed: ${error}`);
        }

        // Verify the binary was created
        const stat = await Deno.stat("./target/release/cfc-ptrace");
        assertEquals(stat.isFile, true);
      })(),
    ]);
  });

  await t.step("Run integrated test", async () => {
    // Run the Rust binary with Go binary as argument
    const rustBinary = new Deno.Command("./target/release/cfc-ptrace", {
      args: ["./cfc-ptrace.bin"],
      stdout: "piped",
      stderr: "piped",
    });
    await new Promise((r) => setTimeout(r, 1000));
    // Start the WebSocket server
    const denoServer = new Deno.Command("deno", {
      args: ["run", "-A", "./filesystem_client.ts"],
      stdout: "piped",
      stderr: "piped",
    });

    const serverProcess = denoServer.spawn();

    try {
      const { code, stdout, stderr } = await rustBinary.output();

      // Log output for debugging
      const stdoutText = new TextDecoder().decode(stdout);
      const stderrText = new TextDecoder().decode(stderr);

      console.log("Rust binary stdout:\n\n", stdoutText);
      console.log("Rust binary stderr:\n\n", stderrText);

      // The test should verify the rust binary exits with the same code as the go binary
      console.log("Rust binary completed with exit code:", code);
      assertEquals(code, 0, "Rust binary should exit with 0");
    } finally {
      // Shutdown the deno server
      serverProcess.kill("SIGTERM");

      // Close streams to prevent resource leaks
      await serverProcess.stdout.cancel();
      await serverProcess.stderr.cancel();

      await serverProcess.status;
    }
  });
});
