// Mount an asciinema player on every `.tape-player[data-cast]`, replacing its
// GIF fallback. Material's instant navigation swaps pages without a reload,
// so mount on each page change (`document$`), not only on load.
function mountTapes() {
  if (typeof AsciinemaPlayer === "undefined") return;
  document.querySelectorAll(".tape-player[data-cast]:not([data-mounted])").forEach((el) => {
    el.dataset.mounted = "1";
    el.replaceChildren();
    AsciinemaPlayer.create(el.dataset.cast, el, {
      fit: "width",
      idleTimeLimit: 2,
      poster: el.dataset.poster || "npt:0:1",
      terminalFontFamily: '"Fira Code", "DejaVu Sans Mono", Menlo, monospace',
    });
  });
}
if (window.document$) {
  document$.subscribe(mountTapes);
} else {
  document.addEventListener("DOMContentLoaded", mountTapes);
}
