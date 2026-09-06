use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use epub::doc::EpubDoc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::reader_prefs::ReaderPrefs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpineChapter {
    pub index: usize,
    pub idref: String,
    pub path: PathBuf,
    pub mime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TocItem {
    pub label: String,
    pub chapter_index: usize,
    pub anchor: Option<String>,
    pub children: Vec<TocItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookInfo {
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub total_chapters: usize,
    pub first_body_chapter: usize,
    pub toc: Vec<TocItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    pub chapter_index: usize,
    pub chapter_title: Option<String>,
    pub pre: String,
    pub matched: String,
    pub post: String,
}

pub struct EpubReader {
    pub id: Uuid,
    #[allow(dead_code)]
    pub path: PathBuf,
    pub title: String,
    pub author: Option<String>,
    pub spine: Vec<SpineChapter>,
    pub toc: Vec<TocItem>,
    pub first_body_chapter: usize,
    doc: EpubDoc<BufReader<File>>,
}

impl EpubReader {
    pub fn open(id: Uuid, path: &Path) -> Result<Self> {
        let doc = EpubDoc::new(path)
            .map_err(|e| anyhow!("{e}"))
            .with_context(|| format!("opening epub {}", path.display()))?;

        let title = doc
            .get_title()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| {
                path.file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Untitled".to_string())
            });

        let author = doc
            .mdata("creator")
            .map(|item| item.value.trim().to_string())
            .filter(|a| !a.is_empty());

        let mut spine = Vec::new();
        for (i, item) in doc.spine.iter().enumerate() {
            let (rel_path, mime) = if let Some(res) = doc.resources.get(&item.idref) {
                (res.path.clone(), res.mime.clone())
            } else {
                (PathBuf::from(&item.idref), "application/xhtml+xml".to_string())
            };
            spine.push(SpineChapter {
                index: i,
                idref: item.idref.clone(),
                path: rel_path,
                mime,
            });
        }

        let toc = build_toc(&doc, &spine);
        let first_body_chapter = find_first_body_chapter(&spine, &toc);

        Ok(Self {
            id,
            path: path.to_path_buf(),
            title,
            author,
            spine,
            toc,
            first_body_chapter,
            doc,
        })
    }

    pub fn book_info(&self) -> BookInfo {
        BookInfo {
            id: self.id.to_string(),
            title: self.title.clone(),
            author: self.author.clone(),
            total_chapters: self.spine.len(),
            first_body_chapter: self.first_body_chapter,
            toc: self.toc.clone(),
        }
    }

    #[allow(dead_code)]
    pub fn total_chapters(&self) -> usize {
        self.spine.len()
    }

    pub fn get_chapter_raw(&mut self, index: usize) -> Result<(String, String, PathBuf)> {
        if index >= self.spine.len() {
            bail!("Chapter index {} out of bounds (total {})", index, self.spine.len());
        }
        let chapter = self.spine[index].clone();
        self.doc.set_current_chapter(index);

        let content = if let Some((html, _)) = self.doc.get_resource_str(&chapter.idref) {
            html
        } else if let Some((html, _)) = self.doc.get_current_str() {
            html
        } else if let Some(bytes) = self.doc.get_resource_by_path(&chapter.path) {
            String::from_utf8(bytes).context("chapter is not valid UTF-8")?
        } else {
            bail!("Could not load chapter {} content", index);
        };

        Ok((content, chapter.mime, chapter.path))
    }

    pub fn get_injected_chapter(
        &mut self,
        index: usize,
        prefs: &ReaderPrefs,
    ) -> Result<(String, String)> {
        let (raw_html, mime, chapter_path) = self.get_chapter_raw(index)?;
        let chapter_dir = chapter_path
            .parent()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        let base_href = if chapter_dir.is_empty() {
            format!("gnosis-reader:///book/{}/", self.id)
        } else {
            format!("gnosis-reader:///book/{}/{}/", self.id, chapter_dir.trim_matches('/'))
        };

        let injected = inject_html(&raw_html, &base_href, prefs, index, self.spine.len());
        Ok((injected, mime))
    }

    pub fn search(&mut self, query: &str, chapter_filter: Option<usize>) -> Vec<SearchMatch> {
        let q_lower = query.trim().to_lowercase();
        if q_lower.is_empty() {
            return Vec::new();
        }

        let mut matches = Vec::new();
        let total = self.spine.len();

        for i in 0..total {
            if let Some(target) = chapter_filter {
                if target != i {
                    continue;
                }
            }

            let Ok((raw_html, _, _)) = self.get_chapter_raw(i) else {
                continue;
            };

            let text = strip_html_tags(&raw_html);
            let text_lower = text.to_lowercase();
            let chapter_title = self.find_chapter_title(i);

            let mut start = 0;
            while let Some(pos) = text_lower[start..].find(&q_lower) {
                let match_idx = start + pos;
                let pre = safe_slice(&text, match_idx.saturating_sub(40), match_idx).trim_start().to_string();
                let match_end = (match_idx + q_lower.len()).min(text.len());
                let matched = safe_slice(&text, match_idx, match_end).to_string();
                let post_end = (match_end + 40).min(text.len());
                let post = safe_slice(&text, match_end, post_end).trim_end().to_string();

                matches.push(SearchMatch {
                    chapter_index: i,
                    chapter_title: chapter_title.clone(),
                    pre,
                    matched,
                    post,
                });

                if matches.len() >= 300 {
                    return matches;
                }

                start = match_idx + q_lower.len();
                if start >= text.len() {
                    break;
                }
            }
        }

        matches
    }

    fn find_chapter_title(&self, chapter_index: usize) -> Option<String> {
        fn search_toc(items: &[TocItem], target: usize) -> Option<String> {
            for item in items {
                if item.chapter_index == target {
                    return Some(item.label.clone());
                }
                if let Some(found) = search_toc(&item.children, target) {
                    return Some(found);
                }
            }
            None
        }
        search_toc(&self.toc, chapter_index)
    }
}

fn build_toc(doc: &EpubDoc<BufReader<File>>, spine: &[SpineChapter]) -> Vec<TocItem> {
    fn convert_nav(
        point: &epub::doc::NavPoint,
        doc: &EpubDoc<BufReader<File>>,
        spine: &[SpineChapter],
    ) -> TocItem {
        let content_str = point.content.to_string_lossy();
        let (file_str, anchor) = match content_str.split_once('#') {
            Some((f, a)) => (f, Some(a.to_string())),
            None => (content_str.as_ref(), None),
        };

        let file_path = PathBuf::from(file_str);
        let chapter_index = doc
            .resource_uri_to_chapter(&file_path)
            .or_else(|| {
                spine.iter().position(|s| {
                    s.path == file_path
                        || s.path.ends_with(&file_path)
                        || file_path.ends_with(&s.path)
                        || s.path.file_name() == file_path.file_name()
                })
            })
            .unwrap_or(0);

        let children = point
            .children
            .iter()
            .map(|c| convert_nav(c, doc, spine))
            .collect();

        TocItem {
            label: point.label.trim().to_string(),
            chapter_index,
            anchor,
            children,
        }
    }

    doc.toc.iter().map(|p| convert_nav(p, doc, spine)).collect()
}

fn find_first_body_chapter(spine: &[SpineChapter], toc: &[TocItem]) -> usize {
    fn find_in_toc(items: &[TocItem]) -> Option<usize> {
        for item in items {
            let label = item.label.to_lowercase();
            if label.contains("chapter")
                || label.contains("prologue")
                || label.contains("introduction")
                || label.contains("part ")
                || label.contains("book ")
                || label.contains("act ")
                || label.starts_with("1")
                || label.starts_with("i.")
                || label.starts_with("i ")
            {
                return Some(item.chapter_index);
            }
            if let Some(c) = find_in_toc(&item.children) {
                return Some(c);
            }
        }
        None
    }

    if let Some(idx) = find_in_toc(toc) {
        return idx;
    }

    for (i, s) in spine.iter().enumerate() {
        let name = s.path.to_string_lossy().to_lowercase();
        if !name.contains("cover")
            && !name.contains("title")
            && !name.contains("copy")
            && !name.contains("toc")
            && !name.contains("nav")
        {
            return i;
        }
    }

    0
}

fn safe_slice(text: &str, mut start: usize, mut end: usize) -> &str {
    if start >= text.len() || start >= end {
        return "";
    }
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    end = end.min(text.len());
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    if start < end {
        &text[start..end]
    } else {
        ""
    }
}

fn strip_html_tags(html: &str) -> String {
    let mut text = String::with_capacity(html.len() / 2);
    let mut in_script_or_style = false;
    let mut tag_name = String::new();

    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '<' {
            tag_name.clear();
            i += 1;
            while i < chars.len() && chars[i] != '>' && !chars[i].is_whitespace() {
                tag_name.push(chars[i]);
                i += 1;
            }
            let t = tag_name.to_lowercase();
            if t == "script" || t == "style" {
                in_script_or_style = true;
            } else if t == "/script" || t == "/style" {
                in_script_or_style = false;
            }
            while i < chars.len() && chars[i] != '>' {
                i += 1;
            }
            text.push(' ');
        } else if !in_script_or_style {
            text.push(c);
        }
        i += 1;
    }

    decode_html_entities(&text)
}

