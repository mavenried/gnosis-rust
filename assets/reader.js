const bookFrame = document.getElementById('book-frame');
const footerEl = document.getElementById('footer');
const footerLocEl = document.getElementById('footer-loc');
const footerPageEl = document.getElementById('footer-page');

let currentBookId = null;
let bookInfo = null;
let currentChapter = 0;
let currentPage = 0;
let totalPagesInChapter = 1;
let startAtBodyText = false;
let pageListTotal = 0;
const WORDS_PER_LOCATION = 250;

function post(message) {
    if (window.webkit?.messageHandlers?.gnosis) {
        window.webkit.messageHandlers.gnosis.postMessage(JSON.stringify(message));
    }
}

function columnCenters() {
    const clientWidth = document.documentElement.clientWidth;
    const isSpread = clientWidth >= 1000;
    if (!isSpread) {
        const center = clientWidth / 2;
        return { left: center, right: center, single: true };
    }
    const quarter = clientWidth / 4;
    return {
        left: quarter,
        right: clientWidth - quarter,
        single: false,
    };
}

function formatTimeLeft(minutes) {
    if (!Number.isFinite(minutes) || minutes < 0) return '';
    if (minutes < 1) return 'Less than a minute left';
    const total = Math.round(minutes);
    const hours = Math.floor(total / 60);
    const mins = total % 60;
    return hours > 0 ? `${hours}h ${mins}m left` : `${mins}m left`;
}

function positionFooter(centers) {
    if (!centers) return;
    footerLocEl.style.left = `${Math.round(centers.left)}px`;
    footerPageEl.style.left = `${Math.round(centers.right)}px`;
    footerPageEl.style.display = centers.single ? 'none' : '';
}
window.addEventListener('resize', () => positionFooter(columnCenters()));

const THEMES = {
    light: { bg: '#ffffff', fg: '#000000', link: '#0066cc' },
    sepia: { bg: '#f1e8d0', fg: '#5b4636', link: '#008b8b' },
    gray: { bg: '#e0e0e0', fg: '#222222', link: '#4488cc' },
    dark: { bg: '#222222', fg: '#e0e0e0', link: '#77bbee' },
};

let currentStyle = {
    theme: 'light',
    fontFamily: null,
    fontSize: 100,
    lineHeight: 140,
    paragraphSpacing: 100,
    margin: 48,
    justify: false,
    rsvpWpm: 300,
};

function applyStyle() {
    const { bg, fg } = THEMES[currentStyle.theme] ?? THEMES.light;
    document.body.style.background = bg;
    footerEl.style.color = fg;
    rsvpOverlay.style.background = bg;
    rsvpOverlay.style.color = fg;
    searchPanel.style.background = bg;
    searchPanel.style.color = fg;
    tocPanel.style.background = bg;
    tocPanel.style.color = fg;

    // Push style into chapter frame if loaded
    try {
        if (bookFrame.contentDocument) {
            const root = bookFrame.contentDocument.documentElement;
            root.style.setProperty('--gnosis-bg', bg);
            root.style.setProperty('--gnosis-fg', fg);
            root.style.setProperty('--gnosis-font-size', `${currentStyle.fontSize || 100}%`);
            root.style.setProperty('--gnosis-line-height', `${(currentStyle.lineHeight || 140) / 100}`);
            root.style.setProperty('--gnosis-para-spacing', `${(currentStyle.paragraphSpacing ?? 100) / 100}em`);
            root.style.setProperty('--gnosis-margin', `${currentStyle.margin ?? 48}px`);
            root.style.setProperty('--gnosis-text-align', currentStyle.justify ? 'justify' : 'left');
            if (currentStyle.fontFamily) {
                root.style.setProperty('--gnosis-font-family', `"${currentStyle.fontFamily.replaceAll('"', '')}", serif`);
            } else {
                root.style.removeProperty('--gnosis-font-family');
            }
            bookFrame.contentWindow?.gnosisResize?.();
        }
    } catch (_) {}
}

window.gnosisSetStyle = style => {
    currentStyle = { ...currentStyle, ...style };
    if (style?.rsvpWpm) {
        rsvpWpm = Math.max(60, Math.min(1000, Math.round(style.rsvpWpm)));
        updateRsvpControls();
    }
    applyStyle();
};

window.gnosisSetStartMode = skipFrontMatter => {
    startAtBodyText = !!skipFrontMatter;
};

