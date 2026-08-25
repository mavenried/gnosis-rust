import './foliate-js/view.js'

const view = document.getElementById('view')

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
    const { cfi, fraction, location } = e.detail
    post({
        type: 'relocate',
        cfi,
        fraction,
        locationCurrent: location?.current ?? null,
        locationTotal: location?.total ?? null,
    })
})

const THEMES = {
    light: { bg: '#ffffff', fg: '#000000', link: '#0066cc' },
    sepia: { bg: '#f1e8d0', fg: '#5b4636', link: '#008b8b' },
    gray: { bg: '#e0e0e0', fg: '#222222', link: '#4488cc' },
    dark: { bg: '#222222', fg: '#e0e0e0', link: '#77bbee' },
}

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

window.gnosisSetStyle = style => {
    currentStyle = { ...currentStyle, ...style }
    applyStyle()
}

let startAtBodyText = false
window.gnosisSetStartMode = skipFrontMatter => {
    startAtBodyText = !!skipFrontMatter
}

window.gnosisOpenBook = async (url, lastCfi) => {
    try {
        view.close()
        const book = await openBookOverRange(url)
        await view.open(book)
        view.renderer?.setAttribute('animated', '')
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

window.gnosisGoTo = href => view.goTo(href)
window.gnosisNext = () => view.next()
window.gnosisPrev = () => view.prev()
