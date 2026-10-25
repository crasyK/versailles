/**
 * `.sdk/versailles.d.ts` must describe exactly the SDK's public surface:
 * fails when a public SDK name has no type, a type has no SDK name, a name
 * the snapshot lists as deprecated is not marked `@deprecated`, or the
 * declarations do not compile.
 *
 * The SDK side comes from running it (`sdk-surface.mjs`); the type side from
 * the TypeScript checker walking `versailles` (and `versailles.for()`).
 */
import ts from "typescript";
import { join } from "node:path";
import { root, read, sdkSurface, SDK_TYPES } from "./sdk-surface.mjs";

const file = join(root, SDK_TYPES);
const program = ts.createProgram([file], {
  noEmit: true,
  strict: true,
  target: ts.ScriptTarget.ES2022,
  lib: ["lib.es2022.d.ts", "lib.dom.d.ts"],
  types: [],
});
const problems = [];

for (const d of ts.getPreEmitDiagnostics(program)) {
  const where = d.file ? `${d.file.fileName}:${d.file.getLineAndCharacterOfPosition(d.start ?? 0).line + 1}` : "";
  problems.push(`${where} ${ts.flattenDiagnosticMessageText(d.messageText, "\n")}`);
}

const checker = program.getTypeChecker();
const source = program.getSourceFile(file);
if (!source) throw new Error(`cannot load ${SDK_TYPES}`);

/** The type of `declare global { var <name> }`. */
function globalVar(name) {
  const sym = checker.getSymbolsInScope(source, ts.SymbolFlags.Variable).find((s) => s.name === name);
  if (!sym) throw new Error(`${SDK_TYPES} declares no global '${name}'`);
  return { sym, type: checker.getTypeOfSymbolAtLocation(sym, source) };
}

const deprecatedTyped = new Set();
const typed = new Set();

function isDeprecated(sym) {
  return sym.getJsDocTags(checker).some((t) => t.name === "deprecated");
}

/** Leaves: functions and primitives. Object types without call signatures are walked. */
function walk(type, prefix, depth) {
  if (depth > 6) return;
  for (const prop of checker.getPropertiesOfType(type)) {
    const decl = prop.valueDeclaration ?? prop.declarations?.[0];
    if (!decl) continue;
    const t = checker.getTypeOfSymbolAtLocation(prop, decl);
    const path = prefix ? `${prefix}.${prop.name}` : prop.name;
    if (isDeprecated(prop)) deprecatedTyped.add(path);
    const calls = t.getCallSignatures();
    if (calls.length) {
      typed.add(path);
      if (prop.name === "for" && !prefix) {
        walk(calls[0].getReturnType(), "for()", depth + 1);
      }
      continue;
    }
    if (t.flags & ts.TypeFlags.Object && checker.getPropertiesOfType(t).length) {
      walk(t, path, depth + 1);
      continue;
    }
    typed.add(path);
  }
}

const { type } = globalVar("versailles");
walk(type, "", 0);
typed.add("<global>.versailles");
const deck = globalVar("deck");
if (isDeprecated(deck.sym)) deprecatedTyped.add("<global>.deck");
typed.add("<global>.deck");

const actual = new Set(sdkSurface());
for (const name of actual) if (!typed.has(name)) problems.push(`SDK has '${name}' but ${SDK_TYPES} does not type it`);
for (const name of typed) if (!actual.has(name)) problems.push(`${SDK_TYPES} types '${name}' but the SDK has no such name`);

// Deprecated names in the snapshot must say so in the types (and only those).
const snapshot = JSON.parse(read("docs/api-surface.json"));
const deprecated = new Set(snapshot.sdkDeprecated ?? []);
const expand = (n) => (n.startsWith("<global>") ? [n] : [n, `for().${n}`]);
for (const name of deprecated) {
  for (const path of expand(name)) {
    if (actual.has(path) && !deprecatedTyped.has(path)) problems.push(`'${path}' is deprecated but its type has no @deprecated tag`);
  }
}
for (const path of deprecatedTyped) {
  const base = path.replace(/^for\(\)\./, "");
  if (!deprecated.has(base)) problems.push(`'${path}' is @deprecated in the types but not in docs/api-surface.json sdkDeprecated`);
}

if (problems.length) {
  console.error(`check-sdk-types: ${problems.length} problem(s)\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
console.log(`check-sdk-types: ok (${actual.size} names typed, ${deprecated.size} deprecated)`);
