<h1 align="center">Gnosis</h1>
<p align="center">
A beautiful, practical ebook library manager for GTK, drawing its reading
experience from <a href="https://github.com/johnfactotum/foliate">Foliate</a>.
</p>

## Features

- **Library management** — scan folders for EPUBs, edit metadata (title,
  author, series, book number), with optional write-back to the EPUB file
  itself (a copy alongside the original, or in place).
- **Reader** — a WebKit-based EPUB renderer built on
  [foliate-js](https://github.com/johnfactotum/foliate-js), with theme and
  font selection, publisher-font support, configurable start position, and
  resume-where-you-left-off. Streams large books via HTTP Range requests
  instead of loading them into memory whole.
- **Library browsing** — search, sort (title/author/date added/series),
  filter by reading status, and dedicated browsable Authors/Series pages
  with custom cover images, music-player style.
- **Background maintenance** — non-blocking metadata refresh, a dedicated
  series/book-number rescan, and an in-app activity log.

## Requirements

- Rust (2024 edition)
- GTK 4, libadwaita, WebKitGTK 6.0, and their development headers

On Arch Linux:

```sh
sudo pacman -S gtk4 libadwaita webkitgtk-6.0
```

On other distributions, install the equivalent GTK 4, libadwaita, and
WebKitGTK 6.0 development packages (e.g. `webkitgtk6.0-devel` on Fedora,
`libwebkitgtk-6.0-dev` on recent Debian/Ubuntu).

## Building and running

```sh
cargo run
```

Gnosis stores its library database, cached cover art, and settings under
your XDG data directory (`~/.local/share/gnosis` on Linux).

### Pre-built binaries

Each [GitHub release](https://github.com/mavenried/gnosis/releases)
includes a prebuilt Linux x86_64 binary, built automatically by
[`.github/workflows/release.yml`](.github/workflows/release.yml).

## Acknowledgments

The reader is built on a vendored subset of
[foliate-js](https://github.com/johnfactotum/foliate-js) by John Factotum,
included under the MIT license (see `assets/foliate-js/LICENSE`).
