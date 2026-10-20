// The playground terminal's line runner: a small shell, not a POSIX one.
// It splits a line into commands, runs `rich` (through the `exec` it is
// given) and a few builtins, and connects them with pipes and redirects.
// It also turns a session into a tape for `rich record`.
//
// What a line can hold:
//   - words, with 'single' and "double" quotes and \ escapes, and $NAME or
//     ${NAME} from the session's variables (unset names are empty);
//   - NAME=value before a command (for that command), or alone (for the
//     session);
//   - `|` pipes, `;` and `&&` and `||` between commands, `# comments`;
//   - `< file`, `> file` and `>> file`, and `2>&1` / `2>/dev/null`;
//   - `*` and `?` in a word, matched against the file names in its
//     directory (left as written when nothing matches).
// Builtins: echo, printf, seq, cat, ls, clear, pwd, true, false, help, history.
// Anything else is `command not found`, exit 127.
//
// Environment-neutral: no DOM, no worker API; worker.js and the docs check
// (scripts/check_playground_blocks.mjs) both use it.

const encoder = new TextEncoder();
const decoder = new TextDecoder();

export const BUILTINS = ["echo", "printf", "seq", "cat", "ls", "clear", "pwd", "true", "false", "help", "history"];
// Builtins a tape leaves out: `rich record` replays in a real shell, which
// has no `help`, and whose `history` is its own.
const SESSION_ONLY = new Set(["help", "history"]);

export class ShellError extends Error {}

// ---- words -------------------------------------------------------------

// Tokens: { word, quoted } for words (quoted: whether any part was quoted,
// so it is never globbed), or { op } for operators.
export function tokenize(line, vars = {}) {
  const tokens = [];
  let word = "";
  let inWord = false;
  let quoted = false;
  let glob = false;
  const push = () => {
    if (inWord) tokens.push({ word, quoted, glob });
    word = "";
    inWord = false;
    quoted = false;
    glob = false;
  };
  const expand = (i) => {
    // line[i] is "$": return [text, next index].
    const rest = line.slice(i + 1);
    let m = /^\{([A-Za-z_][A-Za-z0-9_]*)\}/.exec(rest) || /^([A-Za-z_][A-Za-z0-9_]*)/.exec(rest);
    if (m) return [vars[m[1]] ?? "", i + 1 + m[0].length];
    if (rest[0] === "?") return [String(vars["?"] ?? 0), i + 2];
    if (rest[0] === "(") throw new ShellError("$(…) is not supported here");
    return ["$", i + 1];
  };
  for (let i = 0; i < line.length; ) {
    const c = line[i];
    if (c === " " || c === "\t") {
      push();
      i += 1;
    } else if (c === "#" && !inWord) {
      break;
    } else if (c === "'") {
      const end = line.indexOf("'", i + 1);
      if (end < 0) throw new ShellError("unclosed single quote");
      word += line.slice(i + 1, end);
      inWord = quoted = true;
      i = end + 1;
    } else if (c === '"') {
      inWord = quoted = true;
      i += 1;
      for (;;) {
        if (i >= line.length) throw new ShellError("unclosed double quote");
        const d = line[i];
        if (d === '"') {
          i += 1;
          break;
        }
        if (d === "\\" && i + 1 < line.length && '"\\$`'.includes(line[i + 1])) {
          word += line[i + 1];
          i += 2;
        } else if (d === "$") {
          const [text, next] = expand(i);
          word += text;
          i = next;
        } else if (d === "`") {
          throw new ShellError("`…` is not supported here");
        } else {
          word += d;
          i += 1;
        }
      }
    } else if (c === "\\") {
      if (i + 1 < line.length) word += line[i + 1];
      inWord = quoted = true;
      i += 2;
    } else if (c === "$") {
      const [text, next] = expand(i);
      word += text;
      inWord = true;
      i = next;
    } else if (c === "`") {
      throw new ShellError("`…` is not supported here");
    } else if (c === "|" || c === ";" || c === "&" || c === "<" || c === ">") {
      // `2>&1` and `2>file` only make sense right after a bare `2`.
      if (c === ">" && inWord && !quoted && word === "2") {
        word = "";
        inWord = false;
        if (line.startsWith(">&1", i)) {
          tokens.push({ op: "2>&1" });
          i += 3;
        } else {
          tokens.push({ op: "2>" });
          i += 1;
        }
        continue;
      }
      push();
      const two = line.slice(i, i + 2);
      if (two === "&&" || two === "||" || two === ">>") {
        tokens.push({ op: two });
        i += 2;
      } else if (c === "&") {
        throw new ShellError("background jobs (&) are not supported here");
      } else {
        tokens.push({ op: c });
        i += 1;
      }
    } else {
      if (c === "*" || c === "?") glob = true;
      word += c;
      inWord = true;
      i += 1;
    }
  }
  push();
  return tokens;
}

