// The docs' Marketplace page (docs/marketplace.md, docs/assets/marketplace.js).
//
//     node scripts/check_marketplace_page.mjs --unit          # the logic, under Node
//     node scripts/check_marketplace_page.mjs [SITE] [--screenshots DIR]
//
// First the page's logic under Node, against the fixture marketplace
// (docs/tapes/fixtures/marketplace): search over ids, names, descriptions,
// capabilities and authors; the kind, tier, official and recommended
// filters; every sort; the install and `cargo add` commands; the
// "Authored by" wording, which never lets a listing claim to be rs-rich's;
// and reading stats.json, nulls and bad links included.
//
// Then, unless --unit, the page in headless Chromium (Playwright), on SITE
// (a `mkdocs build` output; default site/dev, else site), with the
// marketplace's URLs answered from the fixture and every other outside
// request refused. It checks that:
//
//   - the page lists every package, official first, each with its badge,
//     and the official badge links to the tiers;
//   - typing, the filters and the sorts change the list, and the count
//     says so;
//   - a deep link (`#id`) opens and focuses its package, clearing a filter
//     that hides it;
//   - the copy buttons copy the install and `cargo add` commands;
//   - the keyboard moves from the search box through the packages;
//   - a listing that claims to be rs-rich's, or holds HTML, shows as text,
//     with no badge it was not given;
//   - stats show as counts, "—" where null; the page works without them;
//     a review link outside the marketplace's discussions is dropped;
//   - before the first index (a 404) it says the index is being signed,
//     and an unreachable marketplace offers to try again;
//   - the page's requests go only to the marketplace, without credentials
//     or a referrer (the theme's web font and repository facts, on every
//     page, are refused and not counted); nothing logs an error.
//
// --screenshots DIR saves the page in light and dark there.
//
// Needs Node 22 and the `playwright` package with its Chromium (next to
// this script's node_modules, then the global npm root).

import { createServer } from "node:http";
import { createRequire } from "node:module";
import { createHash } from "node:crypto";
import { execSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync, mkdirSync } from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURE = path.join(ROOT, "docs", "tapes", "fixtures", "marketplace");
const MAIN = "https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-marketplace/main/";
const STATS = "https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-marketplace/stats/stats.json";
const THEME_REQUESTS = ["https://fonts.googleapis.com/", "https://api.github.com/repos/buchochelliq-labs/rs-rich-cli"];

const args = process.argv.slice(2);
const UNIT_ONLY = args.includes("--unit");
const shotsAt = args.indexOf("--screenshots");
const SHOTS = shotsAt >= 0 ? path.resolve(args[shotsAt + 1]) : null;
const SITE = path.resolve(
  args.find((a, i) => !a.startsWith("--") && args[i - 1] !== "--screenshots") ||
    (existsSync(path.join(ROOT, "site", "dev")) ? path.join(ROOT, "site", "dev") : path.join(ROOT, "site")),
);

let failures = 0;
function check(ok, what) {
  console.log(`${ok ? "ok  " : "FAIL"} ${what}`);
  if (!ok) failures += 1;
}
function same(actual, expected, what) {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  check(ok, ok ? what : `${what}: got ${JSON.stringify(actual)}, expected ${JSON.stringify(expected)}`);
}

// The logic, as the page defines it: the script run without a document.
function loadLogic() {
  const context = { module: { exports: {} }, URL, console };
  vm.runInNewContext(readFileSync(path.join(ROOT, "docs", "assets", "marketplace.js"), "utf8"), context);
  return context.module.exports;
}

function fixtureListings() {
  return readdirSync(path.join(FIXTURE, "packages"))
    .filter((f) => f.endsWith(".json"))
    .map((f) => JSON.parse(readFileSync(path.join(FIXTURE, "packages", f), "utf8")));
}