fn decode_html_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

pub fn inject_html(
    raw_html: &str,
    base_href: &str,
    prefs: &ReaderPrefs,
    chapter_index: usize,
    total_chapters: usize,
) -> String {
    let css = build_injected_css(prefs);
    let script = build_injected_script(chapter_index, total_chapters);
    let base_tag = format!("<base href=\"{base_href}\" />\n");

    let head_injection = format!("{base_tag}<style id=\"gnosis-reader-style\">\n{css}\n</style>\n");
    let script_injection = format!("<script id=\"gnosis-chapter-script\">\n{script}\n</script>\n");

    let mut html = raw_html.to_string();

    // Insert head elements
    if let Some(pos) = html.find("<head>") {
        html.insert_str(pos + 6, &head_injection);
    } else if let Some(pos) = html.find("<head ") {
        if let Some(end_bracket) = html[pos..].find('>') {
            html.insert_str(pos + end_bracket + 1, &head_injection);
        }
    } else if let Some(pos) = html.find("<html>") {
        html.insert_str(pos + 6, &format!("<head>{head_injection}</head>"));
    }

    // Wrap body contents in #gnosis-viewport > #gnosis-content
    if let Some(body_start) = html.find("<body") {
        if let Some(open_tag_end) = html[body_start..].find('>') {
            let content_start = body_start + open_tag_end + 1;
            if let Some(body_end) = html.rfind("</body>") {
                let body_content = html[content_start..body_end].to_string();
                let wrapped = format!(
                    "<div id=\"gnosis-viewport\"><div id=\"gnosis-content\">{body_content}</div></div>{script_injection}"
                );
                html.replace_range(content_start..body_end, &wrapped);
            }
        }
    }

    html
}