// Chapter Loading and Navigation
let chapterLoadPromise = null;
let chapterResolve = null;

function loadChapter(index, options = {}) {
    if (!bookInfo || bookInfo.total_chapters === 0) return Promise.resolve();
    const targetIdx = Math.max(0, Math.min(bookInfo.total_chapters - 1, index));
    currentChapter = targetIdx;

    if (options.direction) {
        const cls = options.direction > 0 ? 'turning-next' : 'turning-prev';
        bookFrame.classList.add(cls);
        post({ type: 'loading' });
    }

    chapterLoadPromise = new Promise(resolve => {
        chapterResolve = resolve;
    });

    const anchorPart = options.targetAnchor ? `#${options.targetAnchor}` : '';
    bookFrame.src = `gnosis-reader:///chapter/${currentBookId}/${targetIdx}${anchorPart}`;

    return chapterLoadPromise.then(() => {
        bookFrame.classList.remove('turning-next', 'turning-prev');
        applyStyle();
        if (options.targetPage === 'last') {
            bookFrame.contentWindow?.gnosisGoToPage?.(999999, false);
        } else if (typeof options.targetPage === 'number') {
            bookFrame.contentWindow?.gnosisGoToPage?.(options.targetPage, false);
        } else if (options.targetAnchor) {
            bookFrame.contentWindow?.gnosisGoToAnchor?.(options.targetAnchor);
        }
    });
}

bookFrame.addEventListener('load', () => {
    if (chapterResolve) {
        chapterResolve();
        chapterResolve = null;
    }
    applyStyle();
});

// Called by chapter bridge script on relocation
window.gnosisOnRelocate = detail => {
    currentChapter = detail.chapter;
    currentPage = detail.page;
    totalPagesInChapter = detail.totalPages;

    const centers = columnCenters();
    positionFooter(centers);

    const locParts = [];
    locParts.push(`Chapter ${detail.chapter + 1} of ${detail.totalChapters}`);
    locParts.push(`Page ${detail.page + 1} of ${detail.totalPages}`);
    if (detail.fraction != null) {
        locParts.push(`${Math.round(detail.fraction * 100)}%`);
    }
    const locText = locParts.join(' \u{b7} ');

    const remainingPages = Math.max(0, detail.totalPages - detail.page - 1);
    const rightText = formatTimeLeft((remainingPages * WORDS_PER_LOCATION) / rsvpWpm);

    if (centers?.single) {
        footerLocEl.textContent = [locText, rightText].filter(Boolean).join(' \u{b7} ');
        footerPageEl.textContent = '';
    } else {
        footerLocEl.textContent = locText;
        footerPageEl.textContent = rightText;
    }

    post({
        type: 'relocate',
        cfi: detail.locator,
        fraction: detail.fraction,
    });
};

window.gnosisNextChapter = () => {
    if (bookInfo && currentChapter + 1 < bookInfo.total_chapters) {
        loadChapter(currentChapter + 1, { targetPage: 0, direction: 1 });
    }
};

window.gnosisPrevChapter = () => {
    if (currentChapter > 0) {
        loadChapter(currentChapter - 1, { targetPage: 'last', direction: -1 });
    }
};

window.gnosisNext = () => {
    const frameWin = bookFrame.contentWindow;
    if (frameWin && frameWin.gnosisNext) {
        const handled = frameWin.gnosisNext();
        if (!handled) {
            window.gnosisNextChapter();
        }
    } else {
        window.gnosisNextChapter();
    }
};

window.gnosisPrev = () => {
    const frameWin = bookFrame.contentWindow;
    if (frameWin && frameWin.gnosisPrev) {
        const handled = frameWin.gnosisPrev();
        if (!handled) {
            window.gnosisPrevChapter();
        }
    } else {
        window.gnosisPrevChapter();
    }
};

window.gnosisScrollBy = (dx, dy) => {
    bookFrame.contentWindow?.gnosisScrollBy?.(dx, dy);
};

window.gnosisSnap = (vx, vy) => {
    bookFrame.contentWindow?.gnosisSnap?.(vx, vy);
};

