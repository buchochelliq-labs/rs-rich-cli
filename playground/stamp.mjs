// Write OUT/playground.json, the stamp the page reads before it offers
// "Try it": which modules there are, the kinds each renders and the
// versions they were built from. Each module is loaded to ask it, so a
// module that does not instantiate fails the build here.
//
//     node playground/stamp.mjs OUT

import { execSync } from "node:child_process";
import { statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { load } from "./node.mjs";

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

const stamp = { format: 1, commit: commit(), modules: {}, xterm: { js: "xterm/xterm.js", css: "xterm/xterm.css" } };
for (const name of ["lean", "full"]) {
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
}
writeFileSync(path.join(out, "playground.json"), JSON.stringify(stamp, null, 2) + "\n");
console.log(`wrote ${path.join(out, "playground.json")}`);
