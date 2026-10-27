// The docs playground in a real browser: headless Chromium (Playwright)
// against a built site, served from a local static server.
//
//     node scripts/check_playground_page.mjs [SITE]     # default: site/dev, else site
//
// SITE is a `mkdocs build` output whose assets/playground/ holds the modules
// (playground/build.sh). It checks that:
//
//   - the playground page offers "Try it", and a click loads the lean
//     module and renders the block into a sandboxed srcdoc iframe;
//   - typing re-renders, and the kind picker switches to `syntax`, which
//     loads the full module (syntect);
//   - "Terminal view" loads xterm.js and draws the same text;
//   - "Copy link" writes the state into the fragment, and opening that link
//     restores it as the textarea's text, with nothing injected into the page;
//   - a guide page's block, its Rust beside it, renders its input;
//   - tier 2: a CLI block's "Try it" loads only the `rich` WASI module and
//     opens a terminal with the block's first command at the prompt; Enter
//     runs it, the next is filled in, a pipe and a builtin work; Stop ends
//     a command that never does and a fresh session answers; "Save as tape"
//     downloads the lines typed; a guide page's CLI example has the button;
//   - tier 3: an `app` block loads the app module only on "Run it", and runs
//     in a Web Worker: `choose` answers keys and the mouse and shows what it
//     returned, the to-do app takes typing, `explore` returns a path, and
//     Stop ends an animating app (its terminal stops changing);
//   - without the stamp (a version built without the module) there is no
//     button, and with JavaScript off the block is a plain code block;
//   - nothing logs an error to the console.
//
// Needs Node 22 and the `playwright` package with its Chromium (found next to
// this script's node_modules, then in the global npm root).

