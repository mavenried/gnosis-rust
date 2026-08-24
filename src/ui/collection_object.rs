use std::collections::HashMap;
use std::path::PathBuf;

use gtk::glib;
use gtk::subclass::prelude::*;

use crate::library::Book;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionKind {
    Author,
    Series,
}

impl CollectionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CollectionKind::Author => "author",
            CollectionKind::Series => "series",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CollectionData {
    pub kind: CollectionKind,
    pub name: String,
    pub cover_path: Option<PathBuf>,
    pub book_count: u32,
}

/// Groups `books` by author or series (case-insensitively deduped — first-
/// seen casing wins for display), counting books per group and resolving
/// each group's cover image: an explicit `custom_covers` entry (keyed by
/// the display name — see `library::db::all_collection_covers`), else the
/// first book in that group with a cover. Results are sorted by display
/// name, case-insensitive.
pub fn group_books(
    books: &[Book],
    kind: CollectionKind,
    custom_covers: &HashMap<String, PathBuf>,
) -> Vec<CollectionData> {
    let mut display_names: HashMap<String, String> = HashMap::new();
    let mut counts: HashMap<String, u32> = HashMap::new();
    let mut fallback_covers: HashMap<String, PathBuf> = HashMap::new();

    for book in books {
        let name = match kind {
            CollectionKind::Author => book.author.clone(),
            CollectionKind::Series => book.series.clone(),
        };
        let Some(name) = name.filter(|n| !n.trim().is_empty()) else {
            continue;
        };

        let key = name.to_lowercase();
        display_names.entry(key.clone()).or_insert(name);
        *counts.entry(key.clone()).or_insert(0) += 1;
        if let Some(cover) = &book.cover_path {
            fallback_covers.entry(key).or_insert_with(|| cover.clone());
        }
    }

    let mut keys: Vec<&String> = display_names.keys().collect();
    keys.sort_by_key(|key| display_names[*key].to_lowercase());

    keys.into_iter()
        .map(|key| {
            let name = display_names[key].clone();
            let cover_path = custom_covers
                .get(&name)
                .cloned()
                .or_else(|| fallback_covers.get(key).cloned());
            CollectionData {
                kind,
                book_count: counts[key],
                name,
                cover_path,
            }
        })
        .collect()
}

mod imp {
    use std::cell::RefCell;

    use gtk::glib;
    use gtk::subclass::prelude::*;

    use super::CollectionData;

    #[derive(Default)]
    pub struct CollectionObject {
        pub data: RefCell<Option<CollectionData>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CollectionObject {
        const NAME: &'static str = "GnosisCollectionObject";
        type Type = super::CollectionObject;
    }

    impl ObjectImpl for CollectionObject {}
}

glib::wrapper! {
    pub struct CollectionObject(ObjectSubclass<imp::CollectionObject>);
}

impl CollectionObject {
    pub fn new(data: CollectionData) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().data.replace(Some(data));
        obj
    }

    pub fn data(&self) -> CollectionData {
        self.imp()
            .data
            .borrow()
            .clone()
            .expect("CollectionObject constructed without data")
    }
}