fn build_injected_css(prefs: &ReaderPrefs) -> String {
    let (bg, fg, link) = match prefs.theme.as_str() {
        "sepia" => ("#f1e8d0", "#5b4636", "#008b8b"),
        "gray" => ("#e0e0e0", "#222222", "#4488cc"),
        "dark" => ("#222222", "#e0e0e0", "#77bbee"),
        _ => ("#ffffff", "#000000", "#0066cc"),
    };

    let font_family_rule = if let Some(ref family) = prefs.font_family {
        format!("font-family: \"{}\", serif !important;", family.replace('"', ""))
    } else {
        String::new()
    };

    let text_align = if prefs.justify { "justify" } else { "left" };
    let font_size = prefs.font_size;
    let line_height = (prefs.line_height as f64) / 100.0;
    let para_spacing = (prefs.paragraph_spacing as f64) / 100.0;
    let margin = prefs.margin;

    format!(
        r#"
:root {{
    --gnosis-bg: {bg};
    --gnosis-fg: {fg};
    --gnosis-link: {link};
    --gnosis-font-size: {font_size}%;
    --gnosis-line-height: {line_height};
    --gnosis-para-spacing: {para_spacing}em;
    --gnosis-margin: {margin}px;
    --gnosis-text-align: {text_align};
}}

html, body {{
    margin: 0 !important;
    padding: 0 !important;
    width: 100vw !important;
    height: 100vh !important;
    overflow: hidden !important;
    background: var(--gnosis-bg) !important;
    color: var(--gnosis-fg) !important;
    box-sizing: border-box !important;
    -webkit-user-select: text !important;
    user-select: text !important;
}}

#gnosis-viewport {{
    position: absolute !important;
    inset: 0 !important;
    width: 100vw !important;
    height: 100vh !important;
    overflow: hidden !important;
    box-sizing: border-box !important;
}}

