// Applies the saved theme before first paint (kept out of inline script for CSP).
(function () {
  try {
    var t = localStorage.getItem("wda.theme") || "auto";
    var d = t === "dark" || (t === "auto" && matchMedia("(prefers-color-scheme: dark)").matches);
    document.documentElement.dataset.theme = d ? "dark" : "light";
  } catch (e) {
    document.documentElement.dataset.theme = "light";
  }
})();
