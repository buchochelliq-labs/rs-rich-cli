// Run a CLI example block's commands under Node, as the docs terminal runs
// them in the browser: the `rich` WASI module (playground/build.sh's
// `cli/rich.wasm`) under the vendored shim, over a fresh copy of the sample
// files, through the same line runner (docs/assets/rich-shell/). Each block
// runs in a worker thread with a time limit, so a command that never ends
// fails the block instead of hanging the check.
//
//     import { runBlock } from "./cli.mjs";
//     const result = await runBlock("docs/assets/playground/cli/rich.wasm", "rich --version");
//     // { ok, lines: [{ line, code }], output, error }

import { Worker } from "node:worker_threads";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));

export function runBlock(wasm, text, { timeout = 30000, columns = 100 } = {}) {
  return new Promise((resolve) => {
    const worker = new Worker(path.join(HERE, "cli-worker.mjs"), {
      workerData: { wasm: path.resolve(wasm), text, columns },
    });
    const timer = setTimeout(() => {
      worker.terminate();
      resolve({ ok: false, lines: [], output: "", error: `timed out after ${timeout / 1000} s` });
    }, timeout);
    worker.once("message", (result) => {
      clearTimeout(timer);
      worker.terminate();
      resolve(result);
    });
    worker.once("error", (error) => {
      clearTimeout(timer);
      resolve({ ok: false, lines: [], output: "", error: String(error && error.stack ? error.stack : error) });
    });
  });
}
