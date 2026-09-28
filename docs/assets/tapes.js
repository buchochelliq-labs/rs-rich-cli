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

// Gallery cards (`a.tape-card`): the still becomes the tape's GIF while the
// card is hovered or has focus, and goes back when it has neither. Under
// `prefers-reduced-motion: reduce` the still stays. `data-gif` is relative
// to the still's own URL, so it survives the site's URL layout.
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

function mountGallery() {
  document.querySelectorAll("a.tape-card:not([data-mounted])").forEach((card) => {
    const img = card.querySelector("img[data-gif]");
    if (!img) return;
    card.dataset.mounted = "1";
    const still = img.getAttribute("src");
    const gif = new URL(img.dataset.gif, img.src).href;
    let hovered = false;
    let focused = false;
    const update = () => {
      const play = (hovered || focused) && !reducedMotion.matches;
      if (play === card.hasAttribute("data-playing")) return;
      card.toggleAttribute("data-playing", play);
      img.src = play ? gif : still;
    };
    card.addEventListener("mouseenter", () => { hovered = true; update(); });
    card.addEventListener("mouseleave", () => { hovered = false; update(); });
    card.addEventListener("focus", () => { focused = true; update(); });
    card.addEventListener("blur", () => { focused = false; update(); });
    reducedMotion.addEventListener("change", update);
  });
}

function mountAll() {
  mountTapes();
  mountGallery();
}
if (window.document$) {
  document$.subscribe(mountAll);
} else {
  document.addEventListener("DOMContentLoaded", mountAll);
}