window.gnosisNavigate = href => {
    if (!href) return;
    const [pathPart, anchorPart] = href.split('#');
    if (!pathPart && anchorPart) {
        bookFrame.contentWindow?.gnosisGoToAnchor?.(anchorPart);
        return;
    }
    // Match href against TOC or spine in bookInfo
    if (bookInfo?.toc) {
        const match = findTocTarget(bookInfo.toc, pathPart);
        if (match != null) {
            loadChapter(match.chapter_index, { targetAnchor: anchorPart || match.anchor });
            return;
        }
    }
    // Fallback: try parsing index or just go to anchor
    if (anchorPart) {
        bookFrame.contentWindow?.gnosisGoToAnchor?.(anchorPart);
    }
};

function findTocTarget(items, pathPart) {
    for (const item of items) {
        if (item.anchor === pathPart || (item.label && item.label.toLowerCase().includes(pathPart.toLowerCase()))) {
            return item;
        }
        if (item.children) {
            const childMatch = findTocTarget(item.children, pathPart);
            if (childMatch) return childMatch;
        }
    }
    return null;
}

window.gnosisOpenExternal = url => {
    post({ type: 'openexternal', url });
};

// Open Book Entry Point
window.gnosisOpenBook = async (bookId, locator, style) => {
    try {
        currentBookId = bookId;
        if (style) window.gnosisSetStyle(style);
        closeToc();
        closeSearch();

        const res = await fetch(`gnosis-reader:///book-info/${bookId}`);
        if (!res.ok) throw new Error(`HTTP ${res.status} fetching book info`);
        bookInfo = await res.json();

        tocListEl.replaceChildren();
        renderToc(bookInfo.toc);
        applyStyle();

        // Determine starting chapter and page
        let targetChapter = 0;
        let targetPage = 0;
        let targetAnchor = null;

        if (locator) {
            try {
                const parsed = JSON.parse(locator);
                if (typeof parsed.chapter === 'number') {
                    targetChapter = parsed.chapter;
                    targetPage = parsed.page || 0;
                    targetAnchor = parsed.anchor || null;
                }
            } catch (_) {
                // If legacy CFI or string, try estimating chapter from fraction
                const match = locator.match(/chapter[_-]?(\d+)/i);
                if (match) {
                    targetChapter = parseInt(match[1], 10);
                }
            }
        } else if (startAtBodyText && bookInfo.first_body_chapter > 0) {
            targetChapter = bookInfo.first_body_chapter;
        }

        await loadChapter(targetChapter, { targetPage, targetAnchor });

        post({
            type: 'ready',
            title: bookInfo.title ?? null,
        });
    } catch (err) {
        const message = String((err && err.message) || err);
        post({ type: 'error', message });
    }
};

// RSVP (Speed Reader) implementation
const rsvpOverlay = document.getElementById('rsvp-overlay');
const rsvpBeforeEl = document.getElementById('rsvp-before');
const rsvpPivotEl = document.getElementById('rsvp-pivot');
const rsvpAfterEl = document.getElementById('rsvp-after');
const rsvpWpmLabel = document.getElementById('rsvp-wpm-label');
const rsvpPlayPauseBtn = document.getElementById('rsvp-play-pause');
const rsvpCloseBtn = document.getElementById('rsvp-close');
const rsvpWpmDownBtn = document.getElementById('rsvp-wpm-down');
const rsvpWpmUpBtn = document.getElementById('rsvp-wpm-up');

const wordSegmenter = typeof Intl !== 'undefined' && Intl.Segmenter
    ? new Intl.Segmenter(undefined, { granularity: 'word' })
    : null;

function* textNodes(element) {
    const walker = document.createTreeWalker(
        element,
        NodeFilter.SHOW_TEXT,
        {
            acceptNode: node => {
                const tag = node.parentElement?.tagName?.toLowerCase();
                if (tag === 'script' || tag === 'style') return NodeFilter.FILTER_REJECT;
                return NodeFilter.FILTER_ACCEPT;
            }
        }
    );
    let node;
    while ((node = walker.nextNode())) yield node;
}

function buildWordList(doc) {
    if (!doc?.body || !wordSegmenter) return [];
    const words = [];
    for (const node of textNodes(doc.body)) {
        const str = node.nodeValue;
        if (!str || !str.trim()) continue;
        for (const seg of wordSegmenter.segment(str)) {
            if (seg.isWordLike) {
                words.push({ text: seg.segment });
            }
        }
    }
    return words;
}

let rsvpWords = [];
let rsvpIndex = 0;
let rsvpPlaying = false;
let rsvpTimer = null;
let rsvpWpm = 300;

