// The docs playground's tier-3 worker (docs/design/docs-playground.md): one
// `app` block's session (`Session` in the app module) runs here, off the
// page's thread, so an app that loops can be stopped by ending the worker.
// The page (playground.js) owns xterm.js: it posts what the reader types and
// writes what comes back.
//
// Messages in:  {type: "start", js, wasm, app, input, columns, rows}
//               {type: "input", data} · {type: "resize", columns, rows}
// Messages out: {type: "ready"} · {type: "out", data} ·
//               {type: "done", result} · {type: "copy", text} ·
//               {type: "error", message, panic}
let session = null;
let timer = 0;
const started = performance.now();
const now = () => performance.now() - started;

function send(data) {
  if (data) postMessage({ type: "out", data });
}

// After every turn: hand on copies, report the end, or wait for the next
// tick the session asks for.
function after() {
  for (const text of JSON.parse(session.copies())) postMessage({ type: "copy", text });
  clearTimeout(timer);
  if (session.done()) {
    postMessage({ type: "done", result: session.result() });
    session.free();
    session = null;
    return;
  }
  const wait = session.timeout(now());
  if (wait >= 0) timer = setTimeout(() => turn((s) => s.tick(now())), Math.max(1, Math.min(wait, 1000)));
}

function turn(step) {
  if (!session) return;
  try {
    send(step(session));
    after();
  } catch (e) {
    session = null;
    postMessage({ type: "error", message: String(e && e.message ? e.message : e), panic: e instanceof WebAssembly.RuntimeError });
  }
}

onmessage = async ({ data: message }) => {
  if (message.type === "start") {
    try {
      const glue = await import(message.js);
      await glue.default({ module_or_path: message.wasm });
      session = new glue.Session(message.app, message.input, message.columns, message.rows);
    } catch (e) {
      postMessage({ type: "error", message: String(e && e.message ? e.message : e), panic: e instanceof WebAssembly.RuntimeError });
      return;
    }
    postMessage({ type: "ready" });
    turn((s) => s.output(now()));
  } else if (message.type === "input") {
    turn((s) => s.input(message.data, now()));
  } else if (message.type === "resize") {
    turn((s) => s.resize(message.columns, message.rows, now()));
  }
};