// A line as a list of pipelines, each with the operator that runs it
// (";" always, "&&" after success, "||" after failure).
export function parse(line, vars = {}) {
  const tokens = tokenize(line, vars);
  const list = [];
  let pipeline = [];
  let command = newCommand();
  let joiner = ";";
  function newCommand() {
    return { words: [], assigns: [], stdin: null, stdout: null, append: false, stderr: null };
  }
  const endCommand = () => {
    if (!command.words.length && !command.assigns.length) {
      if (command.stdin || command.stdout) throw new ShellError("a redirect needs a command");
      return false;
    }
    pipeline.push(command);
    command = newCommand();
    return true;
  };
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    if (token.op === "|") {
      if (!endCommand()) throw new ShellError("syntax error near |");
    } else if (token.op === ";" || token.op === "&&" || token.op === "||") {
      const had = endCommand();
      if (!had && !pipeline.length) {
        if (token.op === ";" && !list.length) continue;
        throw new ShellError(`syntax error near ${token.op}`);
      }
      list.push({ joiner, pipeline });
      pipeline = [];
      joiner = token.op;
    } else if (token.op === "<" || token.op === ">" || token.op === ">>" || token.op === "2>") {
      const target = tokens[i + 1];
      if (!target || target.op) throw new ShellError(`${token.op} needs a file name`);
      i += 1;
      if (token.op === "<") command.stdin = target.word;
      else if (token.op === "2>") command.stderr = target.word;
      else {
        command.stdout = target.word;
        command.append = token.op === ">>";
      }
    } else if (token.op === "2>&1") {
      command.stderr = "&1";
    } else if (!command.words.length && !token.quoted && /^[A-Za-z_][A-Za-z0-9_]*=/.test(token.word)) {
      const at = token.word.indexOf("=");
      command.assigns.push([token.word.slice(0, at), token.word.slice(at + 1)]);
    } else {
      command.words.push(token);
    }
  }
  if (endCommand() || pipeline.length) list.push({ joiner, pipeline });
  else if (joiner !== ";") throw new ShellError(`syntax error: nothing after ${joiner}`);
  return list;
}

// ---- the file system -----------------------------------------------------

// `fs` is { read(path) -> Uint8Array | null, write(path, bytes, append) ->
// bool, list(path) -> [{ name, dir, size }] | null } over the session's
// files. Paths are relative to `/`, the only directory there is to be in.

function clean(path) {
  return path.replace(/^\/+/, "").replace(/^(\.\/)+/, "");
}

function globWord(fs, token) {
  if (token.quoted || !token.glob) return [token.word];
  const slash = token.word.lastIndexOf("/");
  const dir = slash >= 0 ? token.word.slice(0, slash) : "";
  const pattern = token.word.slice(slash + 1);
  const entries = fs.list(clean(dir)) || [];
  const re = new RegExp(
    "^" + pattern.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".") + "$",
  );
  const names = entries
    .map((e) => e.name)
    .filter((name) => re.test(name) && (pattern.startsWith(".") || !name.startsWith(".")))
    .sort();
  return names.length ? names.map((name) => (slash >= 0 ? `${dir}/${name}` : name)) : [token.word];
}

// ---- builtins ------------------------------------------------------------

function echo(args) {
  let newline = true;
  let escapes = false;
  while (args.length && /^-[neE]+$/.test(args[0])) {
    const flag = args.shift();
    if (flag.includes("n")) newline = false;
    if (flag.includes("e")) escapes = true;
    if (flag.includes("E")) escapes = false;
  }
  let text = args.join(" ");
  if (escapes) text = backslashes(text);
  return text + (newline ? "\n" : "");
}

