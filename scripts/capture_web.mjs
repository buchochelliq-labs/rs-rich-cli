// Screenshots of rs-rich-web's browser pages, for docs/guide/intuituive/web.md.
//
//     node scripts/capture_web.mjs [--only xterm,dom,dom-tree,program]
//
// It builds and starts `cargo run -p rs-rich-web --example serve` on a free
// port, reads the address it prints (with its token), opens it in headless
// Chromium at a fixed size, types into the app, and saves PNGs of the page
// to docs/media/web/:
//
//   xterm.png     the example app on the xterm.js page (the default)
//   dom.png       the same app drawn by the DOM renderer (`&renderer=dom`)
//   dom-tree.png  the DOM page with the accessibility tree's elements outlined
//                 and labelled with their ARIA roles (they are invisible on
//                 the real page; the outline is added only for the picture)
//   program.png   vim, served by `rich serve` (rs-rich-cli's `serve` feature),
//                 showing the page's own app.js: a program that is not ours
//
// Browser screenshots are regenerated at release time, by hand; CI does not
// check them, because the fonts (and so the pixels) differ between machines
// (decision 7 of the 0.0.19 plan). Text screenshots, which CI does check,
// come from scripts/capture_guide.py and `rich record --check`.
//
// Needs Node 22 and the `playwright` package with its Chromium: found next to
// this script's node_modules, then in the global npm root (`npm i -g
// playwright`). Set CARGO_TARGET_DIR to share a build directory.

import { spawn, spawnSync, execSync } from "node:child_process";
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUT = path.join(ROOT, "docs", "media", "web");
const TARGET = process.env.CARGO_TARGET_DIR || path.join(ROOT, "target");
// 82 columns by 22 rows of xterm.js's 14px cells, with the page's padding.
const VIEWPORT = { width: 700, height: 382 };

function loadPlaywright() {
  const require = createRequire(import.meta.url);
  try {
    return require("playwright");
  } catch {
    const global = execSync("npm root -g", { encoding: "utf8" }).trim();
    return createRequire(path.join(global, "noop.js"))("playwright");
  }
}

function cargoBuild(args) {
  const result = spawnSync("cargo", ["build", ...args], { cwd: ROOT, stdio: "inherit" });
  if (result.status !== 0) {
    throw new Error(`cargo build ${args.join(" ")} failed`);
  }
}

// Start a server and resolve with its address once it prints one.
function serve(program, args) {
  const child = spawn(program, args, { cwd: ROOT, stdio: ["ignore", "pipe", "inherit"] });
  return new Promise((resolve, reject) => {
    let seen = "";
    const timer = setTimeout(() => reject(new Error(`${program} printed no address`)), 30000);
    child.stdout.on("data", (chunk) => {
      seen += chunk;
      const url = seen.match(/http:\/\/\S+/);
      if (url) {
        clearTimeout(timer);
        resolve({ url: url[0], stop: () => child.kill("SIGINT") });
      }
    });
    child.on("exit", (code) => reject(new Error(`${program} exited with ${code}`)));
  });
}

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// Type into the example app: two lines said, then the counter up to three.
async function driveExample(page) {
  for (const line of ["Hello from the browser", "Each tab runs its own copy"]) {
    await page.keyboard.type(line, { delay: 15 });
    await page.keyboard.press("Enter");
    await pause(150);
  }
  await page.keyboard.press("Tab");
  for (let i = 0; i < 3; i++) {
    await page.keyboard.press("+");
  }
  // Let the clock tick past a few seconds and the last frame arrive.
  await pause(3200);
}

async function shoot(page, name) {
  const file = path.join(OUT, `${name}.png`);
  await page.screenshot({ path: file, clip: { x: 0, y: 0, ...VIEWPORT } });
  console.log(`wrote ${path.relative(ROOT, file)}`);
}

// Outline the accessibility tree's elements and label each with its role.
const TREE_CSS = `
  #tree * { outline: 1px solid rgba(255, 196, 0, 0.85); outline-offset: -1px; }
  #tree [role]::before {
    content: attr(role); position: absolute; right: 2px; top: 0;
    font: 11px/1.3 system-ui, sans-serif; color: #101010;
    background: rgba(255, 196, 0, 0.9); padding: 0 3px; border-radius: 2px;
  }
  #tree .item { outline-color: rgba(0, 200, 255, 0.9); }
  #tree .item[role]::before { background: rgba(0, 200, 255, 0.9); left: 2px; right: auto; }
`;

async function main() {
  const only = process.argv.includes("--only")
    ? process.argv[process.argv.indexOf("--only") + 1].split(",")
    : ["xterm", "dom", "dom-tree", "program"];
  mkdirSync(OUT, { recursive: true });
  const { chromium } = loadPlaywright();
  const browser = await chromium.launch();
  // The tree outline is a style the page's content security policy would
  // refuse; bypassing it changes nothing else the page does.
  const context = await browser.newContext({ viewport: VIEWPORT, bypassCSP: true });

  try {
    const pages = only.filter((name) => name !== "program");
    if (pages.length) {
      cargoBuild(["-p", "rs-rich-web", "--example", "serve"]);
      const example = path.join(TARGET, "debug", "examples", "serve");
      for (const name of pages) {
        // A server per page, so each picture starts from a fresh session.
        const server = await serve(example, ["127.0.0.1:0"]);
        try {
          const page = await context.newPage();
          const url = name === "xterm" ? server.url : `${server.url}&renderer=dom`;
          await page.goto(url);
          await page.waitForSelector(name === "xterm" ? ".xterm-rows" : "#grid .line");
          await pause(500);
          if (name !== "xterm") {
            await page.click("#screen");
          }
          await driveExample(page);
          if (name === "dom-tree") {
            await page.addStyleTag({ content: TREE_CSS });
          }
          await shoot(page, name);
          await page.close();
        } finally {
          server.stop();
        }
      }
    }

    if (only.includes("program")) {
      cargoBuild(["-p", "rs-rich-cli", "--features", "serve"]);
      const rich = path.join(TARGET, "debug", "rich");
      // vim with no vimrc and no swap file, so the picture is the same on any
      // machine that has vim: syntax colours and line numbers on, showing the
      // xterm.js page's own script.
      const vim = ["vim", "-N", "-n", "-R", "-u", "NONE", "-i", "NONE",
        "-c", "syntax on", "-c", "set number nowrap laststatus=2",
        "crates/rich-web/assets/app.js"];
      const server = await serve(rich, ["serve", "--port", "0", "--", ...vim]);
      try {
        const page = await context.newPage();
        await page.goto(server.url);
        await page.waitForSelector(".xterm-rows");
        await pause(1500);
        await shoot(page, "program");
        await page.close();
      } finally {
        server.stop();
      }
    }
  } finally {
    await browser.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
