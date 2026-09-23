// Run each case in a real PTY, replay the exact bytes into xterm.js in
// Chromium, and screenshot the terminal. Usage: node shoot.js cases.json outdir
const fs = require('fs');
const path = require('path');
const pty = require('node-pty');
const { chromium } = require('playwright-core');

const [casesFile, outDir] = process.argv.slice(2);
const cases = JSON.parse(fs.readFileSync(casesFile, 'utf8'));
fs.mkdirSync(outDir, { recursive: true });

function run(c) {
  return new Promise((resolve) => {
    const env = { ...process.env, TERM: 'xterm-256color', COLORTERM: 'truecolor', ...(c.env || {}) };
    delete env.NO_COLOR;
    for (const k of c.unset || []) delete env[k];
    const p = pty.spawn(c.cmd, c.args, { cols: c.cols || 100, rows: c.rows || 30, cwd: c.cwd, env });
    const chunks = [];
    p.onData((d) => chunks.push(d));
    for (const step of c.input || []) setTimeout(() => p.write(step.data), step.afterMs);
    const kill = setTimeout(() => p.kill(), c.timeoutMs || 15000);
    p.onExit(({ exitCode, signal }) => {
      clearTimeout(kill);
      resolve({ data: chunks.join(''), exitCode, signal });
    });
  });
}

(async () => {
  const xtermJs = fs.readFileSync(require.resolve('@xterm/xterm/lib/xterm.js'), 'utf8');
  const xtermCss = fs.readFileSync(require.resolve('@xterm/xterm/css/xterm.css'), 'utf8');
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined });
  const results = [];
  for (const c of cases) {
    const r = await run(c);
    fs.writeFileSync(path.join(outDir, `${c.name}.raw`), r.data);
    const page = await browser.newPage({ deviceScaleFactor: 2 });
    await page.setContent(`<html><head><style>${xtermCss}
      body{margin:0;background:#161922;padding:16px;display:inline-block}
      .title{font:600 14px sans-serif;color:#c8ccd4;margin:0 0 8px 2px}
      </style></head><body><div class="title"></div><div id="t"></div><script>${xtermJs}</script></body></html>`);
    await page.evaluate(({ data, cols, rows, title }) => new Promise((done) => {
      document.querySelector('.title').textContent = title;
      const term = new Terminal({ cols, rows, convertEol: false, fontSize: 14,
        fontFamily: 'DejaVu Sans Mono, monospace', theme: { background: '#161922' } });
      term.open(document.getElementById('t'));
      term.write(data, done);
    }), { data: r.data, cols: c.cols || 100, rows: c.rows || 30, title: c.title || c.name });
    await page.waitForTimeout(150);
    await (await page.$('body')).screenshot({ path: path.join(outDir, `${c.name}.png`) });
    await page.close();
    results.push({ name: c.name, cmd: [c.cmd, ...c.args].join(' '), exitCode: r.exitCode, signal: r.signal, bytes: r.data.length });
    console.log(`${c.name}: exit=${r.exitCode} signal=${r.signal} bytes=${r.data.length}`);
  }
  fs.writeFileSync(path.join(outDir, 'results.json'), JSON.stringify(results, null, 2));
  await browser.close();
})();
