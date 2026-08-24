use std::io::{BufReader, Cursor, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use epub::doc::EpubDoc;
use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::writer::Writer;
use zip::ZipArchive;
use zip::write::SimpleFileOptions;

pub struct MetadataUpdate<'a> {
    pub title: &'a str,
    pub author: Option<&'a str>,
    pub series: Option<&'a str>,
    pub series_index: Option<f64>,
}

/// Writes a copy of `source` with `update` baked into its OPF metadata,
/// placed alongside the original file, which is left untouched. Returns the
/// new file's path.
pub fn write_copy_with_metadata(source: &Path, update: &MetadataUpdate) -> Result<PathBuf> {
    let (opf_zip_name, new_opf) = updated_opf(source, update)?;
    let dest = destination_path(source);
    copy_zip_with_replacement(source, &dest, &opf_zip_name, &new_opf)?;
    Ok(dest)
}

/// Overwrites `source` in place with `update` baked into its OPF metadata.
/// Writes to a temporary file alongside `source` first and only replaces it
/// with an atomic rename once the rewrite has fully succeeded, so a failure
/// partway through never leaves `source` corrupted.
pub fn write_metadata_in_place(source: &Path, update: &MetadataUpdate) -> Result<()> {
    let (opf_zip_name, new_opf) = updated_opf(source, update)?;

    let tmp_dest = source.with_extension(format!(
        "{}.gnosis-tmp",
        source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("epub")
    ));

    let result = copy_zip_with_replacement(source, &tmp_dest, &opf_zip_name, &new_opf);
    match result {
        Ok(()) => std::fs::rename(&tmp_dest, source).context("replacing original epub file"),
        Err(err) => {
            std::fs::remove_file(&tmp_dest).ok();
            Err(err)
        }
    }
}

/// Reads `source`'s OPF file and returns `(zip entry name, rewritten OPF
/// bytes)` with `update` applied.
fn updated_opf(source: &Path, update: &MetadataUpdate) -> Result<(String, Vec<u8>)> {
    let mut doc = EpubDoc::new(source)
        .map_err(|e| anyhow!("{e}"))
        .with_context(|| format!("opening epub {}", source.display()))?;

    let opf_path = doc.root_file.clone();
    let opf_bytes = doc
        .get_resource_by_path(&opf_path)
        .ok_or_else(|| anyhow!("epub is missing its OPF file"))?;
    let opf_text = String::from_utf8(opf_bytes).context("OPF file is not valid UTF-8")?;

    let new_opf = rewrite_opf(&opf_text, update)?;
    let opf_zip_name = opf_path.to_string_lossy().replace('\\', "/");

    Ok((opf_zip_name, new_opf.into_bytes()))
}

fn destination_path(source: &Path) -> PathBuf {
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "book".to_string());
    let ext = source
        .extension()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "epub".to_string());

    let mut candidate = parent.join(format!("{stem} (edited).{ext}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = parent.join(format!("{stem} (edited {n}).{ext}"));
        n += 1;
    }
    candidate
}

/// Copies every entry of the `source` zip archive into `dest` unchanged,
/// except for `replace_name`, whose contents become `replacement`.
fn copy_zip_with_replacement(
    source: &Path,
    dest: &Path,
    replace_name: &str,
    replacement: &[u8],
) -> Result<()> {
    let file = std::fs::File::open(source).context("opening source epub")?;
    let mut archive =
        ZipArchive::new(BufReader::new(file)).context("reading epub as a zip archive")?;

    let out = std::fs::File::create(dest).context("creating new epub file")?;
    let mut writer = zip::ZipWriter::new(out);

    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).context("reading zip entry")?;
        let name = entry.name().to_string();
        if name == replace_name {
            writer
                .start_file(&name, SimpleFileOptions::default())
                .context("writing updated OPF entry")?;
            writer.write_all(replacement)?;
        } else {
            writer.raw_copy_file(entry).context("copying zip entry")?;
        }
    }

    writer.finish().context("finalizing epub archive")?;
    Ok(())
}

