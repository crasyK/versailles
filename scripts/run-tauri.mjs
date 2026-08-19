import { spawn } from "node:child_process";
import { homedir } from "node:os";
import { delimiter, join } from "node:path";

const cargoBin = join(homedir(), ".cargo", "bin");
process.env.PATH = `${cargoBin}${delimiter}${process.env.PATH ?? ""}`;
// Windows debug builds overflow Tauri's 8MiB context-creation thread.
process.env.RUST_MIN_STACK = process.env.RUST_MIN_STACK || "33554432";

const args = process.argv.slice(2);
if (args.length === 0) {
  console.error("Usage: node scripts/run-tauri.mjs <dev|build|...> [...args]");
  process.exit(1);
}

const child = spawn("tauri", args, {
  stdio: "inherit",
  shell: true,
  env: process.env,
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exit(code ?? 1);
  }
});
