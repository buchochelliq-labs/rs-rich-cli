// The marketplace's web site (site/, published by .github/workflows/pages.yml).
//
//     node scripts/check_site.mjs --unit                  # the logic, under Node
//     node scripts/check_site.mjs                         # and the page in Chromium
//     node scripts/check_site.mjs --screenshots DIR       # and screenshots, live stats
//
// A port of rs-rich-cli's scripts/check_marketplace_page.mjs (the docs
// site's Marketplace page) to the stand-alone site.
//
// First the page's logic under Node, against the fixture marketplace
// (scripts/fixtures/site: rs-rich-cli's docs/tapes/fixtures/marketplace,
// 13 official listings, one approved and two community ones, and made-up
// stats): search over ids, names, descriptions, capabilities and authors;
// the kind, tier, official and recommended filters; every sort; the
// install and `cargo add` commands; the "Authored by" wording, which never
// lets a listing claim to be rs-rich's; and reading stats.json, nulls and
// bad links included.
//
// Then, unless --unit, site/ in headless Chromium (Playwright), served from
// a local http server with the fixture's index.json and listings answered
// in place of the site's own, and stats.json's URL answered from the
// fixture. It checks that:
//
//   - the page lists every package, official first, each with its badge,
//     and the badges link to the tiers in the design note;
//   - typing, the filters and the sorts change the list, and the count
//     (an ARIA live region) says so;
//   - a deep link (`#id`) opens and focuses its package, clearing a filter
//     that hides it;
//   - the copy buttons copy the install and `cargo add` commands;
//   - the keyboard moves from the search box through the packages;
//   - each package links to its listing JSON, its source and its reviews;
//   - a listing that claims to be rs-rich's, or holds HTML, shows as text,
//     with no badge it was not given;
//   - stats show as counts, "—" where null; the page works without them;
//     a review link outside the marketplace's discussions is dropped;
//   - before the first index (a 404) it says the index is being signed,
//     and an unreachable index offers to try again;
//   - the Content-Security-Policy lets the page do all of this and blocks
//     a request anywhere else; requests carry no credentials or referrer;
//     nothing logs an error;
//   - without JavaScript, the page links packages/;
//   - no page of the site embeds anything from another site;
//   - the site as scripts/build_site.py builds it from this checkout (the
//     real index.json and the listings it signed) lists every package the
//     index names, and its packages/index.html lists them without a script;
//   - nothing scrolls sideways on a phone.
//
// --screenshots DIR also saves the built site with the real index and the
// live stats.json (fetched from raw.githubusercontent.com) in light and
// dark at desktop width, and on a phone.
//
// Needs Node 22, python3, git, and the `playwright` package with its
// Chromium (next to this script's node_modules, then the global npm root).

import { createServer } from "node:http";
import { createRequire } from "node:module";
import { createHash } from "node:crypto";
import { execFileSync, execSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SITE = path.join(ROOT, "site");
const FIXTURE = path.join(ROOT, "scripts", "fixtures", "site");
const STATS = "https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-marketplace/stats/stats.json";
const TIERS = "https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/marketplace.md#tiers";

const args = process.argv.slice(2);
const UNIT_ONLY = args.includes("--unit");
const shotsAt = args.indexOf("--screenshots");
const SHOTS = shotsAt >= 0 ? path.resolve(args[shotsAt + 1]) : null;

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
  vm.runInNewContext(readFileSync(path.join(SITE, "assets", "marketplace.js"), "utf8"), context);
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
  ".svg": "image/svg+xml", ".png": "image/png", ".woff2": "font/woff2", ".minisig": "text/plain", ".txt": "text/plain",
};

// A static server, as GitHub Pages serves the artifact: a directory's
// index.html, and a 404 for anything missing.
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

