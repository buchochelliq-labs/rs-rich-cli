"""Generate intuiTUIve's hero banner (docs/assets/intuituive/hero-dark.svg and
hero-light.svg) from the wordmark and the mascots. Run it after either changes."""
import random, re, sys
from pathlib import Path
A=str(Path(__file__).resolve().parent.parent / 'docs/assets/intuituive') + '/'
body=lambda s: s.split('\n',1)[1].rsplit('</svg>',1)[0]
def make(theme):
    dark = theme=='dark'
    bg, grid, ink, sub, panel, pline, dim = (
        ('#15131d','#221f2d','#ecebf2','#a9a4bd','#1e1b29','#3a3550','#6f6a85') if dark else
        ('#f7f5fc','#ebe7f6','#1c1a24','#5b5670','#ffffff','#d6cff0','#9a94b0'))
    W,H=1200,380
    o=[f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" role="img" aria-label="intuiTUIve: terminal apps from signals, with Memo the elephant and her calf Tui">']
    o.append(f'<defs><clipPath id="c"><rect width="{W}" height="{H}" rx="18"/></clipPath>'
             f'<pattern id="g" width="16" height="32" patternUnits="userSpaceOnUse"><path d="M16 0H0V32" fill="none" stroke="{grid}" stroke-width="1"/></pattern></defs>')
    o.append(f'<g clip-path="url(#c)"><rect width="{W}" height="{H}" fill="{bg}"/><rect width="{W}" height="{H}" fill="url(#g)"/>')
    # A few lit cells on the grid: the cells a signal redraws.
    random.seed(7)
    for _ in range(26):
        x=random.randrange(0,W//16)*16; y=random.randrange(0,H//32)*32
        if (24<x<640 and 40<y<340) or (636<x<980 and 30<y<320) or (x>960 and y>200): continue
        op=random.choice([0.18,0.28,0.45])
        o.append(f'<rect x="{x+1}" y="{y+1}" width="15" height="31" fill="#7c4dff" opacity="{op}"/>')
    o.append('</g>')
    # Wordmark and tagline.
    wm=open(A+('wordmark-dark.svg' if dark else 'wordmark-light.svg')).read()
    o.append(f'<g transform="translate(48 70) scale(1.55)">{body(wm)}</g>')
    font='font-family="ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif"'
    o.append(f'<text x="80" y="208" {font} font-size="30" font-weight="600" fill="{ink}">Terminal apps from signals.</text>')
    o.append(f'<text x="80" y="244" {font} font-size="21" fill="{sub}">Describe the screen once. When a signal changes,</text>')
    o.append(f'<text x="80" y="272" {font} font-size="21" fill="{sub}">only the cells that changed are redrawn.</text>')
    mono='font-family="ui-monospace,SFMono-Regular,Menlo,Consolas,monospace"'
    o.append(f'<rect x="80" y="296" width="330" height="40" rx="8" fill="{panel}" stroke="{pline}"/>'
             f'<text x="98" y="322" {mono} font-size="17" fill="{sub}"><tspan fill="#7c4dff">$</tspan> cargo add rs-rich-intuituive</text>')
    # A small app window.
    X,Y,w,h=640,58,320,250
    o.append(f'<g><rect x="{X}" y="{Y}" width="{w}" height="{h}" rx="10" fill="{panel}" stroke="{pline}" stroke-width="2"/>'
             f'<circle cx="{X+18}" cy="{Y+16}" r="4.5" fill="{dim}"/><circle cx="{X+34}" cy="{Y+16}" r="4.5" fill="{dim}"/><circle cx="{X+50}" cy="{Y+16}" r="4.5" fill="#7c4dff"/>'
             f'<text x="{X+w-16}" y="{Y+21}" text-anchor="end" {mono} font-size="13" fill="{dim}">to-do</text>'
             f'<path d="M{X} {Y+32}H{X+w}" stroke="{pline}" stroke-width="2"/>')
    items=[('[x]','Draw the screen once',False),('[ ]','Keep state in signals',True),('[ ]','Redraw what changed',False),('[ ]','Ship it',False)]
    for i,(box,txt,sel) in enumerate(items):
        yy=Y+48+i*34
        if sel: o.append(f'<rect x="{X+10}" y="{yy-4}" width="{w-20}" height="30" rx="5" fill="#7c4dff"/>')
        col='#ffffff' if sel else ink
        o.append(f'<text x="{X+22}" y="{yy+17}" {mono} font-size="16" fill="{col}">{box} {txt}</text>')
    # Sparkline row and status bar.
    bars=[6,10,8,14,12,18,15,22,19,26,23,28]
    for i,b in enumerate(bars):
        o.append(f'<rect x="{X+22+i*14}" y="{Y+214-b}" width="10" height="{b}" rx="2" fill="#7c4dff" opacity="{0.45+0.5*i/len(bars):.2f}"/>')
    o.append(f'<text x="{X+w-18}" y="{Y+212}" text-anchor="end" {mono} font-size="13" fill="{dim}">1/4 done</text>'
             f'<path d="M{X} {Y+224}H{X+w}" stroke="{pline}" stroke-width="2"/>'
             f'<text x="{X+16}" y="{Y+241}" {mono} font-size="12" fill="{dim}">space tick · a add · q quit</text></g>')
    # Memo and Tui, by the window.
    mt=open(A+'mascot/memo-and-tui.svg').read()
    o.append(f'<g transform="translate(978 222) scale(0.94)">{body(mt)}</g>')
    o.append('</svg>')
    return '\n'.join(o)+'\n'
for t in ('dark','light'):
    open(A+f'hero-{t}.svg','w').write(make(t))