function unit() {
  const L = loadLogic();
  const listings = fixtureListings();
  const ids = (list) => list.map((l) => l.id).sort();
  const stats = L.readStats(JSON.parse(readFileSync(path.join(FIXTURE, "stats.json"), "utf8")));

  same(listings.length, 16, "the fixture has 16 listings");
  same(ids(L.filter(listings, { query: "rot13" })), ["wordplay"], "a capability's name matches (rot13)");
  same(ids(L.filter(listings, { query: "upcase" })), ["textcase"], "a transform matches (upcase)");
  same(ids(L.filter(listings, { query: "Example Artist" })), ["status-icons"], "an author matches");
  same(ids(L.filter(listings, { query: "  DARK   theme " })), ["dusk", "high-contrast", "ocean"], "every word must match, in any case");
  check(L.filter(listings, { query: "theme" }).length >= 6, "theme matches the kind and theme capabilities");
  same(ids(L.filter(listings, { kind: "theme" })), ["dusk", "high-contrast", "mono", "ocean", "paper", "solarized"], "the kind filter");
  same(ids(L.filter(listings, { tier: "community" })), ["shout", "solarized"], "the tier filter");
  same(L.filter(listings, { officialOnly: true }).length, 13, "official only");
  same(ids(L.filter(listings, { recommendedOnly: true })), ["status-icons", "textcase"], "recommended only");
  same(ids(L.filter(listings, { kind: "theme", officialOnly: true, query: "light" })), ["paper"], "filters combine");

  const sorted = L.sort(listings, "default", stats.packages);
  same(sorted.slice(0, 13).every((l) => l.tier === "official"), true, "the default sort puts official packages first");
  same(sorted[13].tier, "reviewed", "then approved, then community");
  const names = L.sort(listings, "name").map((l) => l.name.toLowerCase());
  check(names.every((n, i) => !i || names[i - 1].localeCompare(n, "en") <= 0), "sorting by name");
  same(L.sort(listings, "newest")[0].id, "dev", "newest first, by the day a version was listed, then name");
  const byDownloads = L.sort(listings, "downloads", stats.packages);
  const totals = byDownloads.map((l) => (stats.packages[l.id] ? stats.packages[l.id].total : null));
  const counted = totals.filter((n) => n !== null);
  check(counted.every((n, i) => !i || counted[i - 1] >= n) && totals.indexOf(null) >= counted.length, "most downloaded first, the uncounted last");
  const byRating = L.sort(listings, "rating", stats.packages).map((l) => L.rating(stats.packages[l.id]));
  const rated = byRating.filter((r) => r !== null);
  check(rated.every((r, i) => !i || rated[i - 1] >= r), "best rated first");
  check(L.rating({ reviews: { up: 40, down: 2 } }) > L.rating({ reviews: { up: 2, down: 0 } }), "many votes outweigh a few");
  same(L.sort(listings, "recommended").slice(0, 2).map((l) => l.id), ["textcase", "status-icons"], "recommended first, official before approved");

  const byId = Object.fromEntries(listings.map((l) => [l.id, l]));
  same(L.installCommand(byId.textcase), "rich plugin install textcase", "the install command");
  same(L.installCommand(byId.textcase, byId.textcase.versions[1]), "rich plugin install textcase@0.1.0", "a version's install command");
  same(L.installCommand(byId.mermaid), `cargo add rs-rich-mermaid@=${byId.mermaid.versions[0].version}`, "a crate's cargo add line");
  same(L.installCommand(byId.dot), `cargo add rs-rich-diagram@=${byId.dot.versions[0].version} --features plugin`, "a crate's features");
  same(L.newest(byId.shout).version, "0.2.0", "the newest version not yanked");

  same(L.authoredBy(byId.textcase), "Authored by rs-rich", "an official package is authored by rs-rich");
  same(L.authoredBy(byId.shout), "Authored by Example Author from github.com/example/rich-shout", "authors, then the repository");
  const claim = { ...byId.shout, authors: ["rs-rich team", "B", "C", "D", "E"] };
  same(L.authoredBy(claim), 'Authored by "rs-rich team", B, C, and 2 more from github.com/example/rich-shout', "a claim to be rs-rich is quoted, a long list cut");
  same(L.authoredBy({ ...byId.shout, authors: ["x".repeat(60)] }), `Authored by ${"x".repeat(40)}… from github.com/example/rich-shout`, "a long name is cut short");
  same(L.authoredBy({ ...byId.shout, authors: [] }), "Authored by example from github.com/example/rich-shout", "no authors: the repository's owner");
  same(L.authoredBy({ ...byId.shout, tier: "reviewed", authors: undefined, repository: "https://gitlab.com/g/x.git" }), "Authored by g from gitlab.com/g/x", "any host");
  check(L.publisher(byId.shout).startsWith("Community: published by example, not by rs-rich"), "a community package's publisher");
  check(L.publisher(byId["status-icons"]).startsWith("Approved:"), "reviewed shows as Approved");
  same(L.TIER_NAMES.reviewed, "Approved", "the reviewed tier's name");
  same(L.permissionsText("wasm-plugin", [])[0].startsWith("None: sandboxed"), true, "a sandboxed plugin's permissions");
  check(L.permissionsText("crate", ["native-code", "process"]).some((p) => p.includes("not enforced")), "a crate's permissions are declared");
  same(L.capabilityText({ kind: "transform", name: "rot13" }), "Text transform rot13 (rich --transform rot13)", "a capability in words");

  same(stats.packages.textcase.versions["0.1.0"], null, "an uncounted version is null");
  same(L.count(null), "—", "null shows as —");
  same(L.count(1234), "1,234", "a count");
  same(stats.packages.lumis.reviews, null, "no discussion yet: no reviews");
  check(stats.packages.textcase.reviews.url.startsWith("https://github.com/"), "a review link");
  const bad = L.readStats({
    schema_version: 1,
    packages: {
      a: { downloads: { total: -1, versions: { "1.0.0": "9" } }, reviews: { url: "javascript:alert(1)", up: 1, down: 0, comments: 0 } },
      b: { downloads: { total: 3, versions: {} }, reviews: { url: "https://github.com.evil.test/x", up: 1, down: 0, comments: 0 } },
      "Bad Id": { downloads: { total: 1 } },
    },
  });
  same([bad.packages.a.total, bad.packages.a.versions["1.0.0"], bad.packages.a.reviews.url, bad.packages.b.reviews.url], [null, null, null, null], "bad counts and links are dropped");
  same(Object.keys(bad.packages), ["a", "b"], "an invalid id is dropped");
  const D = "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/";
  for (const [link, ok] of [
    [`${D}3`, true],
    ["https://github.com/mallory/phish/discussions/3", false],
    ["https://github.com/buchochelliq-labs/rs-rich-marketplace-evil/discussions/3", false],
    ["https://github.com/buchochelliq-labs/rs-rich-marketplace/issues/3", false],
    [`${D}../../../../mallory/phish`, false],
    [`${D}%2e%2e/%2e%2e/x`, false],
    [`${D}3?redirect=https://evil.test`, false],
    ["http://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/3", false],
    ["https://github.com@evil.test/buchochelliq-labs/rs-rich-marketplace/discussions/3", false],
  ]) {
    const read = L.readStats({ schema_version: 1, packages: { a: { downloads: { total: 1, versions: {} }, reviews: { url: link, up: 1, down: 0, comments: 0 } } } });
    same(read.packages.a.reviews.url, ok ? link : null, `a review link ${ok ? "in" : "outside"} the marketplace's discussions: ${link}`);
  }
  same(L.readStats({ schema_version: 2, packages: {} }), null, "another schema version is ignored");
}

