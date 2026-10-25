/**
 * The SDK against a fake host: one error shape (`Error` with `code`, and
 * `String(err)` still the host's bare message), deprecated aliases calling
 * the same command and warning once, state hooks delivering the current
 * value first, and the hook namespaces present at the top level.
 */
import { readFileSync } from "node:fs";
import vm from "node:vm";
import assert from "node:assert/strict";
import { join } from "node:path";
import { root, SDK } from "./sdk-surface.mjs";

const sdkPath = join(root, SDK);
const calls = [];
const warns = [];
const sandbox = {
  console: { warn: (m) => warns.push(m), error() {}, log() {} },
  setTimeout,
  URLSearchParams,
  __VERSAILLES_WIDGET_ID__: "qs",
  __TAURI__: {
    core: {
      async invoke(cmd, args) {
        calls.push([cmd, args]);
        if (cmd === "media_now") return { title: "t" };
        if (cmd === "quick_set" && args.action.kind === "nightLight") throw "display: night light is not supported";
        if (cmd === "sense_now" && args.hook === "radios") throw "hook 'radios' not declared on 'qs'";
        if (cmd === "tray_list") return { active: true, icons: [] };
        return { ok: true };
      },
    },
    event: { listen: async () => () => {} },
  },
};
vm.createContext(sandbox);
vm.runInContext(readFileSync(sdkPath, "utf8"), sandbox);
const v = sandbox.versailles.for("qs");
assert.equal(sandbox.deck, sandbox.versailles);

// Host string rejection -> Error with code; String(err) unchanged.
let err = await v.display.setNightLight(true).catch((e) => e);
assert.equal(Object.prototype.toString.call(err), "[object Error]");
assert.equal(err.code, "host-error");
assert.equal(err.message, "display: night light is not supported");
assert.equal(String(err), "display: night light is not supported");
assert.equal(`${err}`, "display: night light is not supported");
err = await v.radios.now().catch((e) => e);
assert.equal(err.code, "hook-undeclared");

// Media state: on("media") gets the current value first.
const seen = [];
await v.media.onChange((d, p) => seen.push([d, p]));
await new Promise((r) => setTimeout(r, 10));
assert.equal(seen[0][0].title, "t");
assert.equal(seen[0][1].initial, true);

// Aliases call the same command and warn once.
calls.length = 0;
await v.display.project("extend");
await v.display.project("extend");
await v.display.setProjection("pc");
assert.deepEqual(calls.map((c) => c[1].action.kind), ["project", "project", "project"]);
assert.equal(warns.filter((w) => w.includes("display.project")).length, 1);
await v.audioControl.setOutput("a");
await v.audioControl.setDefaultOutput("a");
assert.deepEqual(calls.slice(-2).map((c) => c[1].action.kind), ["output", "output"]);
assert.deepEqual(await v.tray.now(), await v.tray.state());
assert.equal(typeof v.vpn.connect, "function");
assert.equal(typeof v.accessibility.set, "function");

// Unknown push -> unknown-event.
err = await v.on("nope", () => {}).catch((e) => e);
assert.equal(err.code, "unknown-event");

// No host: no-host code.
const bare = { console, setTimeout, URLSearchParams };
vm.createContext(bare);
vm.runInContext(readFileSync(sdkPath, "utf8"), bare);
err = await bare.versailles.invoke("x").catch((e) => e);
assert.equal(err.code, "no-host");
assert.equal(String(err), "Error: Versailles API not available in this window");
console.log("check-sdk-behaviour: ok (error shape, aliases, state pushes)");