function orpIndex(len) {
    if (len <= 1) return 0;
    if (len <= 5) return 1;
    if (len <= 9) return 2;
    if (len <= 13) return 3;
    return 4;
}

function renderRsvpWord(word) {
    const i = Math.min(orpIndex(word.length), word.length - 1);
    rsvpBeforeEl.textContent = word.slice(0, i);
    rsvpPivotEl.textContent = word[i] ?? '';
    rsvpAfterEl.textContent = word.slice(i + 1);
}

function rsvpDelay(word) {
    const base = 60000 / rsvpWpm;
    let mult = 1;
    if (word.length > 6) mult += (word.length - 6) * 0.06;
    if (/[.!?]["')\]]?$/.test(word)) mult += 1.2;
    else if (/[,;:]["')\]]?$/.test(word)) mult += 0.5;
    return base * mult;
}

function updateRsvpControls() {
    rsvpWpmLabel.textContent = `${rsvpWpm} WPM`;
    rsvpPlayPauseBtn.textContent = rsvpPlaying ? 'Pause' : 'Play';
}

function rsvpStep() {
    if (!rsvpPlaying) return;
    if (rsvpIndex >= rsvpWords.length) {
        rsvpAdvanceSection();
        return;
    }
    const word = rsvpWords[rsvpIndex];
    renderRsvpWord(word.text);
    rsvpIndex++;
    rsvpTimer = setTimeout(rsvpStep, rsvpDelay(word.text));
}

async function rsvpAdvanceSection() {
    if (bookInfo && currentChapter + 1 < bookInfo.total_chapters) {
        await loadChapter(currentChapter + 1, { targetPage: 0 });
        const doc = bookFrame.contentDocument;
        rsvpWords = buildWordList(doc);
        rsvpIndex = 0;
        rsvpStep();
    } else {
        rsvpPlaying = false;
        rsvpBeforeEl.textContent = '';
        rsvpPivotEl.textContent = '';
        rsvpAfterEl.textContent = 'Finished';
        updateRsvpControls();
    }
}

function enterRsvp() {
    const doc = bookFrame.contentDocument;
    if (!doc?.body) return;
    rsvpWords = buildWordList(doc);
    rsvpIndex = 0;
    rsvpPlaying = true;
    rsvpOverlay.classList.add('visible');
    document.body.classList.add('rsvp-active');
    updateRsvpControls();
    rsvpStep();
}

function exitRsvp() {
    rsvpPlaying = false;
    clearTimeout(rsvpTimer);
    rsvpOverlay.classList.remove('visible');
    document.body.classList.remove('rsvp-active');
}

window.gnosisToggleRsvp = () => {
    if (rsvpOverlay.classList.contains('visible')) exitRsvp();
    else enterRsvp();
};

window.gnosisSetRsvpWpm = wpm => {
    rsvpWpm = Math.max(60, Math.min(1000, Math.round(wpm)));
    updateRsvpControls();
    post({ type: 'rsvpWpm', wpm: rsvpWpm });
};

rsvpPlayPauseBtn.addEventListener('click', () => {
    rsvpPlaying = !rsvpPlaying;
    updateRsvpControls();
    if (rsvpPlaying) rsvpStep();
    else clearTimeout(rsvpTimer);
});
rsvpCloseBtn.addEventListener('click', exitRsvp);
rsvpWpmDownBtn.addEventListener('click', () => window.gnosisSetRsvpWpm(rsvpWpm - 25));
rsvpWpmUpBtn.addEventListener('click', () => window.gnosisSetRsvpWpm(rsvpWpm + 25));

document.addEventListener('keydown', e => {
    if (!rsvpOverlay.classList.contains('visible')) return;
    if (e.key === 'Escape') {
        exitRsvp();
        e.preventDefault();
    } else if (e.key === ' ') {
        rsvpPlaying = !rsvpPlaying;
        updateRsvpControls();
        if (rsvpPlaying) rsvpStep();
        else clearTimeout(rsvpTimer);
        e.preventDefault();
    } else if (e.key === 'ArrowUp') {
        window.gnosisSetRsvpWpm(rsvpWpm + 25);
        e.preventDefault();
    } else if (e.key === 'ArrowDown') {
        window.gnosisSetRsvpWpm(rsvpWpm - 25);
        e.preventDefault();
    }
});

// Table of Contents
const tocPanel = document.getElementById('toc-panel');
const tocCloseBtn = document.getElementById('toc-close');
const tocListEl = document.getElementById('toc-list');

function renderToc(items, depth = 0) {
    for (const item of items ?? []) {
        const btn = document.createElement('button');
        btn.className = 'panel-item';
        btn.style.paddingLeft = `${10 + depth * 16}px`;
        btn.textContent = item.label?.trim() || 'Untitled';
        btn.addEventListener('click', () => {
            post({ type: 'loading' });
            loadChapter(item.chapter_index, { targetAnchor: item.anchor });
            closeToc();
        });
        tocListEl.append(btn);
        if (item.children && item.children.length > 0) {
            renderToc(item.children, depth + 1);
        }
    }
}

function closeToc() {
    tocPanel.classList.remove('visible');
}
tocCloseBtn.addEventListener('click', closeToc);

window.gnosisToggleToc = () => {
    if (tocPanel.classList.contains('visible')) {
        closeToc();
    } else {
        closeSearch();
        tocPanel.classList.add('visible');
    }
};

// Native Search
const searchPanel = document.getElementById('search-panel');
const searchInput = document.getElementById('search-input');
const searchCloseBtn = document.getElementById('search-close');
const searchScopeBookBtn = document.getElementById('search-scope-book');
const searchScopeChapterBtn = document.getElementById('search-scope-chapter');
const searchStatusEl = document.getElementById('search-status');
const searchResultsEl = document.getElementById('search-results');

let searchScopeWholeBook = true;

function escapeHtml(str) {
    return str.replace(/[&<>"']/g, c => ({
        '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
    })[c]);
}

function renderSearchResult(item) {
    const btn = document.createElement('button');
    btn.className = 'panel-item';
    btn.innerHTML = `${escapeHtml(item.pre)}<mark>${escapeHtml(item.matched)}</mark>${escapeHtml(item.post)}`;
    btn.addEventListener('click', () => {
        post({ type: 'loading' });
        loadChapter(item.chapter_index);
        closeSearch();
    });
    searchResultsEl.append(btn);
}

function closeSearch() {
    searchPanel.classList.remove('visible');
}

async function runSearch(query) {
    searchResultsEl.replaceChildren();
    if (!query || !currentBookId) {
        searchStatusEl.textContent = '';
        return;
    }
    searchStatusEl.textContent = 'Searching…';

    try {
        const scopeParam = searchScopeWholeBook ? '' : `&chapter=${currentChapter}`;
        const url = `gnosis-reader:///book-search/${currentBookId}?q=${encodeURIComponent(query)}${scopeParam}`;
        const res = await fetch(url);
        const matches = await res.json();

        if (!matches || matches.length === 0) {
            searchStatusEl.textContent = 'No results';
            return;
        }

        searchStatusEl.textContent = `${matches.length} result${matches.length === 1 ? '' : 's'}`;

        let lastChapter = null;
        for (const m of matches) {
            if (m.chapter_index !== lastChapter) {
                lastChapter = m.chapter_index;
                const label = document.createElement('div');
                label.className = 'search-group-label';
                label.textContent = m.chapter_title || `Chapter ${m.chapter_index + 1}`;
                searchResultsEl.append(label);
            }
            renderSearchResult(m);
        }
    } catch (err) {
        searchStatusEl.textContent = 'Search failed';
    }
}

function setSearchScope(wholeBook) {
    searchScopeWholeBook = wholeBook;
    searchScopeBookBtn.classList.toggle('active', wholeBook);
    searchScopeChapterBtn.classList.toggle('active', !wholeBook);
    if (searchInput.value.trim()) runSearch(searchInput.value.trim());
}
searchScopeBookBtn.addEventListener('click', () => setSearchScope(true));
searchScopeChapterBtn.addEventListener('click', () => setSearchScope(false));

searchInput.addEventListener('keydown', e => {
    if (e.key === 'Enter') runSearch(searchInput.value.trim());
    else if (e.key === 'Escape') closeSearch();
});
searchCloseBtn.addEventListener('click', closeSearch);

window.gnosisToggleSearch = () => {
    if (searchPanel.classList.contains('visible')) {
        closeSearch();
    } else {
        closeToc();
        searchPanel.classList.add('visible');
        searchInput.focus();
        searchInput.select();
    }
};

document.addEventListener('keydown', e => {
    if (e.key === 'Escape') {
        if (tocPanel.classList.contains('visible')) closeToc();
        if (searchPanel.classList.contains('visible')) closeSearch();
    }
});
