// Load a built playground module under Node (no browser): the glue
// wasm-bindgen wrote for the web, instantiated from the file's bytes.
//
//     import { load } from "./node.mjs";
//     const lean = await load("docs/assets/playground", "lean");
//     lean.render("markup", "[b]hi[/]", "{}");

import { readFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export async function load(dir, name) {
  const glue = await import(pathToFileURL(path.resolve(dir, name, "rich_playground.js")).href);
  const bytes = readFileSync(path.resolve(dir, name, "rich_playground_bg.wasm"));
  glue.initSync({ module: bytes });
  return glue;
}