#gnosis-content {{
    height: 100% !important;
    box-sizing: border-box !important;
    padding: 0 var(--gnosis-margin) !important;
    column-fill: auto !important;
    column-gap: var(--gnosis-gap, 48px) !important;
    column-width: var(--gnosis-col-width, calc(100vw - 2 * var(--gnosis-margin))) !important;
    transform: translateX(0px);
    will-change: transform;
    {font_family_rule}
    font-size: var(--gnosis-font-size) !important;
    line-height: var(--gnosis-line-height) !important;
    color: var(--gnosis-fg) !important;
}}

#gnosis-content.gnosis-animating {{
    transition: transform 180ms cubic-bezier(0.22, 1, 0.36, 1) !important;
}}

#gnosis-content * {{
    color: inherit !important;
    border-color: currentColor !important;
}}

#gnosis-content *:not(img):not(svg):not(canvas):not(video):not(audio) {{
    background-color: transparent !important;
}}

#gnosis-content a, #gnosis-content a:any-link {{
    color: var(--gnosis-link) !important;
    text-decoration: underline !important;
    cursor: pointer !important;
}}

#gnosis-content p {{
    margin-top: 0 !important;
    margin-bottom: var(--gnosis-para-spacing) !important;
    text-align: var(--gnosis-text-align) !important;
    hyphens: auto !important;
    -webkit-hyphens: auto !important;
}}

#gnosis-content div {{
    text-align: var(--gnosis-text-align) !important;
}}

#gnosis-content img, #gnosis-content svg, #gnosis-content video {{
    max-width: 100% !important;
    max-height: calc(100vh - 40px) !important;
    height: auto !important;
    object-fit: contain !important;
    break-inside: avoid !important;
    page-break-inside: avoid !important;
}}

#gnosis-content h1, #gnosis-content h2, #gnosis-content h3,
#gnosis-content h4, #gnosis-content h5, #gnosis-content h6 {{
    break-after: avoid !important;
    page-break-after: avoid !important;
}}

#gnosis-content table, #gnosis-content figure, #gnosis-content blockquote {{
    max-width: 100% !important;
    box-sizing: border-box !important;
    break-inside: avoid !important;
    page-break-inside: avoid !important;
}}
"#
    )
}

