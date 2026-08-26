# foliate-js (vendored)

Source: https://github.com/johnfactotum/foliate-js (MIT license, see `LICENSE`).

Only the modules needed to render reflowable and fixed-layout EPUBs, plus
search, are vendored here — traced from `view.js`'s own import graph, not
the whole upstream repo (which also includes comic/FB2/MOBI/PDF loaders,
OPDS, a dictionary popup, and TTS, none of which Gnosis uses):

- `view.js` — the `<foliate-view>` custom element; main entry point.
- `epub.js`, `fixed-layout.js` — EPUB format loaders.
- `paginator.js` — the renderer.
- `epubcfi.js`, `progress.js`, `overlayer.js`, `text-walker.js` — support
  modules `view.js` depends on directly.
- `search.js` — matcher behind `view.search()`, dynamically imported by
  `view.js` only when search actually runs.
- `vendor/zip.js`, `vendor/fflate.js` — bundled zip reading, used by
  `epub.js` to read the EPUB (itself a zip archive) directly from bytes.

Not modified from upstream. `assets/reader.html` / `assets/reader.js` are
Gnosis's own glue code that loads `view.js` and bridges it to the native
side over `window.webkit.messageHandlers`.
