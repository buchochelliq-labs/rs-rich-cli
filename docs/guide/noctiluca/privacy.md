# Privacy and security

Noctiluca is a terminal in front of a real Chrome. Chrome does the
browsing, with its sandbox on. Noctiluca reads what Chrome laid out,
cleans it, and draws it. This page says what is kept, what is sent, and
what a page can and cannot do to your terminal.

## The browser profile

A browser profile holds cookies, site storage, the cache and history.

**By default, each run gets a new temporary profile**: a private folder
(only your user can read it) in your system's temporary directory, named
`rich-embed-chrome-…`. It is removed when Noctiluca ends: when you quit,
on `Ctrl+C`, on SIGTERM, SIGHUP or SIGQUIT (closing the terminal, say),
and when Noctiluca panics. So each run starts signed out of
everything, and leaves nothing behind. Only a process that cannot clean
up (killed with SIGKILL, or a machine that loses power) leaves its folder
in the temporary directory, readable only by you, for you or the system
to delete.

**A kept profile is opt-in.** `--profile NAME`, or `name = "NAME"` under
`[profile]`, keeps one profile, with its cookies and storage, between
runs, so you stay signed in. It lives in Noctiluca's data directory under
`profiles/NAME` (`~/.local/share/noctiluca/profiles/NAME` on Linux), made
private to your user. A name is letters, digits, `-` and `_`. Delete the
folder to forget everything in it.

## Downloads

**Downloads are refused** in 0.0.22: Chrome is told to deny every one, and
nothing is written. There is no switch to allow them yet.

What is designed for later: a download goes into a private quarantine
folder, under its download id so a page cannot choose its name or place,
Noctiluca asks you first, and nothing there is opened or run.

## The sandbox

Chrome runs as a separate, headless process with its sandbox on.
Noctiluca never turns the sandbox off, and drops any switch that would.
The sandbox does not run as root, so neither does Noctiluca: run it as a
normal user.

Noctiluca talks to Chrome over DevTools through a pipe on Linux and
macOS, with no network port. On Windows it uses a port on `127.0.0.1`,
which other users of the same machine could reach.

Chrome starts with no extensions, no sync, no first-run screens and its
background networking switched off, and muted unless you ask for sound.

## Page text is cleaned

Everything a page says is untrusted: its text, title, addresses, link
targets, alt text, option labels, download names, and the titles in your
bookmarks file. Before any of it reaches your terminal, Noctiluca strips:

- control characters (C0 and C1) and DEL, so no escape sequence can pass:
  a page cannot move your cursor, change your terminal's title, clear the
  screen or make a terminal link;
- bidirectional overrides, embeddings, isolates and marks, so text cannot
  read in a false order (`exe.png` shown for `gnp.exe`).

Square brackets in page text are shown as they are, never read as rich
markup, so a page cannot style itself in your terminal either.

## What runs in the page

No page script runs in Noctiluca's process. Noctiluca reads the page with
two DevTools calls the page cannot observe: a snapshot of the laid-out
document and the accessibility tree.

To know when to read again, one small script of Noctiluca's runs in an
**isolated world**: the page cannot see it or its globals. It only says
"something changed", at most every 50 ms, and never what. It also notices
a field's value changing with no event, by comparing the values every
half second inside Chrome; password fields are left out.

**Password fields** are never read at all. Their values never reach the
screen, the model or `--dump`.

`--no-js` turns the page's own scripts off.

## Bot checks

To hear the isolated script, Noctiluca turns on DevTools' `Runtime`
domain. Some sites' bot checks notice that, and may treat Noctiluca as
automation: you may see a challenge page, or be refused.

This is accepted. Noctiluca does not hide that it drives Chrome, and it
never tries to get past bot checks, CAPTCHAs or sign-in walls. If a site
refuses it, use another browser for that site.

## What is sent where

- **Noctiluca sends nothing of its own.** It has no telemetry, no update
  check and no account.
- **Chrome loads the pages you open**, as any browser does, with the
  cookies of the profile it runs in.
- **Searches** go to your search engine: by default DuckDuckGo's HTML
  search, with no tracking parameters added. Only what the address bar
  does not read as an address or a keyword is searched. Change it with
  `[search] url`.
- **Keywords** open the address their template makes, and nothing else.
- **Your config and bookmarks** stay on your machine. Noctiluca writes
  the config only when you bind or remove a site key, and bookmarks only
  when you add or remove one, each in one atomic step.
- **Sound** plays on the machine Chrome runs on.