function backslashes(text) {
  return text.replace(/\\([nt\\ae0rv]|x[0-9a-fA-F]{1,2})/g, (_, c) => {
    if (c[0] === "x") return String.fromCharCode(parseInt(c.slice(1), 16));
    return { n: "\n", t: "\t", "\\": "\\", a: "\x07", e: "\x1b", 0: "\0", r: "\r", v: "\v" }[c];
  });
}

function printf(args) {
  if (!args.length) throw new ShellError("printf: usage: printf FORMAT [ARGUMENTS...]");
  const format = backslashes(args[0]);
  let rest = args.slice(1);
  let out = "";
  do {
    let used = 0;
    out += format.replace(/%([-0-9.]*)([sdbc%])/g, (_, width, kind) => {
      if (kind === "%") return "%";
      const value = rest[used++] ?? "";
      let text = kind === "d" ? String(parseInt(value, 10) || 0) : kind === "b" ? backslashes(value) : kind === "c" ? value.slice(0, 1) : value;
      const w = parseInt(width, 10);
      if (w) text = width.startsWith("-") ? text.padEnd(Math.abs(w)) : text.padStart(w);
      return text;
    });
    rest = rest.slice(used);
    if (!used) break;
  } while (rest.length);
  return out;
}

function ls(fs, args, columns) {
  const all = args.includes("-a") || args.includes("-la") || args.includes("-al");
  const long = args.some((a) => /^-[a-z]*l/.test(a));
  const paths = args.filter((a) => !a.startsWith("-"));
  let out = "";
  let err = "";
  let code = 0;
  for (const path of paths.length ? paths : [""]) {
    const entries = fs.list(clean(path));
    if (!entries) {
      if (fs.read(clean(path))) {
        out += `${path}\n`;
        continue;
      }
      err += `ls: cannot access '${path}': No such file or directory\n`;
      code = 2;
      continue;
    }
    if (paths.length > 1) out += `${path}:\n`;
    const shown = entries.filter((e) => all || !e.name.startsWith(".")).sort((a, b) => (a.name < b.name ? -1 : 1));
    if (long) {
      const width = Math.max(1, ...shown.map((e) => String(e.size).length));
      for (const e of shown) out += `${e.dir ? "d" : "-"} ${String(e.size).padStart(width)}  ${e.name}${e.dir ? "/" : ""}\n`;
    } else {
      const names = shown.map((e) => e.name + (e.dir ? "/" : ""));
      const cell = Math.max(1, ...names.map((n) => n.length)) + 2;
      const per = Math.max(1, Math.floor(columns / cell));
      for (let i = 0; i < names.length; i += per) {
        out += names.slice(i, i + per).map((n, j, row) => (j < row.length - 1 ? n.padEnd(cell) : n)).join("") + "\n";
      }
    }
  }
  return { out, err, code };
}

export function helpText(files) {
  return [
    "This is rs-rich's `rich` command, compiled to WebAssembly and running in",
    "your browser tab, with a few sample files. Try:",
    "",
    "  rich README.md                  rich sales.csv",
    "  rich main.rs --line-numbers     rich data.json",
    "  rich chart sales.csv --kind bar --x month --y api",
    "  echo '# Hi' | rich - --markdown",
    "  rich --help",
    "",
    `Builtins: ${BUILTINS.join(", ")}; pipes (|), ;, &&, ||, < > >>.`,
    "Up and Down recall what you typed; Ctrl+C clears the line; Stop ends a",
    "command that runs too long (the files go back to the samples).",
    "",
    `Files: ${files.join("  ")}`,
    "",
  ].join("\n");
}

// ---- running -------------------------------------------------------------

// A session: variables, the last exit code, and what it needs from outside.
//   exec(argv, { stdin, stdout, stderr, env }) -> Promise<{ code, output }>
//     runs `rich` (argv without "rich"); stdout is a callback, or null to
//     collect it into `output` (bytes);
//   write(bytes) / writeError(bytes): the terminal;
//   fs: as above; columns(): the terminal's width; history(): lines typed.
export class Shell {
  constructor({ exec, write, writeError, fs, columns, history, env = {} }) {
    Object.assign(this, { exec, write, writeError, fs, columns, history });
    this.vars = { ...env };
    this.status = 0;
  }

