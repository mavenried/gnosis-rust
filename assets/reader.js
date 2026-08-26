import './foliate-js/view.js'
import { textWalker } from './foliate-js/text-walker.js'

const view = document.getElementById('view')
const fileInput = document.getElementById('file-input')

function pickBookFile() {
    return new Promise((resolve, reject) => {
        fileInput.onchange = () => {
            const file = fileInput.files?.[0]
            fileInput.value = ''
            if (file) resolve(file)
            else reject(new Error('gnosis-reader: no file selected'))
        }
        fileInput.click()
    })
}

async function openBookFromFile(file) {
    const [{ EPUB }, { configure, ZipReader, BlobReader, TextWriter, BlobWriter }] = await Promise.all([
        import('./foliate-js/epub.js'),
        import('./foliate-js/vendor/zip.js'),
    ])
    configure({ useWebWorkers: false })

    const zipReader = new ZipReader(new BlobReader(file))
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

const footerEl = document.getElementById('footer')
const footerLocEl = document.getElementById('footer-loc')
const footerPageEl = document.getElementById('footer-page')

function columnCenters() {
    const size = view.renderer?.size
    if (!size) return null
    const clientWidth = view.clientWidth
    const outer = Math.max(0, (clientWidth - size) / 2)
    const g = 0.07
    const gap = (g / (1 - g)) * size
    const maxInlineSize = 720
    const maxColumnCount = 2
    const divisor = Math.min(maxColumnCount, Math.ceil(size / maxInlineSize))
    if (divisor <= 1) {
        const center = outer + size / 2
        return { left: center, right: center, single: true }
    }
    const columnWidth = size / divisor - gap
    return {
        left: outer + gap / 2 + columnWidth / 2,
        right: outer + size - gap / 2 - columnWidth / 2,
        single: false,
    }
}

let pageListTotal = 0
const WORDS_PER_LOCATION = 250

function formatTimeLeft(minutes) {
    if (!Number.isFinite(minutes) || minutes < 0) return ''
    if (minutes < 1) return 'Less than a minute left'
    const total = Math.round(minutes)
    const hours = Math.floor(total / 60)
    const mins = total % 60
    return hours > 0 ? `${hours}h ${mins}m left` : `${mins}m left`
}

function positionFooter(centers) {
    if (!centers) return
    footerLocEl.style.left = `${Math.round(centers.left)}px`
    footerPageEl.style.left = `${Math.round(centers.right)}px`
    footerPageEl.style.display = centers.single ? 'none' : ''
}
window.addEventListener('resize', () => positionFooter(columnCenters()))

function wrapSection(section) {
    if (section.__gnosisRefs !== undefined) return
    const realLoad = section.load
    const realUnload = section.unload
    section.__gnosisRefs = 0
    section.__gnosisPromise = null
    section.load = function () {
        if (!section.__gnosisPromise) section.__gnosisPromise = realLoad?.call(section)
        section.__gnosisRefs++
        return section.__gnosisPromise
    }
    section.unload = function () {
        section.__gnosisRefs = Math.max(0, section.__gnosisRefs - 1)
        if (section.__gnosisRefs === 0) {
            section.__gnosisPromise = null
            realUnload?.call(section)
        }
    }
}

const PREFETCH_RADIUS = 2
let prefetched = new Set()
view.addEventListener('load', ({ detail }) => {
    const sections = view.book?.sections
    if (!sections) return
    const idx = detail.index
    const next = new Set()
    for (let d = 1; d <= PREFETCH_RADIUS; d++) {
        if (idx - d >= 0) next.add(idx - d)
        if (idx + d < sections.length) next.add(idx + d)
    }
    for (const i of prefetched) if (!next.has(i)) sections[i]?.unload?.()
    prefetched = next
    for (const i of prefetched) {
        const section = sections[i]
        if (!section) continue
        wrapSection(section)
        section.load()
    }
})

view.addEventListener('relocate', e => {
    const { cfi, fraction, location, pageItem } = e.detail

    const centers = columnCenters()
    positionFooter(centers)

    const parts = []
    if (location?.current != null && location?.total != null)
        parts.push(`Location ${location.current + 1} of ${location.total}`)
    if (fraction != null) parts.push(`${Math.round(fraction * 100)}%`)
    const locText = parts.join(' \u{b7} ')
    const rightText = pageItem?.label && pageListTotal
        ? `Page ${pageItem.label} of ${pageListTotal}`
        : location?.current != null && location?.total != null
            ? formatTimeLeft((Math.max(0, location.total - location.current) * WORDS_PER_LOCATION) / rsvpWpm)
            : ''

    if (centers?.single) {
        footerLocEl.textContent = [locText, rightText].filter(Boolean).join(' \u{b7} ')
        footerPageEl.textContent = ''
    } else {
        footerLocEl.textContent = locText
        footerPageEl.textContent = rightText
    }

    post({ type: 'relocate', cfi, fraction })
})

view.addEventListener('click', e => {
    const size = view.renderer?.size
    if (!size) return
    const outerWidth = view.clientWidth
    const margin = (outerWidth - size) / 2
    if (margin <= 0) return
    if (e.clientX < margin) view.prev()
    else if (e.clientX > outerWidth - margin) view.next()
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
    const { bg, fg } = THEMES[currentStyle.theme] ?? THEMES.light
    document.body.style.background = bg
    footerEl.style.color = fg
    rsvpOverlay.style.background = bg
    rsvpOverlay.style.color = fg
    searchPanel.style.background = bg
    searchPanel.style.color = fg
}

window.gnosisSetStyle = style => {
    currentStyle = { ...currentStyle, ...style }
    if (style?.rsvpWpm) {
        rsvpWpm = Math.max(60, Math.min(1000, Math.round(style.rsvpWpm)))
        updateRsvpControls()
    }
    applyStyle()
}

const rsvpOverlay = document.getElementById('rsvp-overlay')
const rsvpBeforeEl = document.getElementById('rsvp-before')
const rsvpPivotEl = document.getElementById('rsvp-pivot')
const rsvpAfterEl = document.getElementById('rsvp-after')
const rsvpWpmLabel = document.getElementById('rsvp-wpm-label')
const rsvpPlayPauseBtn = document.getElementById('rsvp-play-pause')
const rsvpCloseBtn = document.getElementById('rsvp-close')
const rsvpWpmDownBtn = document.getElementById('rsvp-wpm-down')
const rsvpWpmUpBtn = document.getElementById('rsvp-wpm-up')

const wordSegmenter = typeof Intl !== 'undefined' && Intl.Segmenter
    ? new Intl.Segmenter(undefined, { granularity: 'word' })
    : null

function* wordMatcher(strs, makeRange) {
    if (!wordSegmenter) return
    for (let i = 0; i < strs.length; i++) {
        const str = strs[i]
        if (!str || !str.trim()) continue
        for (const seg of wordSegmenter.segment(str)) {
            if (!seg.isWordLike) continue
            yield { text: seg.segment, range: makeRange(i, seg.index, i, seg.index + seg.segment.length) }
        }
    }
}

function buildWordList(doc) {
    if (!doc?.body) return []
    return [...textWalker(doc.body, wordMatcher)]
}

let rsvpWords = []
let rsvpIndex = 0
let rsvpPlaying = false
let rsvpTimer = null
let rsvpWpm = 300
let rsvpSectionIndex = null

function orpIndex(len) {
    if (len <= 1) return 0
    if (len <= 5) return 1
    if (len <= 9) return 2
    if (len <= 13) return 3
    return 4
}

function renderRsvpWord(word) {
    const i = Math.min(orpIndex(word.length), word.length - 1)
    rsvpBeforeEl.textContent = word.slice(0, i)
    rsvpPivotEl.textContent = word[i] ?? ''
    rsvpAfterEl.textContent = word.slice(i + 1)
}

function rsvpDelay(word) {
    const base = 60000 / rsvpWpm
    let mult = 1
    if (word.length > 6) mult += (word.length - 6) * 0.06
    if (/[.!?]["')\]]?$/.test(word)) mult += 1.2
    else if (/[,;:]["')\]]?$/.test(word)) mult += 0.5
    return base * mult
}

function updateRsvpControls() {
    rsvpWpmLabel.textContent = `${rsvpWpm} WPM`
    rsvpPlayPauseBtn.textContent = rsvpPlaying ? 'Pause' : 'Play'
}

function rsvpStep() {
    if (!rsvpPlaying) return
    if (rsvpIndex >= rsvpWords.length) {
        rsvpAdvanceSection()
        return
    }
    const word = rsvpWords[rsvpIndex]
    renderRsvpWord(word.text)
    rsvpIndex++
    rsvpTimer = setTimeout(rsvpStep, rsvpDelay(word.text))
}

async function rsvpAdvanceSection() {
    const beforeIndex = rsvpSectionIndex
    await view.renderer?.nextSection?.()
    const contents = view.renderer?.getContents?.()?.[0]
    if (!contents || contents.index === beforeIndex) {
        rsvpPlaying = false
        rsvpBeforeEl.textContent = ''
        rsvpPivotEl.textContent = ''
        rsvpAfterEl.textContent = 'Finished'
        updateRsvpControls()
        return
    }
    rsvpSectionIndex = contents.index
    rsvpWords = buildWordList(contents.doc)
    rsvpIndex = 0
    rsvpStep()
}

function rsvpStartIndex(words) {
    const range = view.lastLocation?.range
    if (!range) return 0
    const idx = words.findIndex(w => {
        try {
            return range.comparePoint(w.range.startContainer, w.range.startOffset) >= 0
        } catch {
            return false
        }
    })
    return idx === -1 ? 0 : idx
}

function enterRsvp() {
    const contents = view.renderer?.getContents?.()?.[0]
    if (!contents?.doc) return
    rsvpSectionIndex = contents.index
    rsvpWords = buildWordList(contents.doc)
    rsvpIndex = rsvpStartIndex(rsvpWords)
    rsvpPlaying = true
    rsvpOverlay.classList.add('visible')
    document.body.classList.add('rsvp-active')
    updateRsvpControls()
    rsvpStep()
}

function exitRsvp() {
    rsvpPlaying = false
    clearTimeout(rsvpTimer)
    const word = rsvpWords[Math.min(rsvpIndex, rsvpWords.length - 1)]
    if (word) view.renderer?.scrollToAnchor?.(word.range)
    rsvpOverlay.classList.remove('visible')
    document.body.classList.remove('rsvp-active')
}

window.gnosisToggleRsvp = () => {
    if (rsvpOverlay.classList.contains('visible')) exitRsvp()
    else enterRsvp()
}

window.gnosisSetRsvpWpm = wpm => {
    rsvpWpm = Math.max(60, Math.min(1000, Math.round(wpm)))
    updateRsvpControls()
    post({ type: 'rsvpWpm', wpm: rsvpWpm })
}

rsvpPlayPauseBtn.addEventListener('click', () => {
    rsvpPlaying = !rsvpPlaying
    updateRsvpControls()
    if (rsvpPlaying) rsvpStep()
    else clearTimeout(rsvpTimer)
})
rsvpCloseBtn.addEventListener('click', exitRsvp)
rsvpWpmDownBtn.addEventListener('click', () => window.gnosisSetRsvpWpm(rsvpWpm - 25))
rsvpWpmUpBtn.addEventListener('click', () => window.gnosisSetRsvpWpm(rsvpWpm + 25))

document.addEventListener('keydown', e => {
    if (!rsvpOverlay.classList.contains('visible')) return
    if (e.key === 'Escape') {
        exitRsvp()
        e.preventDefault()
    } else if (e.key === ' ') {
        rsvpPlaying = !rsvpPlaying
        updateRsvpControls()
        if (rsvpPlaying) rsvpStep()
        else clearTimeout(rsvpTimer)
        e.preventDefault()
    } else if (e.key === 'ArrowUp') {
        window.gnosisSetRsvpWpm(rsvpWpm + 25)
        e.preventDefault()
    } else if (e.key === 'ArrowDown') {
        window.gnosisSetRsvpWpm(rsvpWpm - 25)
        e.preventDefault()
    }
})

const searchPanel = document.getElementById('search-panel')
const searchInput = document.getElementById('search-input')
const searchCloseBtn = document.getElementById('search-close')
const searchScopeBookBtn = document.getElementById('search-scope-book')
const searchScopeChapterBtn = document.getElementById('search-scope-chapter')
const searchStatusEl = document.getElementById('search-status')
const searchResultsEl = document.getElementById('search-results')

let searchScopeWholeBook = true
let searchGeneration = 0

function escapeHtml(str) {
    return str.replace(/[&<>"']/g, c => ({
        '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
    })[c])
}

function renderSearchResult(cfi, excerpt) {
    const btn = document.createElement('button')
    btn.className = 'search-result'
    btn.innerHTML = `${escapeHtml(excerpt.pre)}<mark>${escapeHtml(excerpt.match)}</mark>${escapeHtml(excerpt.post)}`
    btn.addEventListener('click', () => {
        post({ type: 'loading' })
        view.goTo(cfi)
        closeSearch()
    })
    searchResultsEl.append(btn)
}

function closeSearch() {
    searchPanel.classList.remove('visible')
    view.clearSearch()
    searchGeneration++
}

async function runSearch(query) {
    const generation = ++searchGeneration
    searchResultsEl.replaceChildren()
    view.clearSearch()
    if (!query) {
        searchStatusEl.textContent = ''
        return
    }
    searchStatusEl.textContent = 'Searching…'
    const opts = { query, drawOptions: { color: '#d1453b' } }
    if (!searchScopeWholeBook) {
        const idx = view.renderer?.getContents?.()?.[0]?.index
        if (idx != null) opts.index = idx
    }
    let count = 0
    for await (const result of view.search(opts)) {
        if (generation !== searchGeneration) return
        if (result === 'done') {
            searchStatusEl.textContent = count
                ? `${count} result${count === 1 ? '' : 's'}`
                : 'No results'
        } else if (result.subitems) {
            count += result.subitems.length
            const label = document.createElement('div')
            label.className = 'search-group-label'
            label.textContent = result.label || 'Untitled'
            searchResultsEl.append(label)
            for (const { cfi, excerpt } of result.subitems) renderSearchResult(cfi, excerpt)
        } else if (result.cfi) {
            count++
            renderSearchResult(result.cfi, result.excerpt)
        } else if (result.progress != null && !count) {
            searchStatusEl.textContent = `Searching… ${Math.round(result.progress * 100)}%`
        }
    }
}

function setSearchScope(wholeBook) {
    searchScopeWholeBook = wholeBook
    searchScopeBookBtn.classList.toggle('active', wholeBook)
    searchScopeChapterBtn.classList.toggle('active', !wholeBook)
    if (searchInput.value.trim()) runSearch(searchInput.value.trim())
}
searchScopeBookBtn.addEventListener('click', () => setSearchScope(true))
searchScopeChapterBtn.addEventListener('click', () => setSearchScope(false))

searchInput.addEventListener('keydown', e => {
    if (e.key === 'Enter') runSearch(searchInput.value.trim())
    else if (e.key === 'Escape') closeSearch()
})
searchCloseBtn.addEventListener('click', closeSearch)

document.addEventListener('keydown', e => {
    if (!searchPanel.classList.contains('visible')) return
    if (e.key === 'Escape' && document.activeElement !== searchInput) {
        closeSearch()
        e.preventDefault()
    }
})

window.gnosisToggleSearch = () => {
    if (searchPanel.classList.contains('visible')) {
        closeSearch()
    } else {
        searchPanel.classList.add('visible')
        searchInput.focus()
        searchInput.select()
    }
}

let startAtBodyText = false
window.gnosisSetStartMode = skipFrontMatter => {
    startAtBodyText = !!skipFrontMatter
}

window.gnosisOpenBook = async (lastCfi, style) => {
    try {
        if (style) window.gnosisSetStyle(style)
        prefetched = new Set()
        view.close()
        const file = await pickBookFile()
        const book = await openBookFromFile(file)
        await view.open(book)
        pageListTotal = book?.pageList?.length ?? 0
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
window.gnosisScrollBy = (dx, dy) => view.renderer?.scrollBy(dx, dy)
window.gnosisSnap = (vx, vy) => view.renderer?.snap(vx, vy)
