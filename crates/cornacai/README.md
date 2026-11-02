# Cornacai

A terminal client for coding agents that speak the
[Agent Client Protocol](https://agentclientprotocol.com) (ACP): Claude Code
through its ACP adapter, Gemini CLI, Codex and the others. Cornacai
(kor-NAH-kai: Portuguese *cornaca*, the elephant's rider and guide, plus
"AI") is the [rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli)
workspace's showcase app for conversations, built on intuiTUIve,
rs-rich-acp and rs-rich-embed.

```sh
cargo install cornacai
cornacai --agent "gemini --experimental-acp"
```

- **Sessions as tabs** on one agent connection (Ctrl+N, Alt+N/Alt+P).
- **Answers stream in as Markdown** and the view follows the end until you
  scroll up; thoughts and tool calls fold into cards with a live status.
- **The prompt** grows as you write, keeps a history, and completes `@`
  file mentions from the workspace; each goes to the agent as a
  `resource_link`.
- **What the agent asks is shown before it happens:** permission prompts
  with one button per option; every write reviewed as a diff, hunk by
  hunk; every command asked about. Reads and writes are kept inside the
  workspace.
- **A shell pane** (Ctrl+T), your own: nothing in it reaches the agent.
- **Agent text is untrusted:** control sequences are neutralised and
  hidden characters revealed.

The user guide is
[Cornacai](https://buchochelliq-labs.github.io/rs-rich-cli/guide/cornacai/)
on the docs site, and the design is
[`docs/design/cornacai.md`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/cornacai.md).

Written clean-room from the public ACP specification. Inspired by Toad's
public feature list; none of Toad's code is used. MIT.