// ---------------------------------------------------------------------------
// In a browser.
// ---------------------------------------------------------------------------

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
  ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".css": "text/css", ".json": "application/json",
  ".svg": "image/svg+xml", ".png": "image/png", ".gif": "image/gif", ".woff2": "font/woff2",
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

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

// The fixture, with `extra` listings added to it and its index.
function market(extra = []) {
  const files = {};
  for (const f of readdirSync(path.join(FIXTURE, "packages"))) files[`packages/${f}`] = readFileSync(path.join(FIXTURE, "packages", f));
  const index = JSON.parse(readFileSync(path.join(FIXTURE, "index.json"), "utf8"));
  for (const listing of extra) {
    const bytes = Buffer.from(JSON.stringify(listing));
    files[`packages/${listing.id}.json`] = bytes;
    index.listings[listing.id] = { path: `packages/${listing.id}.json`, sha256: sha256(bytes) };
  }
  files["index.json"] = Buffer.from(JSON.stringify(index));
  return files;
}

async function browser() {
  if (!existsSync(path.join(SITE, "marketplace", "index.html"))) {
    throw new Error(`${SITE} has no marketplace/index.html: run mkdocs build first`);
  }
  const server = await serve(SITE);
  const base = `http://127.0.0.1:${server.address().port}/`;
  const { chromium } = loadPlaywright();
  const chrome = await chromium.launch();
  const errors = [];
  const outside = [];
  const sent = [];
  try {
    // mode: "ok", "unsigned" (index.json is a 404), "down" (refused),
    // "no-stats", "hostile" (a listing that claims to be rs-rich's).
    async function open(mode, hash = "", scheme = "light") {
      const context = await chrome.newContext({ permissions: ["clipboard-read", "clipboard-write"], colorScheme: scheme, viewport: { width: 1280, height: 900 } });
      const hostile = {
        schema_version: 1, id: "imposter", kind: "theme", name: "✓ Official · by rs-rich <img src=x onerror=\"window.__pwned=1\">",
        description: "★ Recommended <b>bold</b> by rs-rich", tier: "community", license: "MIT",
        authors: ["rs-rich"], repository: "https://github.com/mallory/imposter",
        versions: [{
          version: "1.0.0", published: "2026-10-20", requires: { theme_format: 1 },
          artifact: { url: "https://github.com/mallory/imposter/x.ini", sha256: "0".repeat(64), size: 1 },
          capabilities: [{ kind: "theme", name: "imposter" }], permissions: [],
        }],
      };
      const files = market(mode === "hostile" ? [hostile] : []);
      await context.route((url) => !url.href.startsWith(base), async (route) => {
        const url = route.request().url();
        if (url.startsWith("https://raw.githubusercontent.com/")) sent.push({ url, headers: route.request().headers() });
        if (url === STATS) {
          if (mode === "no-stats") return route.fulfill({ status: 404, body: "404: Not Found", headers: { "access-control-allow-origin": "*" } });
          const body = JSON.parse(readFileSync(path.join(FIXTURE, "stats.json"), "utf8"));
          if (mode === "hostile") {
            // A hostile stats branch: a review link to somewhere else.
            body.packages.imposter = { downloads: { total: 5, versions: {} }, reviews: { url: "https://github.com/mallory/phish/discussions/1", up: 99, down: 0, comments: 9 } };
          }
          return route.fulfill({ status: 200, body: JSON.stringify(body), headers: { "access-control-allow-origin": "*", "content-type": "text/plain" } });
        }
        if (url.startsWith(MAIN)) {
          if (mode === "down") return route.abort("connectionrefused");
          const rel = url.slice(MAIN.length);
          if (mode === "unsigned" && rel === "index.json") return route.fulfill({ status: 404, body: "404: Not Found", headers: { "access-control-allow-origin": "*" } });
          if (files[rel]) return route.fulfill({ status: 200, body: files[rel], headers: { "access-control-allow-origin": "*", "content-type": "text/plain" } });
          return route.fulfill({ status: 404, body: "404: Not Found", headers: { "access-control-allow-origin": "*" } });
        }
        // The theme's own (every page's): its web font and the repository
        // facts in the header. Not the Marketplace page's; refused here.
        if (!THEME_REQUESTS.some((prefix) => url.startsWith(prefix))) outside.push(url);
        return route.abort();
      });
      const page = await context.newPage();
      const local = (message) => (message.location().url || base).startsWith(base);
      page.on("console", (m) => m.type() === "error" && local(m) && mode !== "down" && mode !== "unsigned" && mode !== "no-stats" && errors.push(m.text()));
      page.on("pageerror", (e) => errors.push(String(e)));
      await page.goto(`${base}marketplace/${hash}`);
      return { page, context };
    }
    const cards = (page) => page.locator(".mkt-card");
    const cardIds = (page) => page.locator(".mkt-card").evaluateAll((els) => els.map((e) => e.id));

    // The list.
    let { page, context } = await open("ok");
    await page.locator(".mkt-card").first().waitFor();
    same(await cards(page).count(), 16, "every package is listed");
    const ids = await cardIds(page);
    check(ids.slice(0, 13).every((id) => !["shout", "solarized", "status-icons"].includes(id)), "official packages come first");
    same(await page.locator("#mkt-count").textContent(), "16 packages", "the count");
    same(await page.locator("#textcase .mkt-badge--official").count(), 1, "an official package has the Official badge");
    same((await page.locator("#textcase .mkt-badge--official").getAttribute("aria-label")).startsWith("Official, by rs-rich"), true, "the badge has an accessible name");
    const tiers = await page.locator("#textcase .mkt-badge--official").getAttribute("href");
    check(tiers.endsWith("/design/marketplace/#tiers") && existsSync(path.join(SITE, "design", "marketplace", "index.html")), "the badge links to the tiers");
    same(await page.locator("#shout .mkt-badge--official").count(), 0, "a community package has no Official badge");
    same(await page.locator("#shout .mkt-badge--community").textContent(), "Community", "a community badge");
    check((await page.locator("#shout .mkt-community-note").textContent()).includes("not by rs-rich"), "the community note");
    same(await page.locator("#status-icons .mkt-badge--approved").textContent(), "Approved", "reviewed shows as Approved");
    same(await page.locator("#textcase .mkt-badge--recommended").count(), 1, "a recommended package has the mark");
    check((await page.locator("#textcase .mkt-badge--recommended").getAttribute("title")).includes("sandboxed first plugin"), "the note on hover");
    same(await page.locator("#textcase .mkt-author").textContent(), "Authored by rs-rich", "official: authored by rs-rich");
    same(await page.locator("#shout .mkt-author").textContent(), "Authored by Example Author from github.com/example/rich-shout", "authored by, and the source");

    // Stats.
    const textcaseStats = JSON.parse(readFileSync(path.join(FIXTURE, "stats.json"), "utf8")).packages.textcase;
    check((await page.locator("#textcase .mkt-stats").textContent()).includes(textcaseStats.downloads.total.toLocaleString("en-US")), "the download count");
    same(await page.locator("#textcase .mkt-stats a").getAttribute("href"), textcaseStats.reviews.url, "the reviews link");
    same(await page.locator("#textcase .mkt-stats a").textContent(), "Read or write reviews on GitHub", "the reviews link's words");
    check((await page.locator("#lumis .mkt-stats").textContent()).includes("No reviews yet"), "no discussion yet");
    await page.locator("#textcase summary").click();
    const row = page.locator("#textcase .mkt-versions tbody tr").nth(1);
    same(await row.locator("td.mkt-num").textContent(), "—", "an uncounted version shows —");
    check((await page.locator("#textcase .mkt-dl").textContent()).includes("Official: built and published by rs-rich"), "the detail view says who publishes it");
    check((await page.locator("#textcase .mkt-dl").textContent()).includes("Recommended★ Since 2026-10-26"), "the detail view has the recommendation");
    check((await page.locator(".mkt-meta").textContent()).includes("refreshed nightly, not signed"), "stats are said to be unsigned");

    // Search, filters and sorts.
    await page.getByLabel("Search packages", { exact: true }).fill("rot13");
    same(await cardIds(page), ["wordplay"], "searching a capability");
    same(await page.locator("#mkt-count").textContent(), "1 of 16 packages match", "the count follows");
    await page.getByLabel("Search packages", { exact: true }).fill("nothing like this");
    check((await page.locator(".mkt-empty").textContent()).includes("Nothing matches"), "nothing matches");
    await page.getByLabel("Search packages", { exact: true }).press("Escape");
    same(await cards(page).count(), 16, "Escape clears the search");
    await page.getByLabel("Kind", { exact: true }).selectOption("theme");
    same(await cards(page).count(), 6, "the kind filter");
    await page.getByLabel("Tier", { exact: true }).selectOption("community");
    same(await cardIds(page), ["solarized"], "kind and tier");
    await page.getByLabel("Tier", { exact: true }).selectOption("");
    await page.getByLabel("Kind", { exact: true }).selectOption("");
    await page.getByLabel("Official only").check();
    same(await cards(page).count(), 13, "official only");
    await page.getByLabel("Official only").uncheck();
    await page.getByLabel("Recommended only").check();
    same((await cardIds(page)).sort(), ["status-icons", "textcase"], "recommended only");
    await page.getByLabel("Recommended only").uncheck();
    await page.getByLabel("Sort by", { exact: true }).selectOption("downloads");
    const stats = JSON.parse(readFileSync(path.join(FIXTURE, "stats.json"), "utf8")).packages;
    const top = Object.entries(stats).filter(([, s]) => s.downloads.total !== null).sort((a, b) => b[1].downloads.total - a[1].downloads.total)[0][0];
    same((await cardIds(page))[0], top, "most downloaded first");
    await page.getByLabel("Sort by", { exact: true }).selectOption("rating");
    check((await cardIds(page)).length === 16, "best rated");
    await page.getByLabel("Sort by", { exact: true }).selectOption("default");

    // The keyboard.
    await page.getByLabel("Search packages", { exact: true }).focus();
    await page.keyboard.press("ArrowDown");
    same(await page.evaluate(() => document.activeElement.closest(".mkt-card")?.id), ids[0], "ArrowDown from the search goes to the first package");
    await page.keyboard.press("ArrowDown");
    same(await page.evaluate(() => document.activeElement.closest(".mkt-card")?.id), ids[1], "and on to the next");
    await page.keyboard.press("End");
    same(await page.evaluate(() => document.activeElement.closest(".mkt-card")?.id), ids[15], "End goes to the last");
    await page.keyboard.press("Tab");
    check(await page.evaluate(() => getComputedStyle(document.activeElement).outlineStyle !== "none"), "focus is visible");

    // Copying.
    await page.locator("#textcase > .mkt-command .mkt-copy").click();
    same(await page.evaluate(() => navigator.clipboard.readText()), "rich plugin install textcase", "Copy copies the install command");
    check((await page.locator("#mkt-count").textContent()).startsWith("Copied"), "and says so");
    await page.locator("#mermaid > .mkt-command .mkt-copy").click();
    check((await page.evaluate(() => navigator.clipboard.readText())).startsWith("cargo add rs-rich-mermaid@="), "a crate's copy is its cargo add line");
    await page.locator("#textcase .mkt-versions tbody tr").nth(1).locator(".mkt-copy").click();
    same(await page.evaluate(() => navigator.clipboard.readText()), "rich plugin install textcase@0.1.0", "a version's copy");
    same(await page.locator("#shout .mkt-versions tbody tr").nth(1).locator(".mkt-copy").count(), 0, "a yanked version has no command");

    // A deep link, from a filter that hides it.
    await page.getByLabel("Search packages", { exact: true }).fill("rot13");
    await page.evaluate(() => (location.hash = "#dusk"));
    await page.waitForFunction(() => document.activeElement && document.activeElement.id === "dusk");
    check(await page.locator("#dusk details").evaluate((d) => d.open), "a hash change opens its package");
    same(await page.getByLabel("Search packages", { exact: true }).inputValue(), "", "and clears the filter that hid it");
    if (SHOTS) {
      mkdirSync(SHOTS, { recursive: true });
      await page.evaluate(() => (location.hash = ""));
      await page.evaluate(() => window.scrollTo(0, 0));
      await page.screenshot({ path: path.join(SHOTS, "marketplace-light.png"), fullPage: false });
    }
    await context.close();

    ({ page, context } = await open("ok", "#shout"));
    await page.waitForFunction(() => document.activeElement && document.activeElement.id === "shout");
    check(await page.locator("#shout details").evaluate((d) => d.open), "a deep link opens and focuses its package");
    check((await page.locator("#shout .mkt-versions").textContent()).includes("Yanked: The `upper` transform"), "a yanked version and its reason");
    if (SHOTS) await page.screenshot({ path: path.join(SHOTS, "marketplace-detail-light.png") });
    await context.close();

    if (SHOTS) {
      ({ page, context } = await open("ok", "", "dark"));
      await page.locator(".mkt-card").first().waitFor();
      await page.screenshot({ path: path.join(SHOTS, "marketplace-dark.png") });
      await page.evaluate(() => (location.hash = "#shout"));
      await page.waitForFunction(() => document.activeElement && document.activeElement.id === "shout");
      await page.screenshot({ path: path.join(SHOTS, "marketplace-detail-dark.png") });
      await context.close();
      ({ page, context } = await open("ok", "#textcase"));
      await page.setViewportSize({ width: 390, height: 844 });
      await page.waitForFunction(() => document.activeElement && document.activeElement.id === "textcase");
      check(await page.evaluate(() => document.documentElement.scrollWidth <= 390), "no sideways scrolling on a phone");
      await page.screenshot({ path: path.join(SHOTS, "marketplace-phone.png") });
      await context.close();
    }

    // A listing that claims to be rs-rich's.
    ({ page, context } = await open("hostile", "#imposter"));
    await page.locator("#imposter").waitFor();
    same(await page.locator("#imposter .mkt-badge--official").count(), 0, "a listing's words never make an Official badge");
    same(await page.locator("#imposter .mkt-badge--recommended").count(), 0, "or a Recommended mark");
    same(await page.locator("#imposter img").count(), 0, "a listing's HTML is text");
    check((await page.locator("#imposter .mkt-title").textContent()).includes("<img"), "and shows as written");
    same(await page.locator("#imposter .mkt-author").textContent(), 'Authored by "rs-rich" from github.com/mallory/imposter', "its claim is quoted");
    same(await page.evaluate(() => window.__pwned), undefined, "nothing ran");
    same(await page.locator("#imposter .mkt-stats a").count(), 0, "a review link outside the marketplace's discussions is not shown");
    check((await page.locator("#imposter .mkt-stats").textContent()).includes("99"), "its counts still are");
    await context.close();

    // Without stats.
    ({ page, context } = await open("no-stats"));
    await page.locator(".mkt-card").first().waitFor();
    same(await cards(page).count(), 16, "the page works without stats.json");
    check((await page.locator("#textcase .mkt-stats").textContent()).startsWith("— downloads"), "counts show —");
    check((await page.locator(".mkt-meta").textContent()).includes("not available"), "and it says so");
    await context.close();

    // Before the first index.
    ({ page, context } = await open("unsigned"));
    await page.locator(".mkt-notice-title").waitFor();
    same(await page.locator(".mkt-notice-title").textContent(), "The marketplace's first index is being signed.", "before the first index");
    same(await page.locator(".mkt-controls").isVisible(), false, "with no controls");
    if (SHOTS) await page.screenshot({ path: path.join(SHOTS, "marketplace-unsigned.png") });
    await context.close();

    // Unreachable.
    ({ page, context } = await open("down"));
    await page.locator(".mkt-notice--error").waitFor();
    check((await page.locator(".mkt-notice-title").textContent()).startsWith("Could not read the marketplace"), "an unreachable marketplace");
    same(await page.getByRole("button", { name: "Try again" }).count(), 1, "offers to try again");
    await context.close();

    same(outside, [], "the page asks nothing of anywhere but the marketplace");
    check(sent.length > 0 && sent.every((r) => !r.headers.referer && !r.headers.cookie && !r.headers.authorization), "requests carry no referrer or credentials");
    same(errors, [], "nothing logs an error");
  } finally {
    await chrome.close();
    server.close();
  }
}

unit();
if (!UNIT_ONLY) await browser();
if (failures) {
  console.error(`${failures} check(s) failed`);
  process.exit(1);
}
console.log("all checks passed");