  async run(line) {
    let list;
    try {
      list = parse(line, { ...this.vars, "?": this.status });
    } catch (error) {
      if (!(error instanceof ShellError)) throw error;
      this.writeError(encoder.encode(`sh: ${error.message}\n`));
      this.status = 2;
      return this.status;
    }
    for (const { joiner, pipeline } of list) {
      if (joiner === "&&" && this.status !== 0) continue;
      if (joiner === "||" && this.status === 0) continue;
      this.status = await this.pipeline(pipeline);
    }
    return this.status;
  }

  async pipeline(commands) {
    let input = null;
    let code = 0;
    for (let i = 0; i < commands.length; i++) {
      const last = i === commands.length - 1;
      const result = await this.command(commands[i], input, last);
      code = result.code;
      input = result.output;
    }
    return code;
  }

  // One command: its stdin is `input` (bytes) or the terminal (null); its
  // stdout is the terminal when it is the pipeline's last and is not
  // redirected, otherwise collected and returned as `output`.
  async command(command, input, last) {
    const fail = (message, code = 1) => {
      this.writeError(encoder.encode(message.endsWith("\n") ? message : `${message}\n`));
      return { code, output: new Uint8Array(0) };
    };
    if (!command.words.length) {
      for (const [name, value] of command.assigns) this.vars[name] = value;
      return { code: 0, output: new Uint8Array(0) };
    }
    const argv = command.words.flatMap((token) => globWord(this.fs, token));
    if (command.stdin) {
      input = this.fs.read(clean(command.stdin));
      if (!input) return fail(`sh: ${command.stdin}: No such file or directory`);
    }
    const toTerminal = last && !command.stdout;
    const collected = [];
    const stdout = (bytes) => (toTerminal ? this.write(bytes) : collected.push(typeof bytes === "string" ? encoder.encode(bytes) : bytes));
    const errors = command.stderr === "&1" ? stdout : command.stderr ? () => {} : (b) => this.writeError(b);
    const [name, ...args] = argv;
    let code = 0;
    if (name === "rich") {
      const env = { ...this.vars, COLUMNS: String(this.columns()) };
      for (const [k, v] of command.assigns) env[k] = v;
      const result = await this.exec(args, {
        stdin: input,
        stdout: toTerminal ? (b) => this.write(b) : null,
        stderr: (b) => errors(b),
        env,
      });
      code = result.code;
      if (result.output) collected.push(result.output);
    } else if (!BUILTINS.includes(name)) {
      return fail(`${name}: command not found (type help to see what is here)`, 127);
    } else {
      const text = (s) => stdout(encoder.encode(s));
      switch (name) {
        case "echo":
          text(echo(args));
          break;
        case "printf":
          try {
            text(printf(args));
          } catch (error) {
            return fail(error.message, 2);
          }
          break;
        case "seq": {
          const numbers = args.map(Number);
          if (!numbers.length || numbers.length > 3 || numbers.some((n) => !Number.isFinite(n))) {
            return fail("seq: usage: seq [FIRST [STEP]] LAST", 2);
          }
          const [first, step, end] = numbers.length === 1 ? [1, 1, numbers[0]] : numbers.length === 2 ? [numbers[0], 1, numbers[1]] : numbers;
          if (step === 0 || Math.abs((end - first) / step) > 100000) return fail("seq: too many numbers", 2);
          let lines = "";
          for (let n = first; step > 0 ? n <= end : n >= end; n += step) lines += `${n}\n`;
          text(lines);
          break;
        }
        case "cat": {
          const files = args.filter((a) => a !== "-n");
          if (!files.length || files.includes("-")) {
            if (input) stdout(input);
          }
          for (const file of files) {
            if (file === "-") continue;
            const bytes = this.fs.read(clean(file));
            if (!bytes) {
              errors(encoder.encode(`cat: ${file}: No such file or directory\n`));
              code = 1;
            } else stdout(bytes);
          }
          break;
        }
        case "ls": {
          const result = ls(this.fs, args, this.columns());
          text(result.out);
          if (result.err) errors(encoder.encode(result.err));
          code = result.code;
          break;
        }
        case "clear":
          text("\x1b[H\x1b[2J\x1b[3J");
          break;
        case "pwd":
          text("/\n");
          break;
        case "true":
          break;
        case "false":
          code = 1;
          break;
        case "help":
          text(helpText((this.fs.list("") || []).map((e) => e.name + (e.dir ? "/" : "")).sort()));
          break;
        case "history":
          text(this.history().map((line, i) => `${String(i + 1).padStart(4)}  ${line}\n`).join(""));
          break;
      }
    }
    const output = concatBytes(collected);
    if (command.stdout) {
      if (!this.fs.write(clean(command.stdout), output, command.append)) {
        return fail(`sh: ${command.stdout}: cannot write here`);
      }
    }
    return { code, output };
  }
}

