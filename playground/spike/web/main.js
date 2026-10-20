// The spike page's glue: wasm-bindgen's generated module, and xterm.js
// terminals that show what the wasm functions return.
import init, * as rich from "./pkg/rich_playground_spike.js";

const status = document.getElementById("status");
const font = 'ui-monospace, "DejaVu Sans Mono", Menlo, Consolas, monospace';

function terminal(id, cols, rows, options) {
  const term = new Terminal(Object.assign({
    cols, rows, fontFamily: font, fontSize: 13, convertEol: true,
    theme: { background: "#101010" }, scrollback: 0, disableStdin: true,
  }, options || {}));
  term.open(document.getElementById(id));
  return term;
}

// Replace what a terminal shows with `ansi`.
function show(term, ansi) {
  term.reset();
  term.write(ansi);
}

const t0 = performance.now();
await init();
status.textContent = `Ready in ${Math.round(performance.now() - t0)} ms. ${rich.versions()}.`;

const markupTerm = terminal("t-markup", 56, 8);
const markup = document.getElementById("markup");
const renderMarkup = () => show(markupTerm, rich.render_markup(markup.value, markupTerm.cols));
markup.addEventListener("input", renderMarkup);
renderMarkup();

const tableTerm = terminal("t-table", 56, 10);
const csv = document.getElementById("csv");
const frame = document.getElementById("html");
const renderTable = () => {
  show(tableTerm, rich.render_table(csv.value, "Star Wars Movies", tableTerm.cols));
  frame.srcdoc = rich.table_html(csv.value, "Star Wars Movies", 60);
};
csv.addEventListener("input", renderTable);
renderTable();

// Tier 3, when the build has it.
const start = document.getElementById("start");
if (typeof rich.app_start !== "function") {
  start.disabled = true;
  start.textContent = "Not in this build (--features app)";
} else {
  const appTerm = terminal("t-app", 72, 16, { convertEol: false, disableStdin: false, cursorBlink: false });
  let timer = null;
  const now = () => performance.now() - t0;
  // Run the app's timers when it next needs it, as rs-rich-web's session
  // waits on `Driver::timeout`.
  const schedule = () => {
    clearTimeout(timer);
    if (rich.app_done()) return;
    const wait = rich.app_timeout(now());
    if (wait >= 0) {
      timer = setTimeout(() => { appTerm.write(rich.app_tick(now())); schedule(); }, Math.min(wait, 1000));
    }
  };
  appTerm.onData((data) => { appTerm.write(rich.app_input(data, now())); schedule(); });
  start.addEventListener("click", () => {
    appTerm.reset();
    appTerm.write(rich.app_start(appTerm.cols, appTerm.rows));
    appTerm.focus();
    schedule();
  });
}
