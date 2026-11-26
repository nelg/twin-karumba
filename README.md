# Twin Karumba website (GitHub Pages)

This branch (`gh-pages`) is the project website: plain HTML, CSS and
JavaScript, no build step. Same layout as the Bub's Brothers site.

- `index.html`: features and screenshots.
- `download.html`: links to the latest release's files (`releases/latest/download/<name>`),
  so it needs no change for a new release as long as the file names stay the same.
- `js/download.js`: puts the visitor's own system first.
- `images/`: logo, ships and screenshots.

To publish: in the repository settings on GitHub, *Pages*, choose
"Deploy from a branch", branch `gh-pages`, folder `/ (root)`.
