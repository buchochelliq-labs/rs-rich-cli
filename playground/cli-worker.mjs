// The worker behind cli.mjs: one block, one session, one result.
import { parentPort, workerData } from "node:worker_threads";
import { readFileSync } from "node:fs";
import { createSession } from "../docs/assets/rich-shell/session.js";
import { commandLines } from "../docs/assets/rich-shell/shell.js";

const { wasm, text, columns } = workerData;
const module = await WebAssembly.compile(readFileSync(wasm));
const decoder = new TextDecoder();
let output = "";
const shell = createSession(module, {
  write: (bytes) => (output += decoder.decode(bytes, { stream: true })),
  writeError: (bytes) => (output += decoder.decode(bytes, { stream: true })),
  columns: () => columns,
});
const lines = [];
for (const line of commandLines(text)) {
  const code = await shell.run(line);
  lines.push({ line, code });
}
parentPort.postMessage({ ok: lines.length > 0 && lines.every((l) => l.code === 0), lines, output, error: null });