function concatBytes(chunks) {
  const size = chunks.reduce((n, c) => n + c.byteLength, 0);
  const out = new Uint8Array(size);
  let at = 0;
  for (const chunk of chunks) {
    out.set(chunk, at);
    at += chunk.byteLength;
  }
  return out;
}

// ---- a docs block's commands -----------------------------------------------

// The command lines of a CLI example block: `$ `-prefixed lines when the
// block has any (a console transcript: the rest is output), otherwise every
// line; `\` at a line's end continues it; blank lines and comment lines go.
export function commandLines(text) {
  const raw = text.replace(/\r/g, "").split("\n");
  const prompted = raw.some((line) => /^\$ /.test(line));
  const lines = [];
  let pending = "";
  for (const line of raw) {
    let body = line;
    if (prompted) {
      if (pending) body = line;
      else if (/^\$ /.test(line)) body = line.slice(2);
      else continue;
    }
    if (/\\$/.test(body)) {
      pending += body.slice(0, -1) + " ";
      continue;
    }
    body = (pending + body).trim();
    pending = "";
    if (body && !body.startsWith("#")) lines.push(body);
  }
  if (pending.trim()) lines.push(pending.trim());
  return lines;
}

// ---- tapes -----------------------------------------------------------------

// A double-quoted tape word: rs-rich-record's tape lexer takes \" and \\ in
// double quotes, and keeps every other character as written.
function tapeWord(text) {
  return `"${text.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

// `Write`'s content: the tape's own escapes (\n, \t, \\) under the word's.
function writeContent(text) {
  return tapeWord(text.replace(/\\/g, "\\\\").replace(/\n/g, "\\n").replace(/\t/g, "\\t"));
}

// A session as a tape: the sample files its commands name (as `Write`
// lines, text files only; an image is noted for the reader to copy), then
// each line typed, with the pause before the next. `entries` is
// [{ line, at }] with `at` in milliseconds; `files` maps names to bytes.
export function sessionTape({ entries, files, columns, rows, date = new Date() }) {
  const out = [
    `# A session from the rs-rich docs playground, ${date.toISOString().slice(0, 10)}.`,
    "# Replay it with `rich record session.tape` (rs-rich-cli, a native build).",
    'Set Title "rich playground session"',
    `Set Size ${Math.min(500, Math.max(20, columns))}x${Math.min(200, Math.max(5, rows))}`,
    "Set TypingDelay 40ms",
  ];
  const typed = entries.filter((e) => !SESSION_ONLY.has(e.line.trim().split(/\s+/)[0]));
  const named = new Set();
  for (const { line } of typed) {
    for (const name of Object.keys(files)) {
      const re = new RegExp(`(^|[\\s'"<>=/])${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}($|[\\s'"|;&>])`);
      if (re.test(line)) named.add(name);
    }
  }
  for (const name of [...named].sort()) {
    const bytes = files[name];
    let text = null;
    try {
      text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    } catch {
      text = null;
    }
    if (text === null || /[\0\r\x1b]/.test(text) || !/^[A-Za-z0-9._/-]+$/.test(name)) {
      out.push(`# ${name} is a sample from the playground; put a copy beside this tape.`);
    } else {
      out.push(`Write ${name} ${writeContent(text)}`);
    }
  }
  typed.forEach(({ line, at }, i) => {
    // One line per Type: a tape line cannot hold a newline.
    out.push(`Type ${tapeWord(line.replace(/[\r\n]+/g, " "))}`);
    out.push("Enter");
    const next = typed[i + 1];
    const pause = next ? next.at - at : 2000;
    out.push(`Sleep ${Math.max(500, Math.min(5000, Math.round(pause / 100) * 100))}ms`);
  });
  return out.join("\n") + "\n";
}

export { decoder };
