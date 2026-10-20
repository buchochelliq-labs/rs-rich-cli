// Docs as tests: render every `.rich-playground` block in docs/ with the
// built playground modules, under Node (no browser), and fail when one does
// not render.
//
//     node scripts/check_playground_blocks.mjs [MODULES]   # default: docs/assets/playground
//
// MODULES is playground/build.sh's output. For each block it checks that:
//   - its kind is one the modules render, and its data-* options are ones
//     the module knows (the module refuses unknown options);
//   - its input renders, as ANSI and as HTML, without an error or a panic;
//   - its input is under the page's cap;
//   - on a guide page, it sits in a `rich-playground-pair` div whose first
//     block is the Rust that renders it.
// A block whose input stops rendering (a renamed style, a changed option, a
// new panic on wasm) fails the docs job, as a tape that no longer records
// does.
//
// Tier 2: a `data-kind="cli"` block's commands run, in order, in a fresh
// session of the docs terminal (the `rich` WASI module, the sample files
// and the line runner; playground/cli.mjs), and each must exit 0. A CLI
// example (a bash, sh, shell or console fence with a `rich` command) that
// is not marked but runs fails too: every example that runs in the
// terminal offers it. Examples that cannot run there (a URL, --watch, a
// process, a file the samples lack) stay plain.

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { load } from "../playground/node.mjs";
import { runBlock } from "../playground/cli.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const DOCS = path.join(ROOT, "docs");
const MODULES = path.resolve(process.argv[2] || path.join(DOCS, "assets", "playground"));
// The same rules as docs/assets/playground.js.
const MAX_INPUT = 20000;
const RESERVED = new Set(["kind", "kinds"]);
const NUMBERS = new Set(["width", "indent", "bar-width", "bins", "threshold", "context", "tab-size"]);
// Pages whose blocks stand alone, without Rust beside them.
const STANDALONE = new Set(["playground.md"]);
// Pages about the docs rather than of them: their examples are not checked.
const NOT_EXAMPLES = /^(design|plans|releases)\//;
const SHELL_FENCE = /^(bash|sh|shell|console)\b/;
// A command line that starts with `rich` (after `$ ` in a transcript).
const RICH_LINE = /(^|\n)(\$ )?rich( |$)/m;

function markdownFiles(dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...markdownFiles(full));
    else if (entry.name.endsWith(".md")) out.push(full);
  }
  return out;
}

function attributes(info) {
  const attrs = {};
  for (const match of info.matchAll(/([a-z][a-z0-9-]*)="([^"]*)"/g)) attrs[match[1]] = match[2];
  return attrs;
}

