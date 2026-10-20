// The playground terminal's worker: one session (sample files, the line
// runner, `rich` on WASI) per terminal, off the page's thread, so a
// command that runs too long is stopped by terminating this worker.
//
// Messages in:  { type: "init", module }       a compiled WebAssembly.Module
//               { type: "run", line, columns, rows, history }
// Messages out: { type: "ready" }, { type: "out", bytes }, { type: "done", code },
//               { type: "error", message }

import { createSession } from "./session.js";

let shell = null;
let columns = 80;
let rows = 24;
let history = [];

const send = (bytes) => postMessage({ type: "out", bytes: bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes) });

onmessage = async ({ data }) => {
  try {
    if (data.type === "init") {
      shell = createSession(data.module, {
        write: send,
        writeError: send,
        columns: () => columns,
        rows: () => rows,
        history: () => history,
      });
      postMessage({ type: "ready" });
    } else if (data.type === "run" && shell) {
      columns = data.columns;
      rows = data.rows;
      history = data.history || [];
      const code = await shell.run(data.line);
      postMessage({ type: "done", code });
    }
  } catch (error) {
    postMessage({ type: "error", message: String(error && error.message ? error.message : error) });
  }
};
