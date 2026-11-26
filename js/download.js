// Highlights the download for the visitor's system. Without JavaScript all
// three are shown the same way.
(function () {
  "use strict";
  function detect() {
    const ua = navigator.userAgent || "";
    const plat = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
    if (/Win/i.test(plat) || /Windows/i.test(ua)) return ["windows", "Windows"];
    if (/Mac/i.test(plat) || /Mac OS X/i.test(ua)) {
      if (/iPhone|iPad/i.test(ua)) return null;
      return ["macos", "macOS"];
    }
    if (/Android/i.test(ua)) return null;
    if (/Linux/i.test(plat) || /Linux/i.test(ua)) return ["linux", "Linux"];
    return null;
  }
  function init() {
    const found = detect();
    if (!found) return;
    const card = document.querySelector('.platform[data-os="' + found[0] + '"]');
    if (!card) return;
    card.classList.add("mine");
    card.querySelector(".yours").hidden = false;
    card.parentNode.prepend(card);
    const d = document.getElementById("detected");
    d.textContent = "It looks like you're on " + found[1] + ", so that download is first.";
    d.hidden = false;
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();
})();