// Every <img>, <script>, <link>, … of every HTML page under `dir` that
// names another site.
function externalEmbeds(dir) {
  const external = [];
  const walk = (at) => {
    for (const name of readdirSync(at)) {
      const file = path.join(at, name);
      if (statSync(file).isDirectory()) walk(file);
      else if (name.endsWith(".html")) {
        const html = readFileSync(file, "utf8");
        for (const tag of html.match(/<(img|script|link|iframe|source|video|audio|object|embed)\b[^>]*>/gi) || []) {
          const url = (tag.match(/\b(?:src|href|data|poster)\s*=\s*"([^"]*)"/i) || [])[1] || "";
          if (/^(https?:)?\/\//i.test(url)) external.push(`${path.relative(dir, file)}: ${url}`);
        }
      }
    }
  };
  walk(dir);
  return external;
}

// The site as the pages workflow builds it, from this checkout.
function buildSite() {
  const out = mkdtempSync(path.join(os.tmpdir(), "mkt-site-"));
  execFileSync("python3", [path.join(ROOT, "scripts", "build_site.py"), "--output", out], { stdio: "inherit" });
  return out;
}

// Records CSP violations where the test can read them.
const RECORD_CSP = () => {
  window.__csp = [];
  document.addEventListener("securitypolicyviolation", (e) => window.__csp.push(`${e.violatedDirective} ${e.blockedURI}`));
};

async function browser() {
  const server = await serve(SITE);
  const base = `http://127.0.0.1:${server.address().port}/`;
  const built = buildSite();
  const builtServer = await serve(built);
  const builtBase = `http://127.0.0.1:${builtServer.address().port}/`;
  const { chromium } = loadPlaywright();
  const chrome = await chromium.launch();
  const errors = [];
  const outside = [];
  const sent = [];
  let probing = false; // the CSP probes below log errors on purpose
  try {
    // mode: "ok", "unsigned" (index.json is a 404), "down" (refused),
    // "no-stats", "slow-stats" (stats.json never answers), "hostile" (a
    // listing that claims to be rs-rich's).
    async function open(mode, hash = "", scheme = "light", viewport = { width: 1280, height: 900 }) {
      const context = await chrome.newContext({ permissions: ["clipboard-read", "clipboard-write"], colorScheme: scheme, viewport });
      await context.addInitScript(RECORD_CSP);
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
      await context.route(() => true, async (route) => {
        const url = route.request().url();
        if (url.startsWith(base)) {
          const rel = url.slice(base.length).split("#")[0];
          if (rel !== "index.json" && !/^packages\/[^/]+\.json$/.test(rel)) return route.continue();
          sent.push({ url, headers: route.request().headers() });
          if (mode === "down" && rel === "index.json") return route.abort("connectionrefused");
          if (mode === "unsigned" && rel === "index.json") return route.fulfill({ status: 404, body: "not found" });
          if (files[rel]) return route.fulfill({ status: 200, body: files[rel], headers: { "content-type": "application/json" } });
          return route.fulfill({ status: 404, body: "not found" });
        }
        if (url === STATS) {
          sent.push({ url, headers: route.request().headers() });
          if (mode === "slow-stats") return new Promise(() => {});
          if (mode === "no-stats") return route.fulfill({ status: 404, body: "404: Not Found", headers: { "access-control-allow-origin": "*" } });
          const body = JSON.parse(readFileSync(path.join(FIXTURE, "stats.json"), "utf8"));
          if (mode === "hostile") {
            // A hostile stats branch: a review link to somewhere else.
            body.packages.imposter = { downloads: { total: 5, versions: {} }, reviews: { url: "https://github.com/mallory/phish/discussions/1", up: 99, down: 0, comments: 9 } };
          }
          return route.fulfill({ status: 200, body: JSON.stringify(body), headers: { "access-control-allow-origin": "*", "content-type": "text/plain" } });
        }
        outside.push(url);
        return route.abort();
      });
      const page = await context.newPage();
      const expected = mode === "down" || mode === "unsigned" || mode === "no-stats" || mode === "slow-stats";
      page.on("console", (m) => m.type() === "error" && !expected && !probing && errors.push(`${mode}: ${m.text()}`));
      page.on("pageerror", (e) => errors.push(`${mode}: ${e}`));
      await page.goto(`${base}${hash}`);
      return { page, context };
    }
    const cards = (page) => page.locator(".mkt-card");
    const cardIds = (page) => page.locator(".mkt-card").evaluateAll((els) => els.map((e) => e.id));
    const csp = (page) => page.evaluate(() => window.__csp);

    // The list.
    let { page, context } = await open("ok");
    await page.locator(".mkt-card").first().waitFor();
    same(await cards(page).count(), 16, "every package is listed");
    const ids = await cardIds(page);
    check(ids.slice(0, 13).every((id) => !["shout", "solarized", "status-icons"].includes(id)), "official packages come first");
    same(await page.locator("#mkt-count").textContent(), "16 packages", "the count");
    same(await page.locator("#mkt-count").getAttribute("aria-live"), "polite", "the count is a live region");
    same(await page.locator("#textcase .mkt-badge--official").count(), 1, "an official package has the Official badge");
    same(await page.locator("#textcase .mkt-badge--official svg").count(), 1, "with its shield");
    same((await page.locator("#textcase .mkt-badge--official").getAttribute("aria-label")).startsWith("Official, by rs-rich"), true, "the badge has an accessible name");
    same(await page.locator("#textcase .mkt-badge--official").getAttribute("href"), TIERS, "the badge links to the tiers in the design note");
    same(await page.locator("#shout .mkt-badge--official").count(), 0, "a community package has no Official badge");
    same(await page.locator("#shout .mkt-badge--community").textContent(), "Community", "a community badge");
    check((await page.locator("#shout .mkt-community-note").textContent()).includes("not by rs-rich"), "the community note");
    same(await page.locator("#status-icons .mkt-badge--approved").textContent(), "Approved", "reviewed shows as Approved");
    same(await page.locator("#textcase .mkt-badge--recommended").count(), 1, "a recommended package has the mark");
    check((await page.locator("#textcase .mkt-badge--recommended").getAttribute("title")).includes("sandboxed first plugin"), "the note on hover");
    same(await page.locator("#textcase .mkt-author").textContent(), "Authored by rs-rich", "official: authored by rs-rich");
    same(await page.locator("#shout .mkt-author").textContent(), "Authored by Example Author from github.com/example/rich-shout", "authored by, and the source");

    // Links.
    same(await page.locator("#textcase .mkt-links a", { hasText: "Listing JSON" }).getAttribute("href"), "packages/textcase.json", "a link to the listing's JSON");
    const listingJson = await page.evaluate(() => fetch("packages/textcase.json").then((r) => r.json()));
    same(listingJson.id, "textcase", "which is the listing");
    same(await page.locator("#shout .mkt-links a", { hasText: "Source" }).getAttribute("href"), "https://github.com/example/rich-shout", "a link to its source");
    same(await page.locator(".site-header a", { hasText: "How trust works" }).getAttribute("href"), TIERS, "the header links to the design note");
    same(await page.locator(".site-header a", { hasText: "Publish a package" }).getAttribute("href"), "https://github.com/buchochelliq-labs/rs-rich-marketplace#submitting-a-listing", "and to the README's publishing section");
    same(await page.locator(".site-header a", { hasText: "Docs" }).getAttribute("href"), "https://buchochelliq-labs.github.io/rs-rich-cli/latest/", "and to the docs");
    check((await page.locator(".trust").textContent()).includes("verifies the index's minisign"), "the page says rich plugin install verifies the signature");
    same(await page.locator("img:not([alt])").count(), 0, "every image has alt text");

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
    check((await page.locator("#textcase .mkt-dl").textContent()).includes("packages/textcase.json"), "and the listing");
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

    // The Content-Security-Policy: all of the above was allowed, and a
    // request anywhere else is not.
    same(await csp(page), [], "the CSP allowed everything the page did");
    probing = true;
    const elsewhere = await page.evaluate(() => fetch("https://example.com/x").then(() => "sent", () => "blocked"));
    same(elsewhere, "blocked", "a request to another site is blocked");
    check((await csp(page)).some((v) => v.startsWith("connect-src") && v.includes("example.com")), "by the CSP's connect-src");
    const otherRaw = await page.evaluate(() => fetch("https://raw.githubusercontent.com/someone/else/main/x.json").then(() => "sent", () => "blocked"));
    same(otherRaw, "blocked", "and so is any other raw.githubusercontent.com file");
    const html = await page.evaluate(() => {
      try {
        document.body.insertAdjacentHTML("beforeend", "<b>x</b>");
        return "inserted";
      } catch (e) {
        return "refused";
      }
    });
    same(html, "refused", "Trusted Types refuse an HTML string");
    await page.waitForTimeout(200);
    probing = false;
    await context.close();

    ({ page, context } = await open("ok", "#shout"));
    await page.waitForFunction(() => document.activeElement && document.activeElement.id === "shout");
    check(await page.locator("#shout details").evaluate((d) => d.open), "a deep link opens and focuses its package");
    check((await page.locator("#shout .mkt-versions").textContent()).includes("Yanked: The `upper` transform"), "a yanked version and its reason");
    await context.close();

    // A phone.
    ({ page, context } = await open("ok", "#textcase", "light", { width: 390, height: 844 }));
    await page.waitForFunction(() => document.activeElement && document.activeElement.id === "textcase");
    check(await page.evaluate(() => document.documentElement.scrollWidth <= 390), "no sideways scrolling on a phone");
    await context.close();

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

    // A stats request that never answers holds nothing back.
    ({ page, context } = await open("slow-stats"));
    await page.locator(".mkt-card").first().waitFor({ timeout: 20000 });
    same(await cards(page).count(), 16, "the listings show when stats.json stalls");
    check((await page.locator("#textcase .mkt-stats").textContent()).startsWith("— downloads"), "with — for the counts");
    await context.close();

    // Before the first index.
    ({ page, context } = await open("unsigned"));
    await page.locator(".mkt-notice-title").waitFor();
    same(await page.locator(".mkt-notice-title").textContent(), "The marketplace's first index is being signed.", "before the first index");
    same(await page.locator(".mkt-controls").isVisible(), false, "with no controls");
    await context.close();

    // Unreachable.
    ({ page, context } = await open("down"));
    await page.locator(".mkt-notice--error").waitFor();
    check((await page.locator(".mkt-notice-title").textContent()).startsWith("Could not read the marketplace"), "an unreachable index");
    same(await page.getByRole("button", { name: "Try again" }).count(), 1, "offers to try again");
    await context.close();

    same(outside, [], "the page asks nothing of anywhere but this site and stats.json");
    check(sent.length > 0 && sent.every((r) => !r.headers.referer && !r.headers.cookie && !r.headers.authorization), "requests carry no referrer or credentials");

    // Without JavaScript.
    context = await chrome.newContext({ javaScriptEnabled: false });
    page = await context.newPage();
    await page.goto(base);
    check(await page.locator("noscript a[href='packages/']").isVisible(), "without JavaScript, the page links packages/");
    await context.close();

    // The site as the workflow builds it, from this checkout.
    const index = JSON.parse(readFileSync(path.join(ROOT, "index.json"), "utf8"));
    const listed = Object.keys(index.listings).sort();
    context = await chrome.newContext();
    await context.addInitScript(RECORD_CSP);
    const asked = [];
    await context.route(() => true, (route) => {
      const url = route.request().url();
      if (url.startsWith(builtBase)) return route.continue();
      if (url === STATS) return route.fulfill({ status: 200, body: readFileSync(path.join(FIXTURE, "stats.json")), headers: { "access-control-allow-origin": "*" } });
      asked.push(url);
      return route.abort();
    });
    page = await context.newPage();
    page.on("console", (m) => m.type() === "error" && errors.push(`built: ${m.text()}`));
    page.on("pageerror", (e) => errors.push(`built: ${e}`));
    await page.goto(builtBase, { waitUntil: "networkidle" });
    await page.locator(".mkt-card").first().waitFor();
    same((await cardIds(page)).sort(), listed, "the built site lists every package the real index names");
    check(!(await page.locator(".mkt-meta").textContent()).includes("left out"), "and leaves none out (every listing's hash matches)");
    same(await csp(page), [], "with nothing the CSP refuses");
    const fonts = await page.evaluate(async () => {
      await document.fonts.ready;
      return [...document.fonts].filter((f) => f.status === "loaded").map((f) => f.family.replace(/"/g, ""));
    });
    check(fonts.includes("Roboto"), "the site's own Roboto loads");
    same(asked, [], "the built site asks nothing of another site");
    await page.goto(`${builtBase}packages/`);
    same((await page.locator(".static-list a").evaluateAll((as) => as.map((a) => a.getAttribute("href")))).sort(), listed.map((id) => `${id}.json`), "packages/ lists every listing");
    same(await page.locator("script").count(), 0, "without a script");
    await context.close();
    same(externalEmbeds(built), [], "no page of the site embeds anything from another site");

    same(errors, [], "nothing logs an error");

    if (SHOTS) await screenshots(chrome, builtBase);
  } finally {
    await chrome.close();
    server.close();
    builtServer.close();
    rmSync(built, { recursive: true, force: true });
  }
}

// The built site with the real index and the live stats.json.
async function screenshots(chrome, builtBase) {
  mkdirSync(SHOTS, { recursive: true });
  const shots = [
    ["desktop-light.png", "light", { width: 1280, height: 1000 }, ""],
    ["desktop-dark.png", "dark", { width: 1280, height: 1000 }, ""],
    ["desktop-detail-light.png", "light", { width: 1280, height: 1000 }, "#textcase"],
    ["desktop-detail-dark.png", "dark", { width: 1280, height: 1000 }, "#textcase"],
    ["phone-light.png", "light", { width: 390, height: 844 }, ""],
    ["phone-dark.png", "dark", { width: 390, height: 844 }, ""],
    ["phone-detail.png", "light", { width: 390, height: 844 }, "#mermaid"],
  ];
  for (const [name, colorScheme, viewport, hash] of shots) {
    const context = await chrome.newContext({ colorScheme, viewport, deviceScaleFactor: viewport.width < 500 ? 2 : 1 });
    const page = await context.newPage();
    await page.goto(`${builtBase}${hash}`, { waitUntil: "networkidle" });
    await page.locator(".mkt-card").first().waitFor();
    await page.evaluate(() => document.fonts.ready);
    if (hash) await page.waitForFunction((id) => document.activeElement && document.activeElement.id === id, hash.slice(1));
    const meta = await page.locator(".mkt-meta").textContent();
    console.log(`${name}: ${meta}`);
    await page.screenshot({ path: path.join(SHOTS, name), fullPage: name.endsWith("light.png") && !hash && viewport.width > 500 });
    await context.close();
  }
}

unit();
if (!UNIT_ONLY) await browser();
if (failures) {
  console.error(`${failures} check(s) failed`);
  process.exit(1);
}
console.log("all checks passed");
