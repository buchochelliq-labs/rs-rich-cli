// Write OUT/playground.json, the stamp the page reads before it offers
// "Try it": which modules there are, the kinds each renders and the
// versions they were built from. Each module is loaded to ask it, so a
// module that does not instantiate fails the build here.
//
//     node playground/stamp.mjs OUT

import { execSync } from "node:child_process";
import { existsSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { load } from "./node.mjs";
import { runBlock } from "./cli.mjs";

const out = process.argv[2];
if (!out) throw new Error("usage: node playground/stamp.mjs OUT");

function commit() {
  if (process.env.GITHUB_SHA) return process.env.GITHUB_SHA;
  try {
    return execSync("git rev-parse HEAD", { encoding: "utf8" }).trim();
  } catch {
    return null;
  }
}

const stamp = {
  format: 1,
  commit: commit(),
  modules: {},
  xterm: { js: "xterm/xterm.js", css: "xterm/xterm.css", fit: "xterm/addon-fit.js" },
};
for (const name of ["lean", "full", "app"]) {
  const module = await load(out, name);
  const info = JSON.parse(module.info());
  const wasm = `${name}/rich_playground_bg.wasm`;
  stamp["rs-rich"] = info["rs-rich"];
  stamp["rs-rich-ext"] = info["rs-rich-ext"];
  stamp.modules[name] = {
    js: `${name}/rich_playground.js`,
    wasm,
    bytes: statSync(path.join(out, wasm)).size,
    kinds: JSON.parse(module.kinds()).map((kind) => kind.name),
  };
  // The app module's sessions, for `app` blocks (`data-app`).
  if (module.Session) stamp.modules[name].apps = JSON.parse(module.Session.apps()).map((app) => app.name);
}
// Tier 2: the CLI module, if this build has one. `rich --version` must run
// through the terminal's own runner, or the stamp is not written.
const cliWasm = path.join(out, "cli", "rich.wasm");
if (existsSync(cliWasm)) {
  const result = await runBlock(cliWasm, "rich --version", { timeout: 60000 });
  if (!result.ok) throw new Error(`cli/rich.wasm did not run: ${result.error || result.output}`);
  stamp.cli = {
    wasm: "cli/rich.wasm",
    bytes: statSync(cliWasm).size,
    version: result.output.trim().split("\n")[0],
  };
}
writeFileSync(path.join(out, "playground.json"), JSON.stringify(stamp, null, 2) + "\n");
console.log(`wrote ${path.join(out, "playground.json")}`);
