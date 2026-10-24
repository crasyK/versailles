/**
 * Write the updater manifest (`latest.json`) for a release from the built
 * NSIS installer and its updater signature (`.exe.sig`, made by
 * `tauri build` when TAURI_SIGNING_PRIVATE_KEY is set).
 *
 *   node scripts/latest-json.mjs [--installer <setup.exe>] [--version x.y.z]
 *        [--url-base <url>] [--notes <text>] [--out <file>]
 *
 * Defaults: the version from src-tauri/tauri.conf.json, the installer in
 * src-tauri/target/release/bundle/nsis/, the GitHub download URL of tag
 * v<version>, notes from that version's CHANGELOG.md section, and
 * latest.json next to the installer. Upload latest.json and the installer
 * (and its .sig) to the release; the app reads
 * releases/latest/download/latest.json.
 */
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function option(name) {
  const at = process.argv.indexOf(`--${name}`);
  return at > 0 ? process.argv[at + 1] : undefined;
}

function fail(message) {
  console.error(`latest-json: ${message}`);
  process.exit(1);
}

const conf = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const version = option("version") ?? conf.version;
const product = conf.productName ?? "Versailles";
const installer =
  option("installer") ??
  join(root, "src-tauri", "target", "release", "bundle", "nsis", `${product}_${version}_x64-setup.exe`);
if (!existsSync(installer)) fail(`no installer at ${installer} (build first, or pass --installer)`);
const sigPath = `${installer}.sig`;
if (!existsSync(sigPath)) {
  fail(`no signature at ${sigPath}: build with TAURI_SIGNING_PRIVATE_KEY set (see CONTRIBUTING, Release)`);
}
const signature = readFileSync(sigPath, "utf8").trim();

let base = option("url-base") ?? `https://github.com/crasyK/versailles/releases/download/v${version}/`;
if (!base.endsWith("/")) base += "/";
// GitHub turns spaces in asset names into dots; ours have none.
const url = base + encodeURIComponent(basename(installer));

/** The `## x.y.z — date` section of CHANGELOG.md, without its heading. */
function changelogNotes() {
  const path = join(root, "CHANGELOG.md");
  if (!existsSync(path)) return "";
  const lines = readFileSync(path, "utf8").split(/\r?\n/);
  const start = lines.findIndex((l) => l.startsWith(`## ${version} `) || l === `## ${version}`);
  if (start < 0) return "";
  const end = lines.findIndex((l, i) => i > start && l.startsWith("## "));
  return lines
    .slice(start + 1, end < 0 ? undefined : end)
    .join("\n")
    .trim();
}

const platform = { signature, url };
const manifest = {
  version,
  notes: option("notes") ?? changelogNotes(),
  pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
  // The updater looks for `windows-x86_64-nsis` first, then `windows-x86_64`.
  platforms: {
    "windows-x86_64-nsis": platform,
    "windows-x86_64": platform,
  },
};

const out = option("out") ?? join(dirname(installer), "latest.json");
writeFileSync(out, `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`latest-json: wrote ${out}\n  version ${version}\n  url ${url}`);
