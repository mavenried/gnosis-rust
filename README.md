<h1 align="center">Gnosis</h1>
<p align="center">
A clean, friendly ebook app for Linux — organize your library and read your
books.
</p>

## What it does

- **Keep your books organized.** Point Gnosis at the folders where your
  EPUBs live and it builds a library for you — covers, titles, authors,
  series, all in one place. Browse by author or series like flipping
  through a music library.
- **Pick up where you left off.** Gnosis remembers your spot in every book,
  along with your own theme, font, and text size choices for each one.
- **Read comfortably.** Light, sepia, gray, and dark themes; use the
  publisher's own fonts or pick your own; a clean interface that fades out
  of the way while you're actually reading.
- **Speed read.** A built-in RSVP mode flashes your book one word at a
  time at whatever pace you choose, letting you get through pages faster
  without losing your place.
- **Find anything.** Search inside a chapter or the whole book at once,
  jump straight to a match.
- **Mark books as read** and see your progress at a glance from the
  library view.

## Getting Gnosis

Head over to the [Releases page](https://github.com/mavenried/gnosis/releases)
and download the latest `gnosis-linux-x86_64` file. Then, in a terminal, make
it runnable and start it:

```sh
chmod +x gnosis-linux-x86_64
./gnosis-linux-x86_64
```

That's it — no installer, nothing else to set up. Gnosis keeps your library
and settings in a folder on your computer
(`~/.local/share/gnosis`), so it's easy to find or back up later if you
ever want to.

## Building from source

If you'd rather build Gnosis yourself, you'll need Rust (2024 edition) and
the GTK 4, libadwaita, and WebKitGTK 6.0 development packages.

On Arch Linux:

```sh
sudo pacman -S gtk4 libadwaita webkitgtk-6.0
```

On other distributions, install the equivalent packages (e.g.
`webkitgtk6.0-devel` on Fedora, `libwebkitgtk-6.0-dev` on recent
Debian/Ubuntu), then:

```sh
cargo run
```