// Fenced blocks in a Markdown file, with the pair div each sits in. A fence
// inside another fence (an example of the syntax) is text, not a block.
export function blocks(text) {
  const found = [];
  const lines = text.split("\n");
  let pair = null;
  for (let n = 0; n < lines.length; n++) {
    const line = lines[n];
    if (/^\s*<div class="rich-playground-pair"/.test(line)) {
      pair = { line: n + 1, fences: [] };
      continue;
    }
    if (pair && /^\s*<\/div>/.test(line)) {
      pair = null;
      continue;
    }
    const open = /^(\s*)(`{3,}|~{3,})(.*)$/.exec(line);
    if (!open) continue;
    const [, indent, fence, info] = open;
    const body = [];
    let end = n + 1;
    const close = new RegExp(`^\\s*${fence[0] === "`" ? "`" : "~"}{${fence.length},}\\s*$`);
    while (end < lines.length && !close.test(lines[end])) {
      body.push(lines[end].startsWith(indent) ? lines[end].slice(indent.length) : lines[end]);
      end += 1;
    }
    const block = { line: n + 1, info: info.trim(), body: body.join("\n"), pair };
    if (pair) pair.fences.push(block);
    if (!/\.rich-playground\b/.test(info) && SHELL_FENCE.test(block.info) && RICH_LINE.test(block.body)) {
      block.kind = null;
      block.candidate = true;
      found.push(block);
    } else if (/\.rich-playground\b/.test(info)) {
      const attrs = attributes(info);
      block.kind = attrs["data-kind"];
      block.options = {};
      for (const [key, value] of Object.entries(attrs)) {
        if (!key.startsWith("data-")) continue;
        const name = key.slice(5);
        if (RESERVED.has(name)) continue;
        if (NUMBERS.has(name)) block.options[name] = Number(value);
        else if (value === "true" || value === "false") block.options[name] = value === "true";
        else block.options[name] = value;
      }
      found.push(block);
    }
    n = end;
  }
  return found;
}

async function main() {
  const stamp = JSON.parse(readFileSync(path.join(MODULES, "playground.json"), "utf8"));
  const modules = {};
  for (const name of Object.keys(stamp.modules)) modules[name] = await load(MODULES, name);
  const moduleFor = (kind) => Object.keys(stamp.modules).find((name) => stamp.modules[name].kinds.includes(kind));

  let failures = 0;
  let panics = 0;
  let count = 0;
  const pages = new Set();
  const kinds = new Map();
  const cli = [];
  for (const file of markdownFiles(DOCS)) {
    const rel = path.relative(DOCS, file).split(path.sep).join("/");
    for (const block of blocks(readFileSync(file, "utf8"))) {
      if (block.candidate || block.kind === "cli") {
        if (!NOT_EXAMPLES.test(rel) || block.kind === "cli") cli.push({ rel, block });
        continue;
      }
      count += 1;
      pages.add(rel);
      const where = `${rel}:${block.line} (${block.kind})`;
      const problems = [];
      const name = moduleFor(block.kind);
      if (!name) problems.push(`unknown kind ${JSON.stringify(block.kind)}`);
      if (block.body.length > MAX_INPUT) problems.push(`the input is over ${MAX_INPUT} characters`);
      if (!STANDALONE.has(rel)) {
        const first = block.pair && block.pair.fences[0];
        if (!first || !/^rust\b/.test(first.info) || first === block) {
          problems.push("not in a rich-playground-pair div after its Rust");
        }
      }
      if (name) {
        const options = JSON.stringify(block.options);
        for (const fn of ["render", "render_html"]) {
          try {
            const out = modules[name][fn](block.kind, block.body, options);
            if (!out.trim()) problems.push(`${fn} rendered nothing`);
          } catch (error) {
            if (error instanceof WebAssembly.RuntimeError) {
              problems.push(`${fn} panicked (see the message above)`);
              panics += 1;
              modules[name] = await load(MODULES, name, panics);
            } else {
              problems.push(`${fn}: ${error}`);
            }
          }
        }
      }
      kinds.set(block.kind, (kinds.get(block.kind) || 0) + 1);
      if (problems.length) {
        failures += 1;
        console.log(`FAIL ${where}: ${problems.join("; ")}`);
      }
    }
  }
  failures += await checkCli(stamp, cli, pages, kinds);
  count += kinds.get("cli") || 0;
  const summary = [...kinds].map(([kind, n]) => `${kind} ${n}`).join(", ");
  console.log(`${count} playground blocks on ${pages.size} pages (${summary})`);
  if (!count) {
    console.log("FAIL no playground blocks found");
    failures += 1;
  }
  if (failures) {
    console.error(`${failures} block(s) failed`);
    process.exit(1);
  }
}

// Tier 2's blocks, a few at a time (each in its own worker thread).
async function checkCli(stamp, cli, pages, kinds) {
  if (!stamp.cli) {
    if (!cli.some(({ block }) => block.kind === "cli")) return 0;
    console.log("FAIL the stamp has no cli module: run playground/build.sh with the wasm32-wasip1 target");
    return 1;
  }
  const wasm = path.join(MODULES, stamp.cli.wasm);
  let failures = 0;
  let next = 0;
  const one = async () => {
    while (next < cli.length) {
      const { rel, block } = cli[next++];
      const result = await runBlock(wasm, block.body, { timeout: 60000 });
      const where = `${rel}:${block.line}`;
      if (block.kind === "cli") {
        pages.add(rel);
        kinds.set("cli", (kinds.get("cli") || 0) + 1);
        if (!result.ok) {
          failures += 1;
          const failed = result.lines.filter((l) => l.code !== 0).map((l) => `\`${l.line}\` exited ${l.code}`);
          console.log(`FAIL ${where} (cli): ${result.error || failed.join("; ") || "no commands"}`);
          if (!result.error) console.log(result.output.split("\n").slice(-8).join("\n"));
        }
      } else if (result.ok) {
        failures += 1;
        console.log(`FAIL ${where}: this CLI example runs in the docs terminal; mark it { .rich-playground data-kind="cli" }`);
      }
    }
  };
  await Promise.all(Array.from({ length: 4 }, one));
  return failures;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error);
    process.exit(1);
  });
}
