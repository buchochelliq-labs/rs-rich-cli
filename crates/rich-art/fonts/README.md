# Bundled fonts — provenance

## `standard.flf`

The default FIGfont, from the standard FIGlet font distribution. Its own
comment block (preserved verbatim inside the `.flf`, as the format intends)
carries the attribution and permission notice:

> Standard by Glenn Chappell & Ian Chai 3/93 -- based on Frank's .sig
> Includes ISO Latin-1
> figlet release 2.1 -- 12 Aug 1994
> Modified for figlet 2.2 by John Cowan <cowan@ccil.org>
>   to add Latin-{2,3,4,5} support (Unicode U+0100-017F).
> **Permission is hereby given to modify this font, as long as the
> modifier's name is placed on a comment line.**
>
> Modified by Paul Burton <solution@earthlink.net> 12/96 …
> Font modified May 20, 2012 by patorjk to add the 0xCA0 character

This file is **unmodified**; the comment block is left intact so the notice
travels with it.

This is the only third-party asset in the crate. Everything else — the `.flf`
parser and the layout/smushing engine — is original code in `src/figlet.rs`.

## Using other fonts

The crate does not need a bundled font. Any FIGfont works:

```rust
let font = FigletFont::parse(&std::fs::read_to_string("slant.flf")?)?;
let banner = Figlet::new("hello").font(font);
```

Large font collections (the `figlet` package, `pyfiglet`, figlet.org) can be
used this way without vendoring anything into this repository.