import { createServer } from "node:http";
import { createRequire } from "node:module";
import { execSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SITE = path.resolve(
  process.argv[2] || (existsSync(path.join(ROOT, "site", "dev")) ? path.join(ROOT, "site", "dev") : path.join(ROOT, "site")),
);

function loadPlaywright() {
  const require = createRequire(import.meta.url);
  try {
    return require("playwright");
  } catch {
    const global = execSync("npm root -g", { encoding: "utf8" }).trim();
    return createRequire(path.join(global, "noop.js"))("playwright");
  }
}

const TYPES = {
  ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".mjs": "text/javascript",
  ".css": "text/css", ".json": "application/json", ".wasm": "application/wasm", ".svg": "image/svg+xml",
  ".png": "image/png", ".woff2": "font/woff2", ".gif": "image/gif", ".map": "application/json",
};

function serve(root) {
  const server = createServer((request, response) => {
    let file = path.join(root, decodeURIComponent(new URL(request.url, "http://x").pathname));
    if (!file.startsWith(root)) file = root;
    if (existsSync(file) && statSync(file).isDirectory()) file = path.join(file, "index.html");
    if (!existsSync(file)) {
      response.writeHead(404).end("not found");
      return;
    }
    response.writeHead(200, { "content-type": TYPES[path.extname(file)] || "application/octet-stream" });
    response.end(readFileSync(file));
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

let failures = 0;
function check(ok, what) {
  console.log(`${ok ? "ok  " : "FAIL"} ${what}`);
  if (!ok) failures += 1;
}

async function frameText(page, scope) {
  // The iframe is sandboxed without scripts, but its srcdoc is readable.
  const html = await page.locator(`${scope} .rp-frame`).first().getAttribute("srcdoc");
  return (html || "").replace(/<[^>]*>/g, "").replace(/&quot;/g, '"').replace(/&#x27;/g, "'").replace(/&amp;/g, "&");
}

async function main() {
  if (!existsSync(path.join(SITE, "assets", "playground", "playground.json"))) {
    throw new Error(`${SITE} has no assets/playground/playground.json: run playground/build.sh, then mkdocs build`);
  }
  const server = await serve(SITE);
  const base = `http://127.0.0.1:${server.address().port}/`;
  const { chromium } = loadPlaywright();
  const browser = await chromium.launch();
  const errors = [];
  try {
    const context = await browser.newContext({ permissions: ["clipboard-read", "clipboard-write"] });
    // Hermetic: the theme's web fonts and the like are not fetched. An error
    // about a resource elsewhere is not the playground's.
    await context.route((url) => !url.href.startsWith(base), (route) => route.abort());
    const local = (message) => (message.location().url || base).startsWith(base);
    const page = await context.newPage();
    page.on("console", (message) => message.type() === "error" && local(message) && errors.push(message.text()));
    page.on("pageerror", (error) => errors.push(String(error)));
    const wasm = [];
    page.on("request", (request) => request.url().endsWith(".wasm") && wasm.push(request.url()));

    // The playground page.
    await page.goto(`${base}playground/`);
    await page.locator(".rp-try").first().waitFor();
    check(wasm.length === 0, "no module loads before a click");
    await page.locator(".rp-try").first().click();
    await page.waitForFunction(() => (document.querySelector(".rp-frame")?.srcdoc || "").includes("Hello"));
    check(wasm.length === 1 && wasm[0].includes("/lean/"), "the first click loads the lean module only");
    check((await page.locator(".rp-frame").getAttribute("sandbox")) === "", "the output iframe is sandboxed");
    check((await frameText(page, ".rp-page")).includes("Hello, world! 🚀"), "markup renders, emoji included");

    await page.locator(".rp-input").fill("[bold]Typed[/] in the page");
    await page.waitForFunction(() => (document.querySelector(".rp-frame")?.srcdoc || "").includes("Typed"));
    check(true, "typing re-renders");

    await page.locator(".rp-input").fill("[/oops]");
    await page.locator(".rp-error:not([hidden])").waitFor();
    check((await page.locator(".rp-error").textContent()).includes("closing tag"), "a markup error is shown, not thrown");

    await page.locator(".rp-control select").first().selectOption("syntax");
    await page.waitForFunction(() => (document.querySelector(".rp-frame")?.srcdoc || "").includes("answer"));
    check(wasm.some((url) => url.includes("/full/")), "the syntax kind loads the full module");

    await page.getByRole("button", { name: "Terminal view" }).click();
    await page.locator(".rp-terminal .xterm-rows").waitFor();
    await page.waitForFunction(() => document.querySelector(".rp-terminal .xterm-rows")?.textContent.includes("answer"));
    check(true, "the terminal view loads xterm.js and draws the output");
    await page.getByRole("button", { name: "HTML view" }).click();

    // A shared link: the state in the fragment, restored as text.
    const hostile = '<img src=x onerror="window.__pwned=1">[red]shared[/]';
    await page.locator(".rp-input").fill(hostile);
    await page.getByRole("button", { name: "Copy link" }).click();
    await page.waitForFunction(() => location.hash.startsWith("#rp=0."));
    const link = page.url();
    check(link.length < 9000, `the link is short (${link.length} characters)`);
    const second = await context.newPage();
    second.on("pageerror", (error) => errors.push(String(error)));
    await second.goto(link);
    await second.locator(".rp-input").waitFor();
    check((await second.locator(".rp-input").inputValue()) === hostile, "the link restores the input as text");
    await second.waitForFunction(() => (document.querySelector(".rp-frame")?.srcdoc || "").includes("shared"));
    check((await second.evaluate(() => window.__pwned)) === undefined, "nothing in the link runs");
    check((await second.locator(".rp-control select").first().inputValue()) === "syntax", "the link restores the kind");
    await second.close();

    // A guide page: the block's Rust beside its input, rendered on a click.
    await page.goto(`${base}guide/core/text-and-style/`);
    const pair = page.locator(".rich-playground-pair").first();
    check((await pair.locator(".highlight").first().textContent()).includes("print_str"), "a guide block has its Rust beside it");
    await page.locator(".rp-try").first().click();
    await page.waitForFunction(() => (document.querySelector(".rp-frame")?.srcdoc || "").includes("bold"));
    check(true, "a guide block renders its input");

    // Tier 2: a CLI block's terminal.
    await page.goto(`${base}playground/`);
    wasm.length = 0;
    const cliTry = page.locator('.rich-playground[data-kind="cli"] + .rp-try');
    await cliTry.waitFor();
    await cliTry.click();
    const cliRows = page.locator(".rp-shell .xterm-rows");
    const screen = () => cliRows.textContent();
    const waitFor = (text) =>
      page.waitForFunction((t) => document.querySelector(".rp-shell .xterm-rows")?.textContent.includes(t), text, { timeout: 30000 });
    await waitFor("$ rich README.md");
    check(wasm.length === 1 && wasm[0].includes("/cli/rich.wasm"), "the CLI block loads the rich WASI module only");
    await page.locator(".rp-shell .xterm-helper-textarea").focus();
    await page.keyboard.press("Enter");
    await waitFor("$ rich sales.csv");
    check((await screen()).includes("Install"), "Enter runs the block's first command; the next is filled in");
    await page.keyboard.press("Control+U");
    await page.keyboard.type("seq 1 5 | rich chart --kind spark && ls");
    await page.keyboard.press("Enter");
    await waitFor("photo.png");
    check(true, "a pipe, && and the builtins run");
    // The block's next command is at the prompt: clear it first.
    await page.keyboard.press("Control+U");
    await page.keyboard.type("rich --gif ball.gif --loop 0");
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => document.querySelector(".rp-shell")?.dataset.state === "running");
    await page.waitForTimeout(500);
    await page.locator(".rp-shell .rp-stop").click();
    await waitFor("(stopped");
    await page.waitForFunction(() => document.querySelector(".rp-shell")?.dataset.state === "ready", null, { timeout: 30000 });
    await page.locator(".rp-shell .xterm-helper-textarea").focus();
    await page.keyboard.press("Control+U");
    await page.keyboard.type("echo fresh > note.txt; cat note.txt");
    await page.keyboard.press("Enter");
    await page.waitForFunction(() => /fresh\s/.test(document.querySelector(".rp-shell .xterm-rows")?.textContent || ""));
    check(true, "Stop ends a command that runs on, and a fresh session answers");
    const [download] = await Promise.all([page.waitForEvent("download"), page.locator(".rp-shell .rp-save").click()]);
    const tape = readFileSync(await download.path(), "utf8");
    check(download.suggestedFilename() === "session.tape" && tape.includes('Type "rich README.md"') && tape.includes("Write README.md") && tape.includes("Write note.txt") === false,
      "Save as tape downloads the lines typed and the samples they use");

    await page.goto(`${base}tutorial/06-cli/`);
    // The buttons mount once the stamp has loaded, after the page has.
    await cliTry.first().waitFor({ timeout: 15000 }).catch(() => {});
    check((await cliTry.count()) > 0, "a guide page's CLI example has Try it");

    // Tier 3: apps in a terminal, in a worker.
    await page.goto(`${base}playground/`);
    await page.locator(".rp-try").first().waitFor();
    check(!wasm.some((url) => url.includes("/app/")), "no app module loads before Run it");
    const app = (name) => `.rich-playground[data-app="${name}"] + .rp-editor + .rp-output`;
    const rows = (name) => page.locator(`${app(name)} .xterm-rows`);
    const rowsText = (name) => rows(name).textContent();
    const result = (name) => page.locator(`${app(name)} .rp-result:not([hidden])`);
    async function run(name, shows) {
      await page.locator(`.rich-playground[data-app="${name}"] + .rp-try`).click();
      await rows(name).waitFor();
      await page.waitForFunction(
        ([selector, text]) => document.querySelector(selector)?.textContent.includes(text),
        [`${app(name)} .xterm-rows`, shows],
      );
    }

    await run("choose", "TypeScript");
    check(wasm.some((url) => url.includes("/app/")), "Run it loads the app module, and choose draws its list");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    await result("choose").waitFor();
    check((await result("choose").textContent()).includes("Python"), "keys reach choose, and its answer is shown");

    await page.locator(`${app("choose")}`).locator("xpath=preceding-sibling::div[contains(@class,'rp-editor')][1]")
      .getByRole("button", { name: "Restart" }).click();
    await page.waitForFunction((selector) => !document.querySelector(selector), `${app("choose")} .rp-result:not([hidden])`);
    await page.waitForFunction((selector) => document.querySelector(selector)?.textContent.includes("Haskell"), `${app("choose")} .xterm-rows`);
    const goRow = rows("choose").locator(":scope > div", { hasText: /^\s*Go\s*$/ }).first();
    // Where a reader clicks: on xterm.js's screen, over that row.
    await goRow.scrollIntoViewIfNeeded();
    const box = await goRow.boundingBox();
    await page.mouse.click(box.x + 40, box.y + box.height / 2);
    await page.waitForTimeout(300);
    if (!(await result("choose").count())) await page.keyboard.press("Enter");
    await result("choose").waitFor();
    check((await result("choose").textContent()).includes("Go"), "a click in the terminal picks a line");

    await run("todo", "Water the plants");
    await page.keyboard.type("Feed the cat");
    await page.keyboard.press("Enter");
    await page.waitForFunction((selector) => document.querySelector(selector)?.textContent.includes("Feed the cat"), `${app("todo")} .xterm-rows`);
    check(true, "the to-do app takes typing and adds the to-do");

    await run("explore", "server");
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Enter");
    await result("explore").waitFor();
    check((await result("explore").textContent()).includes("server"), "explore returns the path it was on");

    await run("meters", "");
    await page.waitForTimeout(300);
    const before = await rowsText("meters");
    await page.waitForTimeout(400);
    const moving = (await rowsText("meters")) !== before;
    check(moving, "an animated app keeps drawing on its ticks");
    await page.locator(`${app("meters")}`).locator("xpath=preceding-sibling::div[contains(@class,'rp-editor')][1]")
      .getByRole("button", { name: "Stop" }).click();
    const stopped = await rowsText("meters");
    await page.waitForTimeout(500);
    check((await rowsText("meters")) === stopped, "Stop ends the app's worker: its terminal stops changing");

    // A version built without the module: no stamp, no button.
    const bare = await context.newPage();
    await bare.route("**/playground/playground.json", (route) => route.fulfill({ status: 404, body: "" }));
    await bare.goto(`${base}playground/`);
    await bare.waitForTimeout(500);
    check((await bare.locator(".rp-try").count()) === 0, "without a stamp there is no Try it button");
    await bare.close();

    // JavaScript off: a plain code block.
    const off = await browser.newContext({ javaScriptEnabled: false });
    const plain = await off.newPage();
    await plain.goto(`${base}playground/`);
    check((await plain.locator(".rich-playground code").first().textContent()).includes("[bold magenta]Hello[/]"),
      "with JavaScript off the block shows its input");
    check((await plain.locator(".rp-try").count()) === 0, "with JavaScript off there is no button");
    await off.close();
  } finally {
    await browser.close();
    server.close();
  }
  check(errors.length === 0, `no console errors${errors.length ? `: ${errors.join("; ")}` : ""}`);
  if (failures) {
    console.error(`${failures} check(s) failed`);
    process.exit(1);
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
