// Click on a screenshot to see it at full size, centred in the window,
// with its caption below and buttons (or the arrow keys) to go to the
// previous / next one.  Click outside the image, the × or Esc to close.
// Without JavaScript, the links simply open the image.
(function () {
  "use strict";
  function init() {
    const links = Array.from(document.querySelectorAll("a.zoom"));
    if (!links.length) return;

    const box = document.createElement("div");
    box.className = "lightbox";
    box.hidden = true;
    box.setAttribute("role", "dialog");
    box.setAttribute("aria-modal", "true");
    box.setAttribute("aria-label", "Screenshot");
    box.innerHTML =
      '<button type="button" class="lightbox-close" aria-label="Close">×</button>' +
      '<figure class="lightbox-figure">' +
      '  <img alt="">' +
      '  <figcaption class="lightbox-caption"></figcaption>' +
      '  <div class="lightbox-nav">' +
      '    <button type="button" class="lightbox-prev" aria-label="Previous screenshot">‹ Previous</button>' +
      '    <span class="lightbox-count"></span>' +
      '    <button type="button" class="lightbox-next" aria-label="Next screenshot">Next ›</button>' +
      '  </div>' +
      '</figure>';
    document.body.appendChild(box);
    const img = box.querySelector("img");
    const caption = box.querySelector(".lightbox-caption");
    const count = box.querySelector(".lightbox-count");
    let current = -1;

    function captionFor(link) {
      const fig = link.closest("figure");
      const cap = fig && fig.querySelector("figcaption");
      const thumb = link.querySelector("img");
      return cap ? cap.textContent : (thumb ? thumb.alt : "");
    }

    function show(i) {
      current = (i + links.length) % links.length;
      const link = links[current];
      const thumb = link.querySelector("img");
      img.src = link.href;
      img.alt = thumb ? thumb.alt : "";
      caption.textContent = captionFor(link);
      count.textContent = (current + 1) + " / " + links.length;
    }

    function open(i) {
      show(i);
      box.hidden = false;
      document.body.classList.add("lightbox-open");
      box.querySelector(".lightbox-next").focus({ preventScroll: true });
    }

    function close() {
      box.hidden = true;
      document.body.classList.remove("lightbox-open");
      if (current >= 0) links[current].focus({ preventScroll: true });
    }

    box.querySelector(".lightbox-prev").addEventListener("click", (e) => { e.stopPropagation(); show(current - 1); });
    box.querySelector(".lightbox-next").addEventListener("click", (e) => { e.stopPropagation(); show(current + 1); });
    box.querySelector(".lightbox-close").addEventListener("click", (e) => { e.stopPropagation(); close(); });
    // a click on the dark background closes; on the picture, goes to the next one
    box.addEventListener("click", (e) => { if (e.target === box) close(); });
    img.addEventListener("click", (e) => { e.stopPropagation(); show(current + 1); });

    document.addEventListener("keydown", (e) => {
      if (box.hidden) return;
      if (e.key === "Escape") close();
      else if (e.key === "ArrowRight") show(current + 1);
      else if (e.key === "ArrowLeft") show(current - 1);
    });

    links.forEach((link, i) => {
      link.addEventListener("click", (e) => {
        if (e.ctrlKey || e.metaKey || e.shiftKey || e.button !== 0) return;
        e.preventDefault();
        open(i);
      });
    });
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init);
  else init();
})();
