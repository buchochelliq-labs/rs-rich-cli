# Over SSH

There are two ways to browse with a remote machine, and they suit
different jobs.

| | Noctiluca on the remote | `--ssh HOST` here |
|---|---|---|
| Where Chrome runs | the remote | this machine |
| Where the network goes from | the remote | the remote, through the tunnel |
| What crosses SSH | the terminal's output: text, and pictures in the budget | page traffic, as the remote fetches it |
| Its `localhost` | the remote's | the remote's |
| Sound | off (it would play on the remote); `--audio` turns it on there | plays here, on by default |
| The browser profile | on the remote | on this machine |

- **Run Noctiluca on the remote** (`ssh devbox`, then `noctiluca URL`)
  when the link is slow or you want nothing installed here: the text
  projection is cheap over SSH, and pictures step down to fit
  [the byte budget](media.md#images). The remote needs Chrome.
- **Use `--ssh HOST`** when Chrome is here but the pages are there: a dev
  server on the remote's `localhost`, or a network only the remote reaches.
  Nothing runs on the remote but `sshd`, and no frames cross SSH.

## `--ssh HOST`

```bash
noctiluca --ssh me@devbox http://localhost:3000
```

Noctiluca starts `ssh -N -D 127.0.0.1:PORT -- me@devbox`, waits until the
tunnel answers, and starts Chrome with every request going through it.
Before the browser takes the screen, ssh can ask for a password or to
accept a host key, as it always does; `Ctrl+C` stops waiting.

- **`localhost` is the remote's.** Chrome sends loopback addresses
  through the tunnel too, and resolves no name itself: the remote looks
  every name up, so names only the remote knows work.
- **Your SSH setup is used.** `~/.ssh/config` applies (a `Host` alias, a
  user, a jump host, a key). Give ssh more options with `--ssh-arg`, once
  per word: `--ssh-arg=-p --ssh-arg=2222`. The host is never read as an
  option: one starting with `-` is refused.
- **The status bar** says `via ssh HOST`. If the tunnel drops (the
  network goes, the remote reboots), it says so and reconnects by itself,
  waiting longer after each failed try; reload (`Ctrl+R`, `F5`, or `↻`)
  reconnects at once. A reconnect never prompts (ssh's `BatchMode`), so
  it works with keys and an agent; a login that needs a password needs a
  new run.
- **Quitting** stops the tunnel. On Linux it also stops if Noctiluca is
  killed.

### What it means for privacy

- The tunnel listens on `127.0.0.1` only: nothing on the network can use
  it. Its port is drawn at random each run, not handed out in order. On
  Linux, Noctiluca checks the port is ssh's own and refuses to start
  Chrome if another user's program took it.
- **On a machine you share, run Noctiluca on the remote instead.** Chrome
  only takes a proxy on a TCP port, and ssh's proxy asks for no password,
  so another user of **this** machine who finds the port while the tunnel
  runs can still reach the remote's network as you. The random port makes
  it harder to find, not impossible.
- WebRTC sends nothing past the tunnel, and QUIC is off, so no request
  leaves this machine directly.
- The remote sees every request, as your network would. HTTPS is still
  encrypted end to end; plain HTTP is readable there.
- With `--profile NAME`, the kept profile's cookies and storage for
  `localhost` are shared between runs with and without `--ssh`: the
  remote's `localhost:3000` and your own are one site to Chrome. The
  default temporary profile has no such overlap.

See [Troubleshooting](troubleshooting.md#over-ssh) for the errors.
