// Glue between foliate-js's <foliate-view> element and the native side.
// The native side calls the window.gnosis* functions below; this script
// calls back into native code by posting JSON strings to the "gnosis"
// WebKit script message handler (see ui/reader.rs).
import './foliate-js/view.js'

const view = document.getElementById('view')

// foliate-js's own book loader (view.js's makeBook, used whenever
// view.open() is given a URL/File/directory) always does a plain
// `fetch()` + `await res.blob()` before any zip parsing can even start —
// fine for a normal-sized EPUB, but for a large one (a several-hundred-MB
// to multi-GB fixed-layout comic in particular) that means downloading and
// materializing the *entire* book up front, which is what hung/crashed the
// reader on big files.
//
// zip.js's own BlobReader only ever calls `.slice(start, end).arrayBuffer()`
// on whatever it's given — nothing about it actually requires a *real*
// Blob, just that interface — so instead of reimplementing zip.js's
// low-level (and not exported, so unextendable from here) internal Reader
// base class ourselves, this is a fake Blob whose `.arrayBuffer()` pulls
// its slice via an HTTP Range request against `gnosis-reader:///book/<id>`
// (which ui/reader_scheme.rs's backend serves as real 206 Partial Content
// responses) instead of ever fetching the whole file. Handed to zip.js's
// real, already-correct `BlobReader`, this gets us every other Reader
// invariant zip.js's internals expect for free.
class RangeBlob {
    constructor(url, size, start = 0, end = size) {
        this.url = url
        this.fullSize = size
        this.start = start
        this.end = end
        this.size = end - start
    }
    slice(start = 0, end = this.size) {
        const clampedStart = this.start + Math.max(0, start)
        const clampedEnd = this.start + Math.min(this.size, end)
        return new RangeBlob(this.url, this.fullSize, clampedStart, clampedEnd)
    }
    async arrayBuffer() {
        if (this.size <= 0) return new ArrayBuffer(0)
        const res = await fetch(this.url, {
            headers: { Range: `bytes=${this.start}-${this.end - 1}` },
            cache: 'no-store',
        })
        if (!res.ok) throw new Error(`${res.status} ${res.statusText}`)
        return res.arrayBuffer()
    }
}

// makeBook's internal makeZipLoader() does effectively this same
// entries -> loadText/loadBlob/getSize -> `new EPUB(loader)` sequence, just
// against a real Blob it first downloads in full via `fetch()+.blob()`.
// `view.open()` treats a value that isn't a string/File/directory as an
// already-built book object and uses it as-is (see view.js's open()), so
// the resulting EPUB instance is handed to it directly rather than going
// through makeBook(url) at all.
async function openBookOverRange(url) {
    const probe = await fetch(url, {
        headers: { Range: 'bytes=0-0' },
        cache: 'no-store',
    })
    if (!probe.ok) throw new Error(`${probe.status} ${probe.statusText}`)
    const size = Number(probe.headers.get('Content-Range')?.split('/')?.[1])
    if (!Number.isFinite(size) || size <= 0)
        throw new Error('gnosis-reader: scheme did not answer a Range request')

    const [{ EPUB }, { configure, ZipReader, BlobReader, TextWriter, BlobWriter }] = await Promise.all([
        import('./foliate-js/epub.js'),
        import('./foliate-js/vendor/zip.js'),
    ])
    configure({ useWebWorkers: false })

    const zipReader = new ZipReader(new BlobReader(new RangeBlob(url, size)))
    const entries = await zipReader.getEntries()
    const map = new Map(entries.map(entry => [entry.filename, entry]))
    const load = f => (name, ...args) => map.has(name) ? f(map.get(name), ...args) : null
    const loadText = load(entry => entry.getData(new TextWriter()))
    const loadBlob = load((entry, type) => entry.getData(new BlobWriter(type)))
    const getSize = name => map.get(name)?.uncompressedSize ?? 0

    return new EPUB({ loadText, loadBlob, getSize }).init()
}

function post(message) {
    if (window.webkit?.messageHandlers?.gnosis) {
        window.webkit.messageHandlers.gnosis.postMessage(JSON.stringify(message))
    }
}

