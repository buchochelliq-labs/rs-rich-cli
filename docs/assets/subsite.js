// intuiTUIve's pages (guide/intuituive/) are a sub-site of rs-rich: they
// keep intuiTUIve's purple accent, and every other page takes rs-rich's gold
// (`custom`, in extra.css). Material's instant navigation swaps pages
// without a reload, so this runs on each page change (`document$`), and again
// whenever the scheme toggle writes the configured accent back.
function subsiteAccent() {
  const want = /\/guide\/intuituive\//.test(location.pathname) ? "deep-purple" : "custom";
  if (document.body.getAttribute("data-md-color-accent") !== want) {
    document.body.setAttribute("data-md-color-accent", want);
  }
}
new MutationObserver(subsiteAccent).observe(document.body, {
  attributes: true,
  attributeFilter: ["data-md-color-accent"],
});
if (window.document$) {
  document$.subscribe(subsiteAccent);
} else {
  document.addEventListener("DOMContentLoaded", subsiteAccent);
}
subsiteAccent();