fn rewrite_opf(xml: &str, update: &MetadataUpdate) -> Result<String> {
    let mut reader = Reader::from_str(xml);

    let mut events: Vec<Event<'static>> = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(event) => events.push(event.into_owned()),
            Err(err) => bail!("failed to parse OPF XML: {err}"),
        }
    }

    set_text_element(&mut events, "title", Some(update.title));
    set_text_element(&mut events, "creator", update.author);
    set_meta_tag(&mut events, "calibre:series", update.series);
    set_meta_tag(
        &mut events,
        "calibre:series_index",
        update.series_index.map(|n| n.to_string()).as_deref(),
    );

    let mut writer = Writer::new(Cursor::new(Vec::new()));
    for event in events {
        writer.write_event(event).context("writing OPF XML")?;
    }
    String::from_utf8(writer.into_inner().into_inner()).context("generated OPF is not valid UTF-8")
}

fn metadata_bounds(events: &[Event<'static>]) -> Option<(usize, usize)> {
    let start = events.iter().position(
        |event| matches!(event, Event::Start(e) if e.local_name().as_ref() == "metadata"),
    )?;
    let end = events[start..].iter().position(
        |event| matches!(event, Event::End(e) if e.local_name().as_ref() == "metadata"),
    )?;
    Some((start, start + end))
}

/// Sets the text content of the first `<dc:{local_name}>` element inside
/// `<metadata>`, inserting a new element before `</metadata>` if none exists.
/// Does nothing if `value` is `None`.
fn set_text_element(events: &mut Vec<Event<'static>>, local_name: &str, value: Option<&str>) {
    let Some(value) = value else { return };
    let Some((meta_start, meta_end)) = metadata_bounds(events) else {
        return;
    };

    let start_index = events[meta_start..=meta_end]
        .iter()
        .position(|event| matches!(event, Event::Start(e) if e.local_name().as_ref() == local_name))
        .map(|rel| meta_start + rel);

    match start_index {
        Some(i) => {
            let text = BytesText::new(value).into_owned();
            if matches!(events.get(i + 1), Some(Event::Text(_))) {
                events[i + 1] = Event::Text(text);
            } else {
                events.insert(i + 1, Event::Text(text));
            }
        }
        None => {
            let tag = format!("dc:{local_name}");
            let start = Event::Start(BytesStart::new(tag.clone())).into_owned();
            let text = Event::Text(BytesText::new(value).into_owned());
            let end = Event::End(BytesEnd::new(tag)).into_owned();
            events.splice(meta_end..meta_end, [start, text, end]);
        }
    }
}

/// Sets `<meta name="{meta_name}" content="{value}"/>` inside `<metadata>`,
/// inserting it before `</metadata>` if it doesn't already exist. Does
/// nothing if `value` is `None`.
fn set_meta_tag(events: &mut Vec<Event<'static>>, meta_name: &str, value: Option<&str>) {
    let Some(value) = value else { return };
    let Some((meta_start, meta_end)) = metadata_bounds(events) else {
        return;
    };

    let existing = events[meta_start..=meta_end].iter().position(|event| {
        matches!(event, Event::Empty(e) if e.local_name().as_ref() == "meta" && attr_equals(e, "name", meta_name))
    });

    let mut tag = BytesStart::new("meta");
    tag.push_attribute(("name", meta_name));
    tag.push_attribute(("content", escape(value).as_ref()));
    let event = Event::Empty(tag).into_owned();

    match existing {
        Some(rel) => events[meta_start + rel] = event,
        None => events.insert(meta_end, event),
    }
}

fn attr_equals(start: &BytesStart, attr_name: &str, value: &str) -> bool {
    start
        .try_get_attribute(attr_name)
        .ok()
        .flatten()
        .is_some_and(|a| a.value.as_ref() == value)
}
