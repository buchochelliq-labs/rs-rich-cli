// The playground terminal (tier 2): xterm.js on the page, a small line
// editor, and a worker (worker.js) that runs each line. Loaded by
// playground.js on the first "Try it" of a CLI block.
//
// The line editor: typing, Backspace, Left/Right, Home/End, Up/Down through
// what was typed, Ctrl+C (clears the line, or stops a running command),
// Ctrl+L (clears the screen), Ctrl+U (clears the line). A block's commands
// are filled in at the prompt one at a time: Enter runs each, and the next
// one is filled in when it is done.
//
// Stop terminates the worker and starts a fresh one: whatever a command was
// doing ends, and the files go back to the samples. Save as tape downloads
// the lines typed as a `.tape` for `rich record`.

import { sessionTape } from "./shell.js";
import { sampleFiles } from "./samples.js";

const PROMPT = "\x1b[1;32m$\x1b[0m ";
const PROMPT_WIDTH = 2;
const MAX_LINE = 4000;
const encoder = new TextEncoder();

let compiled = null;

// The `rich` module, fetched and compiled once per page and handed to each
// terminal's worker (a compiled module is structured-cloneable).
function compile(url) {
  if (!compiled) {
    compiled = (async () => {
      if (WebAssembly.compileStreaming) {
        try {
          return await WebAssembly.compileStreaming(fetch(url));
        } catch (error) {
          // A server that sends .wasm without application/wasm: compile the bytes.
        }
      }
      const response = await fetch(url);
      if (!response.ok) throw new Error(`${url}: ${response.status}`);
      return WebAssembly.compile(await response.arrayBuffer());
    })();
    compiled.catch(() => {
      compiled = null;
    });
  }
  return compiled;
}

function el(tag, props = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (key === "class") node.className = value;
    else if (key.startsWith("aria-") || key === "role") node.setAttribute(key, value);
    else node[key] = value;
  }
  node.append(...children);
  return node;
}

