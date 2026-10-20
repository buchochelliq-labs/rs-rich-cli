// A terminal session: the sample files in a fresh in-memory file system,
// the line runner (shell.js) and `rich` on WASI (wasi.js), joined up. The
// browser's worker (worker.js) and the docs check under Node both make one.

import { Directory, File } from "./vendor/browser_wasi_shim/index.js";
import { makeRoot, lookup, writeFile, runRich } from "./wasi.js";
import { Shell } from "./shell.js";
import { sampleFiles } from "./samples.js";

// The variables `rich` sees: a 256-colour, truecolour terminal. COLUMNS
// and LINES follow the terminal's size on each command.
export const ENV = { TERM: "xterm-256color", COLORTERM: "truecolor", HOME: "/", PWD: "/", LANG: "C.UTF-8" };

export function createSession(module, { write, writeError, columns, rows = () => 24, history = () => [] }) {
  const root = makeRoot(sampleFiles());
  const fs = {
    read(path) {
      const entry = lookup(root, path);
      return entry instanceof File ? entry.data : null;
    },
    write(path, bytes, append) {
      return writeFile(root, path, bytes, append);
    },
    list(path) {
      const entry = lookup(root, path);
      if (!(entry instanceof Directory)) return null;
      return [...entry.contents].map(([name, inode]) => ({
        name,
        dir: inode instanceof Directory,
        size: inode instanceof File ? inode.data.byteLength : 0,
      }));
    },
  };
  const exec = (args, { stdin, stdout, stderr, env }) =>
    runRich(module, root, { args, env: { ...env, LINES: String(rows()) }, stdin, stdout, stderr });
  return new Shell({ exec, write, writeError, fs, columns, history, env: ENV });
}
