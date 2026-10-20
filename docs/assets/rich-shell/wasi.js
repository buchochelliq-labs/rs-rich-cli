// Run the `rich` CLI, compiled for wasm32-wasip1, once: a fresh instance
// of the compiled module per command, under the vendored WASI shim, over an
// in-memory file system that outlives the instance. Used by the terminal's
// worker (worker.js) and by the docs check under Node
// (scripts/check_playground_blocks.mjs); nothing here touches the DOM.

import { WASI, File, Directory, OpenFile, PreopenDirectory, ConsoleStdout, Fd, wasi as defs } from "./vendor/browser_wasi_shim/index.js";

const encoder = new TextEncoder();

// A terminal's stdin with nothing typed: a character device (so `rich`
// sees a terminal, as it would in a shell) that reads as end of input.
class TerminalInput extends Fd {
  fd_fdstat_get() {
    const fdstat = new defs.Fdstat(defs.FILETYPE_CHARACTER_DEVICE, 0);
    fdstat.fs_rights_base = BigInt(defs.RIGHTS_FD_READ);
    return { ret: 0, fdstat };
  }
  fd_filestat_get() {
    return { ret: 0, filestat: new defs.Filestat(0n, defs.FILETYPE_CHARACTER_DEVICE, 0n) };
  }
  fd_read() {
    return { ret: 0, data: new Uint8Array(0) };
  }
}

// Output that is not a terminal (a pipe into the next command, or a
// redirect): collected, and seen by `rich` as a plain file.
class Collector extends Fd {
  constructor() {
    super();
    this.chunks = [];
  }
  fd_fdstat_get() {
    const fdstat = new defs.Fdstat(defs.FILETYPE_REGULAR_FILE, 0);
    fdstat.fs_rights_base = BigInt(defs.RIGHTS_FD_WRITE);
    return { ret: 0, fdstat };
  }
  fd_filestat_get() {
    return { ret: 0, filestat: new defs.Filestat(0n, defs.FILETYPE_REGULAR_FILE, 0n) };
  }
  fd_write(data) {
    this.chunks.push(data.slice());
    return { ret: 0, nwritten: data.byteLength };
  }
  bytes() {
    return concat(this.chunks);
  }
}

export function concat(chunks) {
  const size = chunks.reduce((n, c) => n + c.byteLength, 0);
  const out = new Uint8Array(size);
  let at = 0;
  for (const chunk of chunks) {
    out.set(chunk, at);
    at += chunk.byteLength;
  }
  return out;
}

// The file system: a Map of names to the shim's File and Directory inodes,
// preopened as `/` for every command, so what one command writes the next
// reads.
export function makeRoot(files) {
  const root = new Map();
  for (const [name, data] of Object.entries(files)) writeFile(root, name, data);
  return root;
}

function walk(root, path, create) {
  const parts = path.split("/").filter((p) => p && p !== ".");
  let dir = root;
  for (const part of parts.slice(0, -1)) {
    let entry = dir.get(part);
    if (!entry && create) {
      entry = new Directory(new Map());
      dir.set(part, entry);
    }
    if (!(entry instanceof Directory)) return null;
    dir = entry.contents;
  }
  return { dir, name: parts[parts.length - 1] };
}

export function lookup(root, path) {
  const parts = path.split("/").filter((p) => p && p !== ".");
  if (!parts.length) return new Directory(root);
  const found = walk(root, path, false);
  return found ? found.dir.get(found.name) || null : null;
}

export function writeFile(root, path, data, append = false) {
  const found = walk(root, path, true);
  if (!found || !found.name) return false;
  const bytes = typeof data === "string" ? encoder.encode(data) : data;
  const existing = found.dir.get(found.name);
  if (existing instanceof Directory) return false;
  if (append && existing instanceof File) existing.data = concat([existing.data, bytes]);
  else found.dir.set(found.name, new File(bytes));
  return true;
}

// Run `rich ARGS`. `stdin` is bytes from a pipe or a redirect, or null for
// the terminal. `stdout` is a callback for terminal output (it streams), or
// null to collect it (a pipe); `stderr` always streams. Returns the exit
// code and, when collected, the output.
export async function runRich(module, root, { args, env, stdin = null, stdout = null, stderr }) {
  const out = stdout ? new ConsoleStdout(stdout) : new Collector();
  const fds = [
    stdin ? new OpenFile(new File(stdin)) : new TerminalInput(),
    out,
    new ConsoleStdout(stderr),
    new PreopenDirectory("/", root),
  ];
  const argv = ["rich", ...args];
  const environ = Object.entries(env).map(([k, v]) => `${k}=${v}`);
  const shim = new WASI(argv, environ, fds, { debug: false });
  // 0.4.2 sizes arguments in UTF-16 units; the buffer needs UTF-8 bytes.
  shim.wasiImport.args_sizes_get = (argc, size) => {
    const view = new DataView(shim.inst.exports.memory.buffer);
    view.setUint32(argc, argv.length, true);
    view.setUint32(size, argv.reduce((n, a) => n + encoder.encode(a).length + 1, 0), true);
    return 0;
  };
  const instance = await WebAssembly.instantiate(module, { wasi_snapshot_preview1: shim.wasiImport });
  let code;
  try {
    code = shim.start(instance);
  } catch (error) {
    if (error instanceof WebAssembly.RuntimeError) {
      // A panic: its message went to stderr, then the module aborted.
      stderr(encoder.encode("rich: stopped (the WebAssembly module aborted)\r\n"));
      code = 134;
    } else {
      throw error;
    }
  }
  return { code, output: stdout ? null : out.bytes() };
}