// Open a terminal in `host`. `wasm` is the module's URL, `worker` the
// worker script's; `commands` are filled in at the prompt in turn.
export async function openTerminal({ host, wasm, worker: workerUrl, bytes, Terminal, FitAddon, commands = [], rows = 18 }) {
  const status = el("span", { class: "rp-status", role: "status" });
  const stop = el("button", { type: "button", class: "md-button rp-small rp-stop", textContent: "Stop", disabled: true });
  const save = el("button", { type: "button", class: "md-button rp-small rp-save", textContent: "Save as tape" });
  const toolbar = el("div", { class: "rp-toolbar" }, stop, save, status);
  const screen = el("div", { class: "rp-terminal rp-shell-screen" });
  const box = el("div", { class: "rp-shell" }, toolbar, screen);
  host.append(box);

  const term = new Terminal({
    rows, cols: 80, convertEol: true, cursorBlink: true, scrollback: 2000,
    fontFamily: '"Fira Code", "DejaVu Sans Mono", Menlo, monospace', fontSize: 13,
  });
  const fit = FitAddon ? new FitAddon.FitAddon() : null;
  if (fit) term.loadAddon(fit);
  term.open(screen);
  // Fit the columns to the box; the rows stay as the block asked. (The box
  // has no height of its own, so fitting rows would grow it without end.)
  const refit = () => {
    if (!fit) return;
    try {
      const size = fit.proposeDimensions();
      if (size && size.cols > 10 && size.cols !== term.cols) term.resize(size.cols, rows);
    } catch (error) {
      // Not laid out yet.
    }
  };
  refit();
  const observer = typeof ResizeObserver === "function" ? new ResizeObserver(refit) : null;
  if (observer) observer.observe(screen);

  const queue = [...commands];
  const history = [];
  const entries = [];
  let buffer = "";
  let cursor = 0;
  let recalled = -1;
  let running = false;
  let worker = null;
  let ready = null;
  let lastByte = 10;

  const setState = (state) => {
    box.dataset.state = state;
    stop.disabled = state !== "running";
  };

  function write(data) {
    if (!data.length) return;
    lastByte = typeof data === "string" ? data.charCodeAt(data.length - 1) : data[data.length - 1];
    term.write(data);
  }

  function redraw() {
    let line = `\r\x1b[K${PROMPT}${buffer}`;
    const back = buffer.length - cursor;
    if (back > 0) line += `\x1b[${back}D`;
    write(line);
  }

  function prompt() {
    if (lastByte !== 10) write("\r\n");
    buffer = queue.length ? queue.shift() : "";
    cursor = buffer.length;
    recalled = -1;
    redraw();
  }

  function start() {
    status.textContent = `Loading rich (${Math.round(bytes / 1024)} KB)…`;
    setState("loading");
    worker = new Worker(workerUrl, { type: "module" });
    const current = worker;
    ready = compile(wasm).then(
      (module) =>
        new Promise((resolve, reject) => {
          current.onmessage = ({ data }) => {
            if (data.type === "ready") resolve();
            else if (data.type === "error") reject(new Error(data.message));
          };
          current.onerror = (event) => reject(new Error(event.message || "the worker did not start"));
          current.postMessage({ type: "init", module });
        }),
    );
    return ready.then(
      () => {
        if (worker !== current) return;
        current.onmessage = ({ data }) => {
          if (data.type === "out") write(data.bytes);
          else if (data.type === "done" || data.type === "error") {
            if (data.type === "error") write(encoder.encode(`\r\nterminal: ${data.message}\r\n`));
            running = false;
            setState("ready");
            status.textContent = data.code ? `exit ${data.code}` : "";
            prompt();
          }
        };
        status.textContent = "";
        setState("ready");
      },
      (error) => {
        status.textContent = "The terminal could not start here.";
        setState("failed");
        write(`\r\nterminal: ${error.message}\r\n`);
        throw error;
      },
    );
  }

  function run() {
    const line = buffer.trim();
    write("\r\n");
    buffer = "";
    cursor = 0;
    recalled = -1;
    if (!line) {
      prompt();
      return;
    }
    if (history[history.length - 1] !== line) history.push(line);
    entries.push({ line, at: performance.now() });
    running = true;
    setState("running");
    status.textContent = "running…";
    worker.postMessage({ type: "run", line, columns: term.cols, rows: term.rows, history });
  }

  function halt() {
    if (!running) return;
    worker.terminate();
    running = false;
    write("^C\r\n\x1b[2m(stopped: a new session starts, with the sample files as they were)\x1b[0m\r\n");
    queue.length = 0;
    start().then(prompt, () => {});
  }

  function key(data) {
    if (running) {
      if (data.includes("\x03")) halt();
      return;
    }
    if (box.dataset.state !== "ready") return;
    for (let i = 0; i < data.length; i++) {
      const rest = data.slice(i);
      const seq = /^\x1b\[(?:[ABCDFH]|[0-9;]*~)/.exec(rest) || /^\x1bO[ABCDFH]/.exec(rest);
      if (seq) {
        i += seq[0].length - 1;
        const code = seq[0].slice(-1);
        if (code === "A" || code === "B") {
          if (!history.length) continue;
          if (code === "A") recalled = recalled < 0 ? history.length - 1 : Math.max(0, recalled - 1);
          else recalled = recalled < 0 ? -1 : recalled + 1;
          if (recalled >= history.length) recalled = -1;
          buffer = recalled < 0 ? "" : history[recalled];
          cursor = buffer.length;
        } else if (code === "C") cursor = Math.min(buffer.length, cursor + 1);
        else if (code === "D") cursor = Math.max(0, cursor - 1);
        else if (code === "H") cursor = 0;
        else if (code === "F") cursor = buffer.length;
        else if (seq[0] === "\x1b[3~" && cursor < buffer.length) buffer = buffer.slice(0, cursor) + buffer.slice(cursor + 1);
        redraw();
        continue;
      }
      const c = data[i];
      if (c === "\r" || c === "\n") {
        run();
        // What was pasted after the line is dropped while it runs.
        return;
      } else if (c === "\x7f" || c === "\b") {
        if (cursor > 0) {
          buffer = buffer.slice(0, cursor - 1) + buffer.slice(cursor);
          cursor -= 1;
        }
      } else if (c === "\x03") {
        write("^C\r\n");
        buffer = "";
        cursor = 0;
        prompt();
        continue;
      } else if (c === "\x0c") {
        write("\x1b[H\x1b[2J");
      } else if (c === "\x15") {
        buffer = "";
        cursor = 0;
      } else if (c === "\x01") cursor = 0;
      else if (c === "\x05") cursor = buffer.length;
      else if (c === "\x1b") continue;
      else if (c >= " " && buffer.length < MAX_LINE) {
        buffer = buffer.slice(0, cursor) + c + buffer.slice(cursor);
        cursor += 1;
      }
      redraw();
    }
  }

  term.onData(key);
  stop.addEventListener("click", halt);
  save.addEventListener("click", () => {
    const tape = sessionTape({ entries, files: sampleFiles(), columns: term.cols, rows: term.rows });
    const url = URL.createObjectURL(new Blob([tape], { type: "text/plain" }));
    const link = el("a", { href: url, download: "session.tape" });
    document.body.append(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    status.textContent = entries.length ? "Saved session.tape: replay it with `rich record session.tape`." : "Saved an empty session.tape.";
  });

  await start();
  write("\x1b[2mrs-rich's `rich`, in this tab. Type help for what is here.\x1b[0m\r\n");
  prompt();
  term.focus();
  return {
    term,
    dispose() {
      if (observer) observer.disconnect();
      if (worker) worker.terminate();
      term.dispose();
      box.remove();
    },
  };
}
