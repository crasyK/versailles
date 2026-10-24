import { spawn } from "node:child_process";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const cargoBin = join(homedir(), ".cargo", "bin");
process.env.PATH = `${cargoBin}${delimiter}${process.env.PATH ?? ""}`;
// Windows debug builds overflow Tauri's 8MiB context-creation thread.
process.env.RUST_MIN_STACK = process.env.RUST_MIN_STACK || "33554432";

const args = process.argv.slice(2);
if (args.length === 0) {
  console.error("Usage: node scripts/run-tauri.mjs <dev|build|...> [...args]");
  process.exit(1);
}

const quote = (p) => (/\s/.test(p) ? `"${p}"` : p);
const readJson = (p) => JSON.parse(readFileSync(p, "utf8"));
let tempConfig = null;

/**
 * `build` gets config overlays (merged before any `--config` of the caller,
 * so the caller's win):
 * - test builds (`-- --profile release-test`): `tauri.test.conf.json`, which
 *   lets the updater use a plain-http endpoint for a local test server
 *   (`VERSAILLES_UPDATE_URL`). Never signed.
 * - `VERSAILLES_SIGN=1` on a release build: `tauri.release.conf.json`
 *   (Authenticode: certificate thumbprint, SHA-256, Certum timestamp). Without
 *   it, release builds stay unsigned, so a build never needs the certificate.
 * - Updater artifacts (`.sig` next to the installer) need the updater key
 *   pair: without `TAURI_SIGNING_PRIVATE_KEY`, or while tauri.conf.json still
 *   has the placeholder public key, they are turned off so the build works.
 */
function buildOverlays(rest) {
  const overlays = [];
  const test = rest.includes("release-test");

  // The public key the build will end up with (the caller's --config may set it).
  let pubkey = readJson(join(root, "src-tauri", "tauri.conf.json")).plugins?.updater?.pubkey ?? "";
  for (let i = 0; i < rest.length; i++) {
    if (rest[i] === "--") break;
    if (rest[i] !== "--config" && rest[i] !== "-c") continue;
    const value = rest[i + 1] ?? "";
    try {
      const conf = value.trim().startsWith("{") ? JSON.parse(value) : readJson(value);
      pubkey = conf.plugins?.updater?.pubkey ?? pubkey;
    } catch {
      // Not readable here; tauri reports it.
    }
  }

  if (test) {
    overlays.push(join(root, "src-tauri", "tauri.test.conf.json"));
    if (process.env.VERSAILLES_SIGN === "1") {
      console.warn("run-tauri: test builds are never signed; VERSAILLES_SIGN is ignored.");
    }
  } else if (process.env.VERSAILLES_SIGN === "1") {
    const release = join(root, "src-tauri", "tauri.release.conf.json");
    const thumbprint = readJson(release).bundle?.windows?.certificateThumbprint ?? "";
    if (!/^[0-9A-Fa-f]{40}$/.test(thumbprint)) {
      console.error(
        "run-tauri: VERSAILLES_SIGN=1, but src-tauri/tauri.release.conf.json has no certificate thumbprint yet (see CONTRIBUTING, Signing).",
      );
      process.exit(1);
    }
    overlays.push(release);
  }

  const realKey = pubkey.length > 40 && !pubkey.includes("REPLACE_WITH") && !/\s/.test(pubkey);
  // The Tauri CLI reads only TAURI_SIGNING_PRIVATE_KEY (the key or a path to it).
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY && process.env.TAURI_SIGNING_PRIVATE_KEY_PATH) {
    process.env.TAURI_SIGNING_PRIVATE_KEY = process.env.TAURI_SIGNING_PRIVATE_KEY_PATH;
  }
  const privateKey = process.env.TAURI_SIGNING_PRIVATE_KEY;
  if (!realKey || !privateKey) {
    console.warn(
      realKey
        ? "run-tauri: TAURI_SIGNING_PRIVATE_KEY(_PATH) is not set: no updater artifacts (.sig) for this build."
        : "run-tauri: no updater public key in tauri.conf.json yet: no updater artifacts (.sig) for this build.",
    );
    tempConfig = join(tmpdir(), `versailles-tauri-${process.pid}.conf.json`);
    writeFileSync(tempConfig, JSON.stringify({ bundle: { createUpdaterArtifacts: false } }));
    overlays.push(tempConfig);
  }
  return overlays.flatMap((p) => ["--config", quote(p)]);
}

const finalArgs =
  args[0] === "build" ? ["build", ...buildOverlays(args.slice(1)), ...args.slice(1)] : args;

const child = spawn("tauri", finalArgs, {
  stdio: "inherit",
  shell: true,
  env: process.env,
});

child.on("exit", (code, signal) => {
  if (tempConfig && existsSync(tempConfig)) rmSync(tempConfig);
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exit(code ?? 1);
  }
});
