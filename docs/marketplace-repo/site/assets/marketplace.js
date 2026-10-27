// The marketplace's web site (site/index.html): the plugin marketplace's
// listings, searched and filtered in the browser. A stand-alone copy of
// rs-rich-cli's docs page (its docs/assets/marketplace.js), served by
// GitHub Pages from this repository (.github/workflows/pages.yml).
//
// It reads `index.json` and each listing it names from the site itself
// (`data-base`, where scripts/build_site.py puts the signed index and the
// listings it signed), checks each listing's SHA-256 against the index,
// and reads the unsigned, display-only `stats.json` from the marketplace's
// `stats` branch on raw.githubusercontent.com (`data-stats`). Requests
// carry no credentials and no referrer; nothing else is fetched or sent,
// and the page's Content-Security-Policy allows nothing else. The index's
// minisign signature is not checked here: `rich plugin install` checks it,
// and every hash, before it writes anything.
//
// Everything a listing or the stats say goes into the page as text
// (`textContent`), never as HTML: the page requires Trusted Types and
// allows no policy, so no HTML string can reach the DOM. The tier badge and
// the Recommended mark are drawn only from a listing's `tier` and
// `recommended` fields.
//
// The pure part (search, filters, sorting, wording) is `MarketplaceLogic`,
// which scripts/check_site.mjs tests under Node.
(function () {
  "use strict";

  const KIND_NAMES = {
    crate: "Crate",
    "wasm-plugin": "WASM plugin",
    "micro-pack": "Micro pack",
    theme: "Theme",
  };
  // The listing format says `reviewed`; people see "Approved".
  const TIER_NAMES = { official: "Official", reviewed: "Approved", community: "Community" };
  const TIER_RANK = { official: 0, reviewed: 1, community: 2 };
  const ID = /^[a-z0-9][a-z0-9._-]{0,63}$/;
  const MAX_AUTHORS = 3;
  const MAX_AUTHOR_CHARS = 40;
  // Where every package's reviews are: its discussion in the marketplace.
  const REVIEWS = "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/";

  function isValidId(id) {
    return typeof id === "string" && ID.test(id);
  }

  function versionsOf(listing) {
    return Array.isArray(listing.versions) ? listing.versions : [];
  }

  // The newest version not yanked, else the newest.
  function newest(listing) {
    const versions = versionsOf(listing);
    return versions.find((v) => !v.yanked) || versions[0] || null;
  }

  // The day a listing last gained a version, for "newest" sorting.
  function latestDay(listing) {
    return versionsOf(listing).reduce((day, v) => (typeof v.published === "string" && v.published > day ? v.published : day), "");
  }

  function capabilities(listing) {
    const out = [];
    for (const version of versionsOf(listing)) {
      for (const c of Array.isArray(version.capabilities) ? version.capabilities : []) out.push(c);
    }
    return out;
  }

  // Everything free text matches, lower-cased.
  function haystack(listing) {
    const fields = [listing.id, listing.name, listing.description, listing.kind, KIND_NAMES[listing.kind], TIER_NAMES[listing.tier]];
    for (const list of [listing.keywords, listing.authors]) {
      if (Array.isArray(list)) fields.push(...list);
    }
    if (listing.crate && listing.crate.name) fields.push(listing.crate.name);
    for (const c of capabilities(listing)) fields.push(c.kind, c.name);
    return fields.filter((f) => typeof f === "string").join("\n").toLowerCase();
  }

  // Every word of `query` somewhere in the listing.
  function matches(listing, query) {
    const words = String(query || "").toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return true;
    const text = haystack(listing);
    return words.every((w) => text.includes(w));
  }

  // filters: {query, kind, tier, officialOnly, recommendedOnly}
  function filter(listings, filters) {
    const f = filters || {};
    return listings.filter(
      (l) =>
        (!f.kind || l.kind === f.kind) &&
        (!f.tier || l.tier === f.tier) &&
        (!f.officialOnly || l.tier === "official") &&
        (!f.recommendedOnly || isRecommended(l)) &&
        matches(l, f.query),
    );
  }

  function isRecommended(listing) {
    const r = listing.recommended;
    return !!(r && typeof r === "object" && typeof r.since === "string" && typeof r.note === "string");
  }

  // The lower bound of a 95% Wilson interval on the share of 👍: many votes
  // count for more than a few.
  function rating(stats) {
    const r = stats && stats.reviews;
    if (!r) return null;
    const n = r.up + r.down;
    if (!n) return null;
    const z = 1.96;
    const p = r.up / n;
    return (p + (z * z) / (2 * n) - z * Math.sqrt((p * (1 - p) + (z * z) / (4 * n)) / n)) / (1 + (z * z) / n);
  }

  function byName(a, b) {
    return String(a.name || a.id).localeCompare(String(b.name || b.id), "en", { sensitivity: "base" }) || (a.id < b.id ? -1 : 1);
  }

  function officialFirst(a, b) {
    return (TIER_RANK[a.tier] ?? 3) - (TIER_RANK[b.tier] ?? 3);
  }

  // by: "default" (official first, then by name), "name", "newest",
  // "downloads", "rating" or "recommended". `stats` maps id to its stats.
  function sort(listings, by, stats) {
    const s = stats || {};
    const nullLast = (x, y) => (x == null ? (y == null ? 0 : 1) : y == null ? -1 : y - x);
    const compare = {
      name: byName,
      newest: (a, b) => (latestDay(b) > latestDay(a) ? 1 : latestDay(b) < latestDay(a) ? -1 : 0) || byName(a, b),
      downloads: (a, b) => nullLast(s[a.id] ? s[a.id].total : null, s[b.id] ? s[b.id].total : null) || officialFirst(a, b) || byName(a, b),
      rating: (a, b) => nullLast(rating(s[a.id]), rating(s[b.id])) || officialFirst(a, b) || byName(a, b),
      recommended: (a, b) => Number(isRecommended(b)) - Number(isRecommended(a)) || officialFirst(a, b) || byName(a, b),
    }[by] || ((a, b) => officialFirst(a, b) || byName(a, b));
    return listings.slice().sort(compare);
  }

  // The command that gets a package: `rich plugin install ID[@VERSION]`, or
  // for a crate the `cargo add` line `rich plugin install` prints.
  function installCommand(listing, version) {
    if (listing.kind === "crate") {
      const v = version || newest(listing);
      const name = (listing.crate && listing.crate.name) || listing.id;
      let line = `cargo add ${name}@=${v ? v.version : ""}`;
      const features = listing.crate && Array.isArray(listing.crate.features) ? listing.crate.features : [];
      if (features.length) line += ` --features ${features.join(",")}`;
      return line;
    }
    return version ? `rich plugin install ${listing.id}@${version.version}` : `rich plugin install ${listing.id}`;
  }

  function requiresText(requires) {
    const r = requires || {};
    if (r.abi != null) return `plugin ABI ${r.abi}`;
    if (r.plugin_api != null) return `plugin API ${r.plugin_api}`;
    if (r.micro_schema != null) return `micro pack schema ${r.micro_schema}`;
    if (r.theme_format != null) return `theme format ${r.theme_format}`;
    return "unknown";
  }

  // A capability in plain words.
  function capabilityText(c) {
    const name = String(c.name);
    switch (c.kind) {
      case "transform": return `Text transform ${name} (rich --transform ${name})`;
      case "fence-markup": return `Draws \`\`\`${name} fences in Markdown`;
      case "fence-ansi": return `Draws \`\`\`${name} fences in Markdown, in colour`;
      case "fence-renderer": return `Draws \`\`\`${name} fences in Markdown`;
      case "highlighter": return `Highlighter ${name}`;
      case "code-highlighter": return `Code highlighter ${name}`;
      case "theme": return `Theme ${name}`;
      case "box-style": return `Box style ${name}`;
      case "renderer": return `Renderer ${name}`;
      case "action": return `Action ${name}`;
      case "component": return `Component ${name}`;
      case "asset": return `Micro asset ${name} (:micro:${name}:)`;
      default: return `${c.kind} ${name}`;
    }
  }

  const PERMISSIONS = {
    "ansi-output": "Its fences keep their colours (after rich's sanitizer)",
    "native-code": "Runs as native code with your rights, once you build it into a program",
    "filesystem-read": "Reads files",
    "filesystem-write": "Writes files",
    network: "Uses the network",
    process: "Starts other programs",
    environment: "Reads environment variables",
    clipboard: "Uses the clipboard",
  };

  // Permissions in plain words, as rich plugin info says them.
  function permissionsText(kind, permissions) {
    const list = Array.isArray(permissions) ? permissions : [];
    if (!list.length) {
      if (kind === "wasm-plugin") return ["None: sandboxed, with no files, network, clock or processes"];
      if (kind === "crate") return ["None declared"];
      return ["None: data only"];
    }
    const out = list.map((p) => PERMISSIONS[p] || String(p));
    if (kind === "crate") out.push("Declared, not enforced: native code runs with your rights");
    return out;
  }

  // `https://github.com/owner/repo.git` as `github.com/owner/repo`.
  function repositorySource(url) {
    const rest = String(url || "").replace(/^https:\/\//, "").replace(/\/+$/, "");
    return rest.replace(/\.git$/, "");
  }

  function repositoryOwner(url) {
    const parts = repositorySource(url).split("/");
    return parts[1] || parts[0] || "";
  }

  // "Authored by …" and whether to add the community note. An official
  // package is rs-rich's; any other names its own authors (in quotes when
  // they claim rs-rich), at most three and cut short, then its repository.
  function authoredBy(listing) {
    if (listing.tier === "official") return "Authored by rs-rich";
    const authors = Array.isArray(listing.authors) ? listing.authors.filter((a) => typeof a === "string" && a) : [];
    const names = authors.slice(0, MAX_AUTHORS).map((a) => {
      let short = Array.from(a).slice(0, MAX_AUTHOR_CHARS).join("");
      if (short.length < a.length) short += "…";
      return /rs-rich/i.test(short) ? `"${short}"` : short;
    });
    if (authors.length > MAX_AUTHORS) names.push(`and ${authors.length - MAX_AUTHORS} more`);
    if (!names.length) names.push(repositoryOwner(listing.repository));
    return `Authored by ${names.join(", ")} from ${repositorySource(listing.repository)}`;
  }

  function publisher(listing) {
    const owner = repositoryOwner(listing.repository);
    if (listing.tier === "official") return "Official: built and published by rs-rich";
    if (listing.tier === "reviewed") return `Approved: published by ${owner}, not by rs-rich. A maintainer read its source and rebuilt it.`;
    return `Community: published by ${owner}, not by rs-rich.`;
  }

  const COMMUNITY_NOTE = "Made by its author, not by rs-rich. Check its source and permissions.";

  // A count, or "—" where nothing was counted.
  function count(n) {
    return typeof n === "number" ? n.toLocaleString("en-US") : "—";
  }

  function isCount(n) {
    return Number.isSafeInteger(n) && n >= 0;
  }

  // stats.json, version 1, as {id: {total, versions, reviews}}; anything
  // else in it is dropped. A review link must be one of the marketplace's
  // own discussions: the stats branch is unprotected, so a link anywhere
  // else is dropped.
  function readStats(json) {
    if (!json || typeof json !== "object" || json.schema_version !== 1 || !json.packages || typeof json.packages !== "object") return null;
    const out = {};
    for (const [id, entry] of Object.entries(json.packages)) {
      if (!isValidId(id) || !entry || typeof entry !== "object") continue;
      const downloads = entry.downloads && typeof entry.downloads === "object" ? entry.downloads : {};
      const versions = {};
      if (downloads.versions && typeof downloads.versions === "object") {
        for (const [v, n] of Object.entries(downloads.versions)) versions[v] = isCount(n) ? n : null;
      }
      let reviews = null;
      const r = entry.reviews;
      if (r && typeof r === "object" && isCount(r.up) && isCount(r.down) && isCount(r.comments)) {
        let url = null;
        try {
          const parsed = new URL(r.url);
          // Checked after the URL is normalised, so `..` cannot leave it.
          if (String(r.url).startsWith(REVIEWS) && parsed.href.startsWith(REVIEWS) && parsed.search === "" && parsed.hash === "") url = parsed.href;
        } catch (e) {
          url = null;
        }
        reviews = { up: r.up, down: r.down, comments: r.comments, url };
      }
      out[id] = { total: isCount(downloads.total) ? downloads.total : null, versions, reviews };
    }
    return { generatedAt: typeof json.generated_at === "string" ? json.generated_at : null, packages: out };
  }

  function httpsUrl(text) {
    try {
      const url = new URL(text);
      return url.protocol === "https:" ? url.href : null;
    } catch (e) {
      return null;
    }
  }

  const Logic = {
    KIND_NAMES, TIER_NAMES, COMMUNITY_NOTE, isValidId, newest, latestDay, haystack, matches, filter, sort, rating,
    isRecommended, installCommand, requiresText, capabilityText, permissionsText, repositorySource,
    repositoryOwner, authoredBy, publisher, count, readStats, httpsUrl,
  };
  if (typeof module === "object" && module.exports) module.exports = Logic;
  if (typeof window === "undefined" || typeof document === "undefined") return;
  window.MarketplaceLogic = Logic;

  // -------------------------------------------------------------------------
  // The page.
  // -------------------------------------------------------------------------

  const FETCH = { credentials: "omit", referrerPolicy: "no-referrer", cache: "no-cache" };
  const MAX_LISTINGS = 500;
  // Our own mark: a shield with a check, built as elements (no HTML).
  const SVG = "http://www.w3.org/2000/svg";
  function shield() {
    const svg = document.createElementNS(SVG, "svg");
    for (const [k, v] of Object.entries({ viewBox: "0 0 16 16", width: "14", height: "14", "aria-hidden": "true", focusable: "false" })) svg.setAttribute(k, v);
    const path = document.createElementNS(SVG, "path");
    path.setAttribute("fill", "currentColor");
    path.setAttribute("d", "M8 0 1.5 2.5v5C1.5 11.6 4.3 14.9 8 16c3.7-1.1 6.5-4.4 6.5-8.5v-5L8 0Zm-1 11.4L3.8 8.2l1.1-1.1L7 9.2l4.1-4.1 1.1 1.1L7 11.4Z");
    svg.append(path);
    return svg;
  }

  // h("a", {href: …}, "text", child…): an element; strings become text.
  function h(tag, attrs, ...children) {
    const el = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) {
      if (v == null || v === false) continue;
      if (k === "class") el.className = v;
      else el.setAttribute(k, v === true ? "" : String(v));
    }
    for (const c of children.flat()) {
      if (c == null || c === false) continue;
      el.append(typeof c === "string" ? document.createTextNode(c) : c);
    }
    return el;
  }

  function link(href, text, label) {
    const url = Logic.httpsUrl(href);
    if (!url) return h("span", {}, text);
    return h("a", { href: url, rel: "noopener noreferrer", referrerpolicy: "no-referrer", "aria-label": label }, text);
  }

  // A file this site serves (a listing's JSON): a relative link.
  function siteLink(path, text, label) {
    return h("a", { href: path, type: "application/json", "aria-label": label }, text);
  }

  async function sha256(bytes) {
    if (!(window.crypto && crypto.subtle)) return null;
    const digest = await crypto.subtle.digest("SHA-256", bytes);
    return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
  }

  // The stats are optional and come from another host: give up on them
  // after this long, so a stalled request never holds the listings back.
  const STATS_TIMEOUT_MS = 8000;

  async function fetchJson(url, timeoutMs = 0) {
    const abort = timeoutMs > 0 ? new AbortController() : null;
    const timer = abort ? setTimeout(() => abort.abort(), timeoutMs) : 0;
    let response;
    try {
      response = await fetch(url, abort ? { ...FETCH, signal: abort.signal } : FETCH);
    } finally {
      clearTimeout(timer);
    }
    if (!response.ok) {
      const error = new Error(`${response.status} ${response.statusText}`);
      error.status = response.status;
      throw error;
    }
    return new Uint8Array(await response.arrayBuffer());
  }

  function parse(bytes) {
    return JSON.parse(new TextDecoder().decode(bytes));
  }

  // Load the index and its listings: {state: "ok", index, listings, skipped}
  // or {state: "unsigned" | "error", message}.
  async function load(base) {
    let index;
    try {
      index = parse(await fetchJson(new URL("index.json", base)));
    } catch (error) {
      if (error.status === 404) return { state: "unsigned" };
      return { state: "error", message: error instanceof SyntaxError ? "its index.json is not JSON" : `the marketplace could not be reached (${error.message})` };
    }
    if (!index || index.schema_version !== 1 || !index.listings || typeof index.listings !== "object") {
      return { state: "error", message: "its index.json is not one this page reads (schema version 1)" };
    }
    const entries = Object.entries(index.listings)
      .filter(([id, e]) => Logic.isValidId(id) && e && e.path === `packages/${id}.json` && /^[0-9a-f]{64}$/.test(e.sha256))
      .slice(0, MAX_LISTINGS);
    let skipped = Object.keys(index.listings).length - entries.length;
    const results = await Promise.allSettled(
      entries.map(async ([id, entry]) => {
        const bytes = await fetchJson(new URL(entry.path, base));
        const hash = await sha256(bytes);
        if (hash !== null && hash !== entry.sha256) throw new Error(`${id}: its SHA-256 does not match the index`);
        const listing = parse(bytes);
        if (!listing || listing.id !== id || listing.schema_version !== 1 || !Array.isArray(listing.versions)) throw new Error(`${id}: not a listing`);
        return listing;
      }),
    );
    const listings = [];
    for (const r of results) {
      if (r.status === "fulfilled") listings.push(r.value);
      else skipped += 1;
    }
    return { state: "ok", index, listings, skipped };
  }

  async function loadStats(url) {
    try {
      return Logic.readStats(parse(await fetchJson(url, STATS_TIMEOUT_MS)));
    } catch (e) {
      return null;
    }
  }

  function mount() {
    const app = document.getElementById("mkt-app");
    if (!app || app.dataset.mounted) return;
    app.dataset.mounted = "1";
    const base = new URL(app.dataset.base || "./", location.href).href;
    const tiersHref = Logic.httpsUrl(app.dataset.tiers) || "#mkt-reading";
    const ui = build(app, tiersHref);
    const start = () => {
      ui.setStatus("Loading the marketplace…", true);
      Promise.all([load(base), loadStats(app.dataset.stats)]).then(([data, stats]) => ui.show(data, stats));
    };
    ui.retry = start;
    start();
  }

  function build(app, tiersHref) {
    const state = { listings: [], stats: null, index: null };
    const ids = { search: "mkt-search", kind: "mkt-kind", tier: "mkt-tier", sort: "mkt-sort" };
    const search = h("input", { id: ids.search, type: "search", class: "mkt-input", placeholder: "rot13, theme, mermaid…", autocomplete: "off", spellcheck: "false", "aria-describedby": "mkt-count" });
    const select = (id, options) => h("select", { id, class: "mkt-input" }, options.map(([value, text]) => h("option", { value }, text)));
    const kind = select(ids.kind, [["", "All kinds"], ...Object.entries(KIND_NAMES)]);
    const tier = select(ids.tier, [["", "All tiers"], ["official", "Official"], ["reviewed", "Approved"], ["community", "Community"]]);
    const sortBy = select(ids.sort, [["default", "Official first"], ["name", "Name"], ["newest", "Newest"], ["downloads", "Most downloaded"], ["rating", "Best rated"], ["recommended", "Recommended first"]]);
    const official = h("input", { type: "checkbox", id: "mkt-official", class: "mkt-check" });
    const recommended = h("input", { type: "checkbox", id: "mkt-recommended", class: "mkt-check" });
    const count = h("p", { id: "mkt-count", class: "mkt-count", role: "status", "aria-live": "polite" });
    const meta = h("p", { class: "mkt-meta" });
    const list = h("ul", { class: "mkt-list", "aria-label": "Packages" });
    const notice = h("div", { class: "mkt-notice", hidden: true });
    const controls = h(
      "form",
      { class: "mkt-controls", role: "search", "aria-label": "Search the marketplace" },
      h("div", { class: "mkt-field mkt-field--search" }, h("label", { for: ids.search }, "Search packages"), search),
      h("div", { class: "mkt-field" }, h("label", { for: ids.kind }, "Kind"), kind),
      h("div", { class: "mkt-field" }, h("label", { for: ids.tier }, "Tier"), tier),
      h("div", { class: "mkt-field" }, h("label", { for: ids.sort }, "Sort by"), sortBy),
      h("div", { class: "mkt-toggles" },
        h("label", { class: "mkt-toggle", for: "mkt-official" }, official, " Official only"),
        h("label", { class: "mkt-toggle", for: "mkt-recommended" }, recommended, " Recommended only")),
    );
    controls.addEventListener("submit", (e) => e.preventDefault());
    app.replaceChildren(controls, count, notice, list, meta);

    const filters = () => ({ query: search.value, kind: kind.value, tier: tier.value, officialOnly: official.checked, recommendedOnly: recommended.checked });
    const statsOf = () => (state.stats ? state.stats.packages : {});

    function render() {
      const all = state.listings;
      const shown = Logic.sort(Logic.filter(all, filters()), sortBy.value, statsOf());
      list.replaceChildren(...shown.map((l) => card(l, statsOf()[l.id] || null, tiersHref, ui)));
      count.textContent = shown.length === all.length ? `${all.length} packages` : `${shown.length} of ${all.length} packages match`;
      if (!shown.length && all.length) {
        list.replaceChildren(h("li", { class: "mkt-empty" }, "Nothing matches. Try fewer words, or clear the filters."));
      }
    }

    for (const control of [search, kind, tier, sortBy, official, recommended]) {
      control.addEventListener(control === search ? "input" : "change", render);
    }
    search.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && search.value) {
        search.value = "";
        render();
      } else if (e.key === "ArrowDown" || e.key === "Enter") {
        const first = list.querySelector(".mkt-title a");
        if (first) {
          e.preventDefault();
          first.focus();
        }
      }
    });
    // Up and down move between packages; Home and End to the first and last.
    list.addEventListener("keydown", (e) => {
      if (!e.target.matches(".mkt-title a")) return;
      const titles = Array.from(list.querySelectorAll(".mkt-title a"));
      const at = titles.indexOf(e.target);
      const to = { ArrowDown: at + 1, ArrowUp: at - 1, Home: 0, End: titles.length - 1 }[e.key];
      if (to === undefined) return;
      e.preventDefault();
      if (to < 0) search.focus();
      else if (titles[to]) titles[to].focus();
    });

    const ui = {
      retry: null,
      setStatus(text, busy) {
        count.textContent = text;
        app.setAttribute("aria-busy", busy ? "true" : "false");
      },
      announce(text) {
        count.textContent = text;
      },
      show(data, stats) {
        app.setAttribute("aria-busy", "false");
        notice.hidden = true;
        notice.replaceChildren();
        if (data.state === "unsigned") {
          controls.hidden = true;
          count.textContent = "";
          notice.hidden = false;
          notice.className = "mkt-notice mkt-notice--info";
          notice.append(
            h("p", { class: "mkt-notice-title" }, "The marketplace's first index is being signed."),
            h("p", {}, "The listings are ready and a maintainer is signing the first index. Until then nothing can be installed, and this page has nothing to list. Come back soon, or read the listings in ",
              link("https://github.com/buchochelliq-labs/rs-rich-marketplace/tree/main/packages", "the marketplace repository"), "."),
          );
          return;
        }
        if (data.state === "error") {
          controls.hidden = true;
          count.textContent = "";
          notice.hidden = false;
          notice.className = "mkt-notice mkt-notice--error";
          const retry = h("button", { type: "button", class: "mkt-button" }, "Try again");
          retry.addEventListener("click", () => {
            controls.hidden = false;
            notice.hidden = true;
            if (ui.retry) ui.retry();
          });
          notice.append(
            h("p", { class: "mkt-notice-title" }, `Could not read the marketplace: ${data.message}.`),
            h("p", {}, h("code", {}, "rich plugin search"), " lists it in a terminal, or read the listings in ",
              link("https://github.com/buchochelliq-labs/rs-rich-marketplace/tree/main/packages", "the marketplace repository"), "."),
            retry,
          );
          return;
        }
        controls.hidden = false;
        state.listings = data.listings;
        state.index = data.index;
        state.stats = stats;
        const parts = [`Index serial ${data.index.serial}, signed ${String(data.index.generated || "").slice(0, 10)}, expires ${String(data.index.expires || "").slice(0, 10)}.`];
        if (typeof data.index.expires === "string" && Date.parse(data.index.expires) < Date.now()) {
          notice.hidden = false;
          notice.className = "mkt-notice mkt-notice--warning";
          notice.append(h("p", {}, `This index expired on ${data.index.expires.slice(0, 10)}. rich installs nothing from it until the marketplace signs a new one.`));
        }
        if (data.skipped) parts.push(`${data.skipped} listing${data.skipped === 1 ? " was" : "s were"} left out: not as the index says.`);
        parts.push(stats && stats.generatedAt ? `Download counts and ratings from ${stats.generatedAt.slice(0, 10)}, refreshed nightly, not signed.` : "Download counts and ratings are not available right now.");
        meta.textContent = parts.join(" ");
        render();
        reveal(false);
      },
    };

    // Show the package the fragment names: clear what hides it, open it,
    // and move focus to it.
    function reveal(fromHashChange) {
      const id = decodeURIComponent(location.hash.slice(1));
      if (!Logic.isValidId(id)) return;
      const listing = state.listings.find((l) => l.id === id);
      if (!listing) return;
      if (!Logic.filter([listing], filters()).length) {
        search.value = "";
        kind.value = "";
        tier.value = "";
        official.checked = false;
        recommended.checked = false;
        render();
      }
      const el = document.getElementById(id);
      if (!el) return;
      const details = el.querySelector("details");
      if (details) details.open = true;
      for (const other of list.querySelectorAll(".mkt-card--target")) other.classList.remove("mkt-card--target");
      el.classList.add("mkt-card--target");
      const still = window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches;
      el.scrollIntoView({ block: "start", behavior: fromHashChange && !still ? "smooth" : "auto" });
      el.focus({ preventScroll: true });
    }
    ui.reveal = reveal;
    hashHandler = () => reveal(true);
    return ui;
  }

  let hashHandler = null;
  // The packages whose details are open, kept as the list is redrawn.
  const openDetails = new Set();
  window.addEventListener("hashchange", () => {
    if (hashHandler && document.getElementById("mkt-app")) hashHandler();
  });

  async function copy(text) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch (e) {
      const area = h("textarea", { class: "mkt-offscreen", "aria-hidden": "true" });
      area.value = text;
      document.body.append(area);
      area.select();
      let ok = false;
      try {
        ok = document.execCommand("copy");
      } catch (err) {
        ok = false;
      }
      area.remove();
      return ok;
    }
  }

  function command(text, label, ui) {
    const button = h("button", { type: "button", class: "mkt-copy", "aria-label": `Copy: ${text}`, title: "Copy" }, "Copy");
    button.addEventListener("click", async () => {
      const ok = await copy(text);
      button.textContent = ok ? "Copied" : "Copy failed";
      ui.announce(ok ? `Copied: ${text}` : "Copying failed; select the command instead.");
      setTimeout(() => (button.textContent = "Copy"), 2000);
    });
    return h("div", { class: "mkt-command", "aria-label": label, role: "group" }, h("code", {}, text), button);
  }

  function tierBadge(listing, tiersHref) {
    const tier = listing.tier;
    if (tier === "official") {
      const badge = h("a", { class: "mkt-badge mkt-badge--official", href: tiersHref, title: "Official: built and published by rs-rich. What the tiers mean.", "aria-label": "Official, by rs-rich: built and published by rs-rich. What the tiers mean" });
      badge.append(shield(), h("span", {}, "Official · by rs-rich"));
      return badge;
    }
    const name = TIER_NAMES[tier] || "Community";
    const meaning = tier === "reviewed" ? "a maintainer read its source and rebuilt the same file" : "checked by the marketplace's CI only, not reviewed";
    return h("a", { class: `mkt-badge mkt-badge--${tier === "reviewed" ? "approved" : "community"}`, href: tiersHref, title: `${name}: ${meaning}. What the tiers mean.`, "aria-label": `${name}: ${meaning}. What the tiers mean` }, name);
  }

  function recommendedBadge(listing) {
    const r = listing.recommended;
    return h("span", { class: "mkt-badge mkt-badge--recommended", title: `Recommended since ${r.since}: ${r.note}`, "aria-label": `Recommended by the maintainers: ${r.note}` }, h("span", { "aria-hidden": "true" }, "★ "), "Recommended");
  }

  function statsLine(listing, stats) {
    const s = stats || { total: null, reviews: null };
    const items = [h("span", { class: "mkt-stat" }, h("span", { class: "mkt-stat-n" }, Logic.count(s.total)), s.total === 1 ? " download" : " downloads")];
    if (s.reviews) {
      items.push(
        h("span", { class: "mkt-stat", "aria-label": `${s.reviews.up} thumbs up` }, h("span", { "aria-hidden": "true" }, "👍 "), Logic.count(s.reviews.up)),
        h("span", { class: "mkt-stat", "aria-label": `${s.reviews.down} thumbs down` }, h("span", { "aria-hidden": "true" }, "👎 "), Logic.count(s.reviews.down)),
        h("span", { class: "mkt-stat" }, Logic.count(s.reviews.comments), s.reviews.comments === 1 ? " review" : " reviews"),
      );
      if (s.reviews.url) items.push(link(s.reviews.url, "Read or write reviews on GitHub", `Read or write reviews of ${listing.id} on GitHub`));
    } else {
      items.push(h("span", { class: "mkt-stat mkt-muted" }, "No reviews yet"));
    }
    return h("p", { class: "mkt-stats" }, items);
  }

  function card(listing, stats, tiersHref, ui) {
    const id = listing.id;
    const newest = Logic.newest(listing);
    const titleId = `mkt-title-${id}`;
    const badges = h("div", { class: "mkt-badges" }, tierBadge(listing, tiersHref), Logic.isRecommended(listing) ? recommendedBadge(listing) : null,
      h("span", { class: "mkt-kind" }, KIND_NAMES[listing.kind] || String(listing.kind)));
    const facts = h("p", { class: "mkt-facts" },
      newest ? `${newest.version}` : "no versions",
      newest && newest.published ? ` · published ${newest.published}` : "",
      newest && newest.yanked ? " · yanked" : "");
    const author = h("p", { class: "mkt-author" }, Logic.authoredBy(listing));
    const children = [
      h("div", { class: "mkt-head" },
        h("h3", { class: "mkt-title", id: titleId }, h("a", { href: `#${id}` }, String(listing.name || id)), " ", h("code", { class: "mkt-id" }, id)),
        badges),
      author,
    ];
    if (listing.tier === "community") children.push(h("p", { class: "mkt-community-note" }, COMMUNITY_NOTE));
    children.push(h("p", { class: "mkt-description" }, String(listing.description || "")), facts, statsLine(listing, stats));
    const installable = listing.kind !== "crate";
    const usable = newest && !newest.yanked;
    if (usable) children.push(command(Logic.installCommand(listing), installable ? `Install ${id}` : `Add ${id} to your program`, ui));
    const repoUrl = Logic.httpsUrl(listing.repository);
    children.push(h("p", { class: "mkt-links" },
      repoUrl ? link(repoUrl, "Source", `Source repository of ${id}: ${Logic.repositorySource(listing.repository)}`) : null,
      siteLink(`packages/${id}.json`, "Listing JSON", `The listing of ${id}, as JSON`)));
    children.push(details(listing, stats, ui));
    return h("li", {}, h("article", { class: `mkt-card mkt-card--${listing.tier === "official" ? "official" : "other"}`, id, tabindex: "-1", "aria-labelledby": titleId }, children));
  }

  function dl(rows) {
    return h("dl", { class: "mkt-dl" }, rows.filter(Boolean).flatMap(([k, v]) => [h("dt", {}, k), h("dd", {}, v)]));
  }

  function details(listing, stats, ui) {
    const newest = Logic.newest(listing);
    const list = (items) => h("ul", {}, items.map((t) => h("li", {}, t)));
    const repoUrl = Logic.httpsUrl(listing.repository);
    const rows = [
      ["Published by", Logic.publisher(listing)],
      ["Authored by", Logic.authoredBy(listing).replace(/^Authored by /, "")],
      Logic.isRecommended(listing) ? ["Recommended", `★ Since ${listing.recommended.since}: ${listing.recommended.note}`] : null,
      ["Licence", String(listing.license || "")],
      ["Source", repoUrl ? link(repoUrl, Logic.repositorySource(listing.repository)) : "—"],
      ["Listing", siteLink(`packages/${listing.id}.json`, `packages/${listing.id}.json`, `The listing of ${listing.id}, as JSON`)],
      listing.homepage && Logic.httpsUrl(listing.homepage) ? ["Homepage", link(listing.homepage, Logic.repositorySource(listing.homepage))] : null,
      listing.crate ? ["Crate", `${listing.crate.name}${Array.isArray(listing.crate.features) && listing.crate.features.length ? `, features ${listing.crate.features.join(", ")}` : ""}`] : null,
      ["What it adds", newest ? list((newest.capabilities || []).map(Logic.capabilityText)) : "—"],
      ["Permissions", list(Logic.permissionsText(listing.kind, newest ? newest.permissions : []))],
    ];
    const versions = Array.isArray(listing.versions) ? listing.versions : [];
    const counts = (stats && stats.versions) || {};
    const table = h("table", { class: "mkt-versions" },
      h("caption", {}, `Every version of ${listing.id}, newest first`),
      h("thead", {}, h("tr", {}, ["Version", "Published", "Requires", "Downloads", "Status", "Command"].map((c) => h("th", { scope: "col" }, c)))),
      h("tbody", {}, versions.map((v) => h("tr", { class: v.yanked ? "mkt-yanked" : null },
        h("th", { scope: "row" }, String(v.version)),
        h("td", {}, String(v.published || "")),
        h("td", {}, Logic.requiresText(v.requires)),
        h("td", { class: "mkt-num" }, Logic.count(counts[v.version])),
        h("td", { class: "mkt-status" }, v.yanked ? `Yanked: ${String(v.yank_reason || "")}` : "Listed"),
        h("td", {}, v.yanked ? "—" : command(Logic.installCommand(listing, v), `Get ${listing.id} ${v.version}`, ui))))));
    const el = h("details", { class: "mkt-details", open: openDetails.has(listing.id) }, h("summary", {}, "Details and every version"), dl(rows), h("div", { class: "mkt-table-wrap" }, table));
    el.addEventListener("toggle", () => (el.open ? openDetails.add(listing.id) : openDetails.delete(listing.id)));
    return el;
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", mount);
  } else {
    mount();
  }
})();
