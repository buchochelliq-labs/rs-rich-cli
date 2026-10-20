"""Generate rs-rich's hero banner (docs/assets/rs-rich/hero-dark.svg and
hero-light.svg) from its wordmark and intuiTUIve's mark and mascot, and the
Python site's (docs/python/assets/hero-*.svg), whose install line is pip's.
The Python site is built from docs/python alone, so its mark and favicon are
copied there too. Run it after any of them changes."""
import random
import shutil
from pathlib import Path
ASSETS = Path(__file__).resolve().parent.parent / 'docs/assets'
A = str(ASSETS / 'rs-rich') + '/'
T = str(ASSETS / 'intuituive') + '/'
GOLD = '#c08600'
PURPLE = '#7c4dff'
body = lambda s: s.split('\n', 1)[1].rsplit('</svg>', 1)[0]
def make(theme, install='cargo add rs-rich'):
    dark = theme == 'dark'
    bg, grid, ink, sub, panel, pline, dim, ok = (
        ('#16141a', '#24212b', '#ecebf2', '#a9a4bd', '#1f1c26', '#3b3646', '#6f6a85', '#4caf50') if dark else
        ('#fdfcf9', '#efebe2', '#1c1a24', '#5b5670', '#ffffff', '#ddd6c6', '#8f8aa3', '#2e7d32'))
    W, H = 1200, 380
    o = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" role="img" aria-label="rs-rich: Python\'s rich, in Rust. Rich text, tables, Markdown and progress bars in the terminal, and intuiTUIve for full apps">']
    o.append(f'<defs><clipPath id="c"><rect width="{W}" height="{H}" rx="18"/></clipPath>'
             f'<pattern id="g" width="16" height="32" patternUnits="userSpaceOnUse"><path d="M16 0H0V32" fill="none" stroke="{grid}" stroke-width="1"/></pattern></defs>')
    o.append(f'<g clip-path="url(#c)"><rect width="{W}" height="{H}" fill="{bg}"/><rect width="{W}" height="{H}" fill="url(#g)"/>')
    # A few lit cells on the grid, in gold: styled spans.
    random.seed(11)
    for _ in range(26):
        x = random.randrange(0, W // 16) * 16; y = random.randrange(0, H // 32) * 32
        if (24 < x < 640 and 30 < y < 350) or (636 < x < 1040 and 20 < y < 330) or (x > 940 and y > 260):
            continue
        op = random.choice([0.18, 0.28, 0.45])
        o.append(f'<rect x="{x+1}" y="{y+1}" width="15" height="31" fill="{GOLD}" opacity="{op}"/>')
    o.append('</g>')
    # Wordmark and tagline.
    wm = open(A + f'wordmark-{theme}.svg').read()
    o.append(f'<g transform="translate(48 58) scale(1.6)">{body(wm)}</g>')
    font = 'font-family="ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif"'
    o.append(f'<text x="80" y="208" {font} font-size="32" font-weight="600" fill="{ink}">Python\'s rich, in Rust.</text>')
    o.append(f'<text x="80" y="244" {font} font-size="21" fill="{sub}">Styled text, tables, Markdown and progress bars</text>')
    o.append(f'<text x="80" y="272" {font} font-size="21" fill="{sub}">in the terminal, held to rich byte for byte.</text>')
    mono = 'font-family="ui-monospace,SFMono-Regular,Menlo,Consolas,monospace"'
    # 17px monospace is about 10.2px a character; 234 fits `cargo add rs-rich`.
    box = 234 + round((len(install) - len('cargo add rs-rich')) * 10.2)
    o.append(f'<rect x="80" y="296" width="{box}" height="40" rx="8" fill="{panel}" stroke="{pline}"/>'
             f'<text x="98" y="322" {mono} font-size="17" fill="{sub}"><tspan fill="{GOLD}">$</tspan> {install}</text>')
    # A terminal window: a panel holding a table, and a progress bar.
    X, Y, w, h = 640, 40, 380, 256
    o.append(f'<g><rect x="{X}" y="{Y}" width="{w}" height="{h}" rx="10" fill="{panel}" stroke="{pline}" stroke-width="2"/>'
             f'<circle cx="{X+18}" cy="{Y+16}" r="4.5" fill="{dim}"/><circle cx="{X+34}" cy="{Y+16}" r="4.5" fill="{dim}"/><circle cx="{X+50}" cy="{Y+16}" r="4.5" fill="{GOLD}"/>'
             f'<text x="{X+w-16}" y="{Y+21}" text-anchor="end" {mono} font-size="13" fill="{dim}">~/release</text>'
             f'<path d="M{X} {Y+32}H{X+w}" stroke="{pline}" stroke-width="2"/>')
    # The panel, with a title in its top border.
    px, py, pw, ph = X+16, Y+50, w-32, 140
    o.append(f'<rect x="{px}" y="{py}" width="{pw}" height="{ph}" rx="4" fill="none" stroke="{GOLD}" stroke-width="2"/>'
             f'<rect x="{px+pw/2-52}" y="{py-10}" width="104" height="20" fill="{panel}"/>'
             f'<text x="{px+pw/2}" y="{py+5}" text-anchor="middle" {mono} font-size="14" font-weight="700" fill="{GOLD}">Crates</text>')
    cols = (px+16, px+150, px+pw-16)
    o.append(f'<text x="{cols[0]}" y="{py+30}" {mono} font-size="14" font-weight="700" fill="{ink}">Crate</text>'
             f'<text x="{cols[1]}" y="{py+30}" {mono} font-size="14" font-weight="700" fill="{ink}">Status</text>'
             f'<text x="{cols[2]}" y="{py+30}" text-anchor="end" {mono} font-size="14" font-weight="700" fill="{ink}">Tests</text>'
             f'<path d="M{px+10} {py+40}H{px+pw-10}" stroke="{pline}" stroke-width="1.5"/>')
    rows = [('rs-rich', '✓ parity', ok, '1,204'), ('rs-rich-ext', '✓ built', ok, '863'), ('rs-rich-cli', '● running', GOLD, '412')]
    for i, (name, st, col, n) in enumerate(rows):
        yy = py+64+i*24
        o.append(f'<text x="{cols[0]}" y="{yy}" {mono} font-size="14" fill="{ink}">{name}</text>'
                 f'<text x="{cols[1]}" y="{yy}" {mono} font-size="14" fill="{col}">{st}</text>'
                 f'<text x="{cols[2]}" y="{yy}" text-anchor="end" {mono} font-size="14" fill="{sub}">{n}</text>')
    # A progress bar under the panel.
    by = py+ph+30
    o.append(f'<text x="{px}" y="{by+5}" {mono} font-size="14" fill="{ink}">Testing</text>'
             f'<rect x="{px+78}" y="{by-3}" width="200" height="8" rx="4" fill="{pline}"/>'
             f'<rect x="{px+78}" y="{by-3}" width="148" height="8" rx="4" fill="{GOLD}"/>'
             f'<text x="{px+pw}" y="{by+5}" text-anchor="end" {mono} font-size="14" fill="{sub}">74%</text></g>')
    # A nod to intuiTUIve: its mark and Memo, for full apps.
    nx, ny = 952, 336
    o.append(f'<g transform="translate({nx-60} {ny-22}) scale(0.5)">{body(open(T + "mark.svg").read())}</g>')
    o.append(f'<text x="{nx-20}" y="{ny-6}" {font} font-size="15" fill="{sub}">and <tspan fill="{PURPLE}" font-weight="600">intuiTUIve</tspan></text>'
             f'<text x="{nx-20}" y="{ny+12}" {font} font-size="15" fill="{sub}">for full apps</text>')
    o.append(f'<g transform="translate(1080 266) scale(0.78)">{body(open(T + "mascot/elephant-wave.svg").read())}</g>')
    o.append('</svg>')
    return '\n'.join(o) + '\n'
PY = ASSETS.parent / 'python' / 'assets'
PY.mkdir(exist_ok=True)
for name in ('mark.svg', 'favicon.svg'):
    shutil.copyfile(A + name, PY / name)
for t in ('dark', 'light'):
    open(A + f'hero-{t}.svg', 'w').write(make(t))
    open(PY / f'hero-{t}.svg', 'w').write(make(t, 'pip install rs-rich'))