function flattenToc(items, depth = 0) {
    const out = []
    for (const item of items ?? []) {
        out.push({ label: item.label, href: item.href, depth })
        if (item.subitems) out.push(...flattenToc(item.subitems, depth + 1))
    }
    return out
}

view.addEventListener('relocate', e => {
    const { cfi, fraction } = e.detail
    post({ type: 'relocate', cfi, fraction })
})

const THEMES = {
    light: { bg: '#ffffff', fg: '#000000', link: '#0066cc' },
    sepia: { bg: '#f1e8d0', fg: '#5b4636', link: '#008b8b' },
    gray: { bg: '#e0e0e0', fg: '#222222', link: '#4488cc' },
    dark: { bg: '#222222', fg: '#e0e0e0', link: '#77bbee' },
}

// Remembered across books: renderer.setStyles() only affects the renderer
// it's called on, and gnosisOpenBook() creates a fresh renderer for every
// book, so the chosen theme/font has to be reapplied on every open (see
// applyStyle() below) or it would silently reset each time you open a book.
let currentStyle = { theme: 'light', fontFamily: null, fontSize: 100 }

function buildCSS({ theme, fontFamily, fontSize }) {
    const { bg, fg, link } = THEMES[theme] ?? THEMES.light
    return `
        html, body {
            background: ${bg} !important;
            color: ${fg} !important;
        }
        body * {
            color: inherit !important;
            background-color: transparent !important;
            border-color: currentColor !important;
        }
        a:any-link {
            color: ${link} !important;
        }
        ${fontFamily ? `
        html, body, p, div, span, li, td, th, blockquote {
            font-family: "${fontFamily.replaceAll('"', '')}" !important;
        }` : ''}
        html {
            font-size: ${fontSize || 100}% !important;
        }
    `
}

function applyStyle() {
    view.renderer?.setStyles?.(buildCSS(currentStyle))
}

// `style` is `{ theme, fontFamily, fontSize }`; any field left out keeps its
// previous value. `fontFamily: null` means "use the book's own font".
window.gnosisSetStyle = style => {
    currentStyle = { ...currentStyle, ...style }
    applyStyle()
}

// Where a book with no saved position opens: at its true first page (the
// print-book convention, and the default), or skipping straight to the
// body-text landmark past any cover/title/copyright pages — some readers
// prefer that instead. Only affects books that don't have a resume position
// yet; a saved position always wins regardless of this setting.
let startAtBodyText = false
window.gnosisSetStartMode = skipFrontMatter => {
    startAtBodyText = !!skipFrontMatter
}

// `url` is the location the current book's bytes are served from (see the
// gnosis-reader:///book/<id> route in ui/reader_scheme.rs — the id is only
// there to give each book a distinct URL for fetch()'s cache). `lastCfi` is
// the resume position previously reported via a 'relocate' message, or null
// for a book opened for the first time.
window.gnosisOpenBook = async (url, lastCfi) => {
    try {
        // view.open() always creates a fresh renderer element and appends
        // it, but never removes whatever renderer a previous open() left
        // behind — closing first (a no-op the very first time) is what
        // actually detaches the old book's rendered content, which is what
        // makes switching books visibly show the new one instead of leaving
        // the old one sitting on top of it.
        view.close()
        const book = await openBookOverRange(url)
        await view.open(book)
        applyStyle()
        await view.init({
            lastLocation: lastCfi || undefined,
            showTextStart: !lastCfi && startAtBodyText,
        })
        post({
            type: 'ready',
            title: book?.metadata?.title ?? null,
            toc: flattenToc(book?.toc),
        })
    } catch (err) {
        const message = String((err && err.message) || err)
        const stack = err && err.stack ? String(err.stack) : null
        post({ type: 'error', message: stack ? `${message}\n${stack}` : message })
    }
}

// foliate-js's paginator has no built-in page-turn triggers (clicks,
// keyboard, scroll) of its own — the embedding app is expected to call
// these. See ui/reader.rs for what calls them.
window.gnosisGoTo = href => view.goTo(href)
window.gnosisNext = () => view.next()
window.gnosisPrev = () => view.prev()