fn build_injected_script(chapter_index: usize, total_chapters: usize) -> String {
    format!(
        r#"
(function() {{
    const viewport = document.getElementById('gnosis-viewport');
    const content = document.getElementById('gnosis-content');
    if (!viewport || !content) return;

    let currentPage = 0;
    let totalPages = 1;
    let stride = window.innerWidth;
    let isSpread = false;
    let dragOffset = 0;
    const chapterIndex = {chapter_index};
    const totalChapters = {total_chapters};

    function recalculateLayout() {{
        const width = window.innerWidth;
        const height = window.innerHeight;
        const style = getComputedStyle(document.documentElement);
        let margin = parseFloat(style.getPropertyValue('--gnosis-margin')) || 48;

        // Auto 2-column spread on screens wider than 1000px
        isSpread = width >= 1000;
        const gap = margin;

        let colWidth;
        if (isSpread) {{
            colWidth = Math.floor((width - (2 * margin) - gap) / 2);
        }} else {{
            colWidth = Math.floor(width - (2 * margin));
        }}

        content.style.setProperty('--gnosis-col-width', `${{colWidth}}px`);
        content.style.setProperty('--gnosis-gap', `${{gap}}px`);

        stride = width;
        const scrollW = content.scrollWidth;
        totalPages = Math.max(1, Math.ceil(scrollW / stride));

        if (currentPage >= totalPages) {{
            currentPage = totalPages - 1;
        }}
        applyPage(false);
        reportRelocate();
    }}

    function applyPage(animated) {{
        if (animated) {{
            content.classList.add('gnosis-animating');
        }} else {{
            content.classList.remove('gnosis-animating');
        }}
        const offset = currentPage * stride;
        content.style.transform = `translateX(-${{offset}}px)`;
        dragOffset = 0;
    }}

    function setPage(p, animated) {{
        const target = Math.max(0, Math.min(totalPages - 1, p));
        if (target !== currentPage || dragOffset !== 0) {{
            currentPage = target;
            applyPage(animated);
            reportRelocate();
        }}
    }}

    function reportRelocate() {{
        const fraction = totalChapters > 0
            ? Math.min(1.0, (chapterIndex + (currentPage / totalPages)) / totalChapters)
            : 0.0;
        const locator = JSON.stringify({{ chapter: chapterIndex, page: currentPage }});

        if (window.parent && window.parent !== window && window.parent.gnosisOnRelocate) {{
            window.parent.gnosisOnRelocate({{
                chapter: chapterIndex,
                totalChapters: totalChapters,
                page: currentPage,
                totalPages: totalPages,
                fraction: fraction,
                locator: locator,
                isSpread: isSpread,
            }});
        }}
    }}

    window.gnosisNext = function() {{
        if (currentPage + 1 < totalPages) {{
            setPage(currentPage + 1, true);
            return true;
        }}
        return false;
    }};

    window.gnosisPrev = function() {{
        if (currentPage > 0) {{
            setPage(currentPage - 1, true);
            return true;
        }}
        return false;
    }};

    window.gnosisGoToPage = function(p, animated) {{
        setPage(p, !!animated);
    }};

    window.gnosisGoToAnchor = function(anchor) {{
        if (!anchor) return;
        const el = document.getElementById(anchor)
            || document.querySelector(`[name="${{anchor}}"]`);
        if (el) {{
            const left = el.getBoundingClientRect().left - content.getBoundingClientRect().left;
            const targetPage = Math.floor(left / stride);
            setPage(targetPage, false);
        }}
    }};

    window.gnosisScrollBy = function(dx, dy) {{
        content.classList.remove('gnosis-animating');
        dragOffset += dx;
        const baseOffset = currentPage * stride;
        const newOffset = Math.max(0, Math.min((totalPages - 1) * stride, baseOffset + dragOffset));
        content.style.transform = `translateX(-${{newOffset}}px)`;
    }};

    window.gnosisSnap = function(vx, vy) {{
        const threshold = stride * 0.2;
        let target = currentPage;
        if (dragOffset > threshold || vx > 0.3) {{
            target = currentPage + 1;
        }} else if (dragOffset < -threshold || vx < -0.3) {{
            target = currentPage - 1;
        }}

        if (target < 0) {{
            if (window.parent && window.parent.gnosisPrevChapter) {{
                window.parent.gnosisPrevChapter();
                return;
            }}
            target = 0;
        }} else if (target >= totalPages) {{
            if (window.parent && window.parent.gnosisNextChapter) {{
                window.parent.gnosisNextChapter();
                return;
            }}
            target = totalPages - 1;
        }}

        setPage(target, true);
    }};

    window.gnosisResize = function() {{
        recalculateLayout();
    }};

    window.gnosisGetStatus = function() {{
        return {{
            chapter: chapterIndex,
            totalChapters: totalChapters,
            page: currentPage,
            totalPages: totalPages,
            fraction: (chapterIndex + (currentPage / totalPages)) / totalChapters,
            isSpread: isSpread,
        }};
    }};

    // Handle clicks: margins turn page, links navigate
    viewport.addEventListener('click', function(e) {{
        const link = e.target.closest('a');
        if (link) {{
            e.preventDefault();
            const href = link.getAttribute('href');
            if (href) {{
                if (href.startsWith('#')) {{
                    window.gnosisGoToAnchor(href.slice(1));
                }} else if (/^[a-z]+:\/\//i.test(href)) {{
                    if (window.parent && window.parent.gnosisOpenExternal) {{
                        window.parent.gnosisOpenExternal(href);
                    }}
                }} else {{
                    if (window.parent && window.parent.gnosisNavigate) {{
                        window.parent.gnosisNavigate(href);
                    }}
                }}
            }}
            return;
        }}

        const x = e.clientX;
        const width = window.innerWidth;
        const marginZone = Math.max(48, width * 0.15);
        if (x < marginZone) {{
            if (!window.gnosisPrev()) {{
                if (window.parent && window.parent.gnosisPrevChapter) window.parent.gnosisPrevChapter();
            }}
        }} else if (x > width - marginZone) {{
            if (!window.gnosisNext()) {{
                if (window.parent && window.parent.gnosisNextChapter) window.parent.gnosisNextChapter();
            }}
        }}
    }});

    // Forward keypresses to parent window
    window.addEventListener('keydown', function(e) {{
        if (window.parent && window.parent !== window) {{
            const eventInit = {{
                key: e.key,
                code: e.code,
                keyCode: e.keyCode,
                ctrlKey: e.ctrlKey,
                shiftKey: e.shiftKey,
                altKey: e.altKey,
                metaKey: e.metaKey,
                bubbles: true
            }};
            window.parent.dispatchEvent(new KeyboardEvent('keydown', eventInit));
        }}
    }});

    window.addEventListener('resize', recalculateLayout);

    // Initial measurement after fonts and images load
    if (document.readyState === 'complete') {{
        recalculateLayout();
    }} else {{
        window.addEventListener('load', recalculateLayout);
    }}
}})();
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_html() {
        let raw = r#"<!DOCTYPE html>
<html>
<head>
    <title>Chapter 1</title>
</head>
<body>
    <h1>Chapter 1</h1>
    <p>Hello world, this is a test.</p>
</body>
</html>"#;
        let prefs = ReaderPrefs::default();
        let injected = inject_html(raw, "gnosis-reader:///book/123/", &prefs, 0, 5);

        assert!(injected.contains("<base href=\"gnosis-reader:///book/123/\" />"));
        assert!(injected.contains("<style id=\"gnosis-reader-style\">"));
        assert!(injected.contains("<div id=\"gnosis-viewport\"><div id=\"gnosis-content\">"));
        assert!(injected.contains("<script id=\"gnosis-chapter-script\">"));
        assert!(injected.contains("<h1>Chapter 1</h1>"));
        assert!(injected.contains("<p>Hello world, this is a test.</p>"));
    }

    #[test]
    fn test_strip_html_tags() {
        let html = "<p>Hello <b>world</b> &amp; everyone!</p><style>p { color: red; }</style>";
        let stripped = strip_html_tags(html);
        assert!(stripped.contains("Hello"));
        assert!(stripped.contains("world"));
        assert!(stripped.contains("& everyone!"));
        assert!(!stripped.contains("color: red"));
    }

    #[test]
    fn test_epub_reader_metamorphosis() {
        let path = Path::new(
            "/home/mavenried/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/epub-2.1.5/tests/docs/Metamorphosis-jackson.epub",
        );
        if !path.exists() {
            return;
        }

        let id = Uuid::new_v4();
        let mut reader = EpubReader::open(id, path).expect("failed to open epub");

        assert_eq!(reader.title, "Metamorphosis");
        assert_eq!(reader.total_chapters(), 8);
        assert!(!reader.toc.is_empty());

        let (injected, mime) = reader
            .get_injected_chapter(5, &ReaderPrefs::default())
            .expect("failed to get injected chapter 5");
        assert_eq!(mime, "application/xhtml+xml");
        assert!(injected.contains("<div id=\"gnosis-viewport\">"));
        assert!(injected.contains("Gregor Samsa"));

        let search_results = reader.search("Gregor", None);
        assert!(!search_results.is_empty());
        let first = &search_results[0];
        assert_eq!(first.matched.to_lowercase(), "gregor");
        assert!(!first.pre.is_empty() || !first.post.is_empty());
    }
}
