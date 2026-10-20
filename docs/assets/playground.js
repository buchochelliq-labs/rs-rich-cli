// The docs playground, tier 1 (docs/design/docs-playground.md): every
// `.rich-playground` block gets a "Try it" button that turns its input into
// an editor and renders it with rs-rich, compiled to wasm, in this tab.
//
// Nothing loads with the page but this file and, when a block is on it, the
// `playground/playground.json` stamp beside it. No stamp (a docs version
// built without the module), no WebAssembly, or no blocks: the blocks stay
// plain code blocks. The module loads on the first click: the lean one
// (no syntect) for most kinds, the full one only for `syntax` and
// `markdown`. Output is an HTML document from `Console::export_html` in a
// sandboxed `srcdoc` iframe; xterm.js loads only when the reader switches a
// block to the terminal view.
//
// A block's state can be shared as a link: the kind, input and options,
// deflated into the URL fragment (`#rp=<block>.<data>`), capped in size,
// and restored as text only (the input goes into a textarea's value, and
// is only ever rendered by the module).
(() => {
  const script = document.currentScript && document.currentScript.src;
  if (!script || typeof WebAssembly !== "object") return;
  const BASE = new URL("playground/", script);
  // The longest input a block takes, and the longest fragment a link carries.
  const MAX_INPUT = 20000;
  const MAX_FRAGMENT = 8000;
  // Options are the block's data-* attributes; these are the page's own.
  const RESERVED = new Set(["kind", "kinds", "mounted"]);
  const NUMBERS = new Set(["width", "padding", "indent", "bar-width", "bins", "threshold", "context", "tab-size"]);
  // The options a shared link may set, and the controls offer.
  const CONTROLS = {
    box: { kinds: ["table", "panel"], values: ["", "ROUNDED", "SQUARE", "HEAVY", "HEAVY_HEAD", "DOUBLE", "MINIMAL", "SIMPLE", "ASCII", "MARKDOWN"] },
    orientation: { kinds: ["bars", "histogram"], values: ["", "horizontal", "vertical"] },
    layout: { kinds: ["diff"], values: ["", "unified", "side-by-side"] },
    view: { kinds: ["data"], values: ["", "tree", "table", "flat"] },
    justify: { kinds: ["panel", "markdown"], values: ["", "left", "center", "right", "full"] },
  };
  const SHARED_OPTIONS = new Set(["width", ...Object.keys(CONTROLS)]);
  // Inputs for the playground page's kind picker.
  const SAMPLES = {
    markup: "[bold magenta]Hello[/], [u]world[/]! :rocket:\n[#ff8800]#ff8800[/] [white on dark_blue] white on dark_blue [/]\n[link=https://github.com/Textualize/rich]a hyperlink[/link]",
    panel: "Panels take [b]any[/b] renderable,\nincluding [i]styled[/i] text.",
    rule: "[b]Centred title[/b]\n\nAnother rule",
    columns: "rs-rich\nrs-rich-ext\nrs-rich-cli\nrs-rich-art\nsyntect\nserde_json\npulldown-cmark\nfancy-regex",
    table: "Released,Title,Box Office\n\"Dec 20, 2019\",The Rise of Skywalker,\"$952,110,690\"\n\"May 25, 2018\",Solo,\"$393,151,347\"\n\"Dec 15, 2017\",[i]The Last Jedi[/],\"$1,332,539,889\"",
    tree: "rs-rich-cli\n  crates\n    rich\n    rich-ext\n      src\n      examples\n    rich-cli\n  docs\n    guide\n  [bold]Cargo.toml[/]",
    json: '{"name": "rs-rich", "version": "0.0.21", "tags": ["terminal", "ansi"],\n "stable": false, "downloads": 1204, "license": null}',
    sparkline: "12 15 14 18 25 31 28 22 35 41 38 30 26 33 47 52 44 39 36 29 24 21 17 14",
    bars: "api,412\nweb,268.5\nworker,97\ncron,12",
    histogram: "21 25 33 38 40 41 44 47 48 52 53 55 57 58 61 64 66 70 72 77 85 140",
    diff: "[server]\nport = 80\nhost = \"0.0.0.0\"\nworkers = 4\n=======\n[server]\nport = 8080\nhost = \"0.0.0.0\"\nworkers = 8\ntls = true\n",
    data: "defaults: &defaults\n  replicas: 2\n  image: registry.example/app:1.4\nservices:\n  web:\n    port: 8080\n    settings: *defaults\n  worker: *defaults\n",
    "data-diff": '{"name": "api", "ports": [80, 443], "tls": true}\n=======\n{"name": "api", "ports": [80, 8443], "tls": true, "hsts": true}\n',
    syntax: 'fn main() {\n    let answer = 6 * 7;\n    println!("The answer is {answer}");\n}\n',
    markdown: "# Release notes\n\nMarkdown renders **bold**, *italic*, `code` and [links](https://docs.rs/rs-rich).\n\n- bullet lists\n  - nested\n\n> Block quotes, too.\n",
  };

  let stampRequest = null;
  let generation = 0;
  const loaded = {};
  let xtermRequest = null;

  function stamp() {
    if (!stampRequest) {
      stampRequest = fetch(new URL("playground.json", BASE), { cache: "no-cache" })
        .then((response) => (response.ok ? response.json() : null))
        .then((s) => (s && s.format === 1 && s.modules && s.modules.lean ? s : null))
        .catch(() => null);
    }
    return stampRequest;
  }

  function moduleName(s, kind) {
    for (const name of ["lean", "full"]) {
      if (s.modules[name] && s.modules[name].kinds.includes(kind)) return name;
    }
    return null;
  }

  // The module's glue, initialised. After a panic (the module aborts and is
  // unusable) `generation` moves on, and the next call imports it afresh.
  function load(s, name) {
    const key = `${name}:${generation}`;
    if (!loaded[key]) {
      const entry = s.modules[name];
      const js = new URL(entry.js, BASE);
      if (generation) js.search = `g=${generation}`;
      loaded[key] = import(js.href).then(async (glue) => {
        await glue.default({ module_or_path: new URL(entry.wasm, BASE) });
        return glue;
      });
      loaded[key].catch(() => delete loaded[key]);
    }
    return loaded[key];
  }

  function loadXterm(s) {
    if (!xtermRequest) {
      xtermRequest = new Promise((resolve, reject) => {
        const css = document.createElement("link");
        css.rel = "stylesheet";
        css.href = new URL(s.xterm.css, BASE).href;
        document.head.append(css);
        const js = document.createElement("script");
        js.src = new URL(s.xterm.js, BASE).href;
        js.onload = () => resolve(window.Terminal);
        js.onerror = () => {
          xtermRequest = null;
          reject(new Error("xterm.js did not load"));
        };
        document.head.append(js);
      });
    }
    return xtermRequest;
  }

  // data-* attributes, as the module's options (kebab-case keys).
  function blockOptions(block) {
    const options = {};
    for (const [key, value] of Object.entries(block.dataset)) {
      const name = key.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
      if (RESERVED.has(name)) continue;
      if (NUMBERS.has(name)) options[name] = Number(value);
      else if (value === "true" || value === "false") options[name] = value === "true";
      else options[name] = value;
    }
    return options;
  }

  function blockInput(block) {
    const code = block.querySelector("code");
    return (code ? code.textContent : block.textContent).replace(/\n$/, "");
  }

  function el(tag, props = {}, ...children) {
    const node = document.createElement(tag);
    for (const [key, value] of Object.entries(props)) {
      if (key === "class") node.className = value;
      else if (key.startsWith("aria-") || key === "role" || key === "for") node.setAttribute(key, value);
      else node[key] = value;
    }
    node.append(...children);
    return node;
  }

  function lineCount(html) {
    const start = html.indexOf("<code");
    const end = html.lastIndexOf("</code>");
    const code = start >= 0 && end > start ? html.slice(start, end) : html;
    return code.replace(/\n$/, "").split("\n").length;
  }

  // ---- shareable links ------------------------------------------------

  const canShare = typeof CompressionStream === "function" && typeof DecompressionStream === "function";

  function toBase64Url(bytes) {
    let binary = "";
    for (const byte of bytes) binary += String.fromCharCode(byte);
    return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  }

  function fromBase64Url(text) {
    if (!/^[A-Za-z0-9_-]*$/.test(text)) throw new Error("not base64url");
    const binary = atob(text.replace(/-/g, "+").replace(/_/g, "/"));
    return Uint8Array.from(binary, (c) => c.charCodeAt(0));
  }

  async function pack(state) {
    const json = new TextEncoder().encode(JSON.stringify(state));
    const stream = new Blob([json]).stream().pipeThrough(new CompressionStream("deflate-raw"));
    return toBase64Url(new Uint8Array(await new Response(stream).arrayBuffer()));
  }

  // Inflate at most `limit` bytes: a small fragment must not become a huge
  // string.
  async function unpack(text, limit) {
    if (text.length > MAX_FRAGMENT) throw new Error("the link is too long");
    const stream = new Blob([fromBase64Url(text)]).stream().pipeThrough(new DecompressionStream("deflate-raw"));
    const reader = stream.getReader();
    const chunks = [];
    let size = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.length;
      if (size > limit) {
        reader.cancel();
        throw new Error("the link's input is too long");
      }
      chunks.push(value);
    }
    return JSON.parse(new TextDecoder().decode(await new Blob(chunks).arrayBuffer()));
  }

  // A shared state, checked field by field: only strings, numbers and
  // booleans, only known option names, within the input cap.
  function validState(state, s) {
    if (!state || typeof state !== "object" || state.v !== 1) return null;
    if (typeof state.k !== "string" || !moduleName(s, state.k)) return null;
    if (typeof state.i !== "string" || state.i.length > MAX_INPUT) return null;
    const options = {};
    for (const [key, value] of Object.entries(state.o && typeof state.o === "object" ? state.o : {})) {
      if (!SHARED_OPTIONS.has(key)) continue;
      if (key === "width" && Number.isFinite(value)) options.width = Math.max(20, Math.min(200, Math.round(value)));
      else if (typeof value === "string" && CONTROLS[key] && CONTROLS[key].values.includes(value)) options[key] = value;
    }
    return { k: state.k, i: state.i, o: options };
  }

  // ---- a block's editor ------------------------------------------------

  function controlsFor(kind, options, onChange) {
    const parts = [];
    const width = el("input", {
      type: "number", min: 20, max: 200, value: options.width || 80, class: "rp-width",
      "aria-label": "Width in cells",
    });
    width.addEventListener("input", () => {
      const value = Number(width.value);
      if (value >= 20 && value <= 200) onChange("width", value);
    });
    parts.push(el("label", { class: "rp-control" }, "width ", width));
    for (const [name, control] of Object.entries(CONTROLS)) {
      if (!control.kinds.includes(kind)) continue;
      const select = el("select", { "aria-label": name });
      for (const value of control.values) {
        select.append(el("option", { value, selected: value === (options[name] || "") }, value || "(as written)"));
      }
      select.addEventListener("change", () => onChange(name, select.value));
      parts.push(el("label", { class: "rp-control" }, `${name} `, select));
    }
    return parts;
  }

  async function open(block, index, shared) {
    const s = await stamp();
    if (!s) return;
    const pair = block.closest(".rich-playground-pair");
    const original = { kind: block.dataset.kind, input: blockInput(block), options: blockOptions(block) };
    const picker = block.dataset.kinds === "all";
    let kind = shared ? shared.k : original.kind;
    let options = { ...original.options, ...(shared ? shared.o : {}) };
    if (!options.width) options.width = 80;
    let terminal = null;
    let timer = 0;

    const status = el("span", { class: "rp-status", role: "status" });
    const error = el("p", { class: "rp-error", role: "alert", hidden: true });
    const input = el("textarea", {
      class: "rp-input", spellcheck: false, maxLength: MAX_INPUT,
      value: shared ? shared.i : original.input, "aria-label": "Input",
    });
    input.rows = Math.min(24, Math.max(4, input.value.split("\n").length + 1));
    const frame = el("iframe", { class: "rp-frame", title: "Rendered output", referrerPolicy: "no-referrer" });
    frame.setAttribute("sandbox", "");
    const screen = el("div", { class: "rp-terminal", hidden: true });
    const output = el("div", { class: "rp-output" }, frame, screen, error);
    const controls = el("span", { class: "rp-controls" });
    const view = el("button", { type: "button", class: "md-button rp-small", textContent: "Terminal view" });
    const share = el("button", { type: "button", class: "md-button rp-small", textContent: "Copy link", hidden: !canShare });
    const reset = el("button", { type: "button", class: "md-button rp-small", textContent: "Reset" });
    const toolbar = el("div", { class: "rp-toolbar" }, controls, view, share, reset, status);
    const editor = el("div", { class: "rp-editor" }, toolbar, input);

    function setOption(name, value) {
      if (value === "" || value === undefined) delete options[name];
      else options[name] = value;
      schedule();
    }

    function buildControls() {
      const parts = controlsFor(kind, options, setOption);
      if (picker) {
        const select = el("select", { "aria-label": "Kind" });
        for (const name of [...s.modules.lean.kinds, ...(s.modules.full ? s.modules.full.kinds : [])]) {
          if (!select.querySelector(`option[value="${name}"]`)) {
            select.append(el("option", { value: name, selected: name === kind }, name));
          }
        }
        select.addEventListener("change", () => {
          kind = select.value;
          input.value = SAMPLES[kind] || "";
          options = { width: options.width };
          buildControls();
          schedule();
        });
        parts.unshift(el("label", { class: "rp-control" }, "kind ", select));
      }
      controls.replaceChildren(...parts);
    }

    async function draw() {
      const name = moduleName(s, kind);
      if (!name) return;
      const entry = s.modules[name];
      const first = !loaded[`${name}:${generation}`];
      if (first) status.textContent = `Loading rs-rich (${Math.round(entry.bytes / 1024)} KB)…`;
      let glue;
      try {
        glue = await load(s, name);
      } catch (e) {
        status.textContent = "The playground could not load here.";
        return;
      }
      status.textContent = "";
      const text = input.value.slice(0, MAX_INPUT);
      const json = JSON.stringify(options);
      try {
        if (terminal) {
          const ansi = glue.render(kind, text, json);
          const rows = ansi.replace(/\n$/, "").split("\n").length;
          terminal.reset();
          terminal.resize(options.width, Math.max(2, rows));
          terminal.write(ansi.replace(/\n/g, "\r\n"));
        } else {
          const html = glue.render_html(kind, text, json);
          frame.srcdoc = html;
          frame.style.height = `${lineCount(html) * glue.html_line_height() + 20}px`;
        }
        error.hidden = true;
      } catch (e) {
        if (e instanceof WebAssembly.RuntimeError) {
          // A panic aborts the module: load a fresh one next time.
          generation += 1;
          error.textContent = "rs-rich stopped on this input (a panic); edit it to try again.";
        } else {
          error.textContent = String(e && e.message ? e.message : e);
        }
        error.hidden = false;
      }
    }

    function schedule() {
      clearTimeout(timer);
      timer = setTimeout(draw, 120);
    }

    view.addEventListener("click", async () => {
      if (terminal) {
        terminal.dispose();
        terminal = null;
        screen.hidden = true;
        frame.hidden = false;
        view.textContent = "Terminal view";
      } else {
        try {
          const Terminal = await loadXterm(s);
          terminal = new Terminal({
            cols: options.width, rows: 4, convertEol: false, disableStdin: true, cursorBlink: false,
            fontFamily: '"Fira Code", "DejaVu Sans Mono", Menlo, monospace', fontSize: 13,
          });
          screen.hidden = false;
          frame.hidden = true;
          terminal.open(screen);
          view.textContent = "HTML view";
        } catch (e) {
          status.textContent = "The terminal view could not load.";
          return;
        }
      }
      draw();
    });

    share.addEventListener("click", async () => {
      try {
        const data = await pack({ v: 1, k: kind, i: input.value, o: sharedOptions() });
        if (data.length > MAX_FRAGMENT) {
          status.textContent = "Too long to share as a link.";
          return;
        }
        const url = new URL(location.href);
        url.hash = `rp=${index}.${data}`;
        history.replaceState(history.state, "", url.href);
        await navigator.clipboard.writeText(url.href);
        status.textContent = "Link copied.";
      } catch (e) {
        status.textContent = "The link is in the address bar.";
      }
    });

    function sharedOptions() {
      const out = {};
      for (const key of SHARED_OPTIONS) {
        if (options[key] !== undefined && options[key] !== original.options[key]) out[key] = options[key];
      }
      return out;
    }

    reset.addEventListener("click", () => {
      kind = original.kind;
      input.value = original.input;
      options = { ...original.options, width: original.options.width || 80 };
      buildControls();
      schedule();
    });

    input.addEventListener("input", schedule);
    buildControls();
    block.hidden = true;
    block.after(editor);
    (pair || editor).after(output);
    block.dataset.open = "1";
    await draw();
    return editor;
  }

  // ---- mounting --------------------------------------------------------

  async function mount() {
    const blocks = [...document.querySelectorAll(".rich-playground:not([data-mounted])")];
    if (!blocks.length) return;
    const s = await stamp();
    if (!s) return;
    const all = [...document.querySelectorAll(".rich-playground")];
    for (const block of blocks) {
      block.dataset.mounted = "1";
      if (!moduleName(s, block.dataset.kind)) continue;
      const index = all.indexOf(block);
      const button = el("button", { type: "button", class: "md-button rp-try", textContent: "Try it" });
      button.addEventListener("click", async () => {
        button.remove();
        const editor = await open(block, index, null);
        if (editor) editor.querySelector("textarea").focus();
      });
      (block.closest(".rich-playground-pair") || block).after(button);
    }
    await restore(all, s);
  }

  async function restore(all, s) {
    const match = /^#rp=(\d+)\.([A-Za-z0-9_-]+)$/.exec(location.hash);
    if (!match || !canShare) return;
    const block = all[Number(match[1])];
    if (!block || block.dataset.open) return;
    let state = null;
    try {
      state = validState(await unpack(match[2], MAX_INPUT * 4 + 2000), s);
    } catch (e) {
      state = null;
    }
    if (!state) return;
    // A guide block keeps its own kind; only the playground page's picker
    // takes another.
    if (block.dataset.kinds !== "all") state.k = block.dataset.kind;
    const button = (block.closest(".rich-playground-pair") || block).nextElementSibling;
    if (button && button.classList.contains("rp-try")) button.remove();
    const editor = await open(block, Number(match[1]), state);
    if (editor) editor.scrollIntoView({ block: "center" });
  }

  if (window.document$) {
    document$.subscribe(() => mount());
  } else if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", () => mount());
  } else {
    mount();
  }
})();
