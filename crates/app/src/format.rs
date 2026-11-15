use doc_model::Document;
use l10n::tr;

use crate::model::DocModel;

pub struct Loaded {
    pub doc: Document,
    pub layer: bool,
    /// What the model can't hold (detached groups, unknown fields), written back on save.
    pub kept: doc_format::Preserved,
    pub warnings: Vec<String>,
}

pub fn load(text: &str) -> Loaded {
    loaded(doc_format::parse(text))
}

/// `file` is relative to the project; its cuts from the project's cuts file are planted back.
pub fn load_chapter(text: &str, file: &str, cuts: &[doc_format::ExternalCut]) -> Loaded {
    let mut l = loaded(doc_format::parse_page(text, file, cuts));
    l.doc.set_cut_source(Some(file.to_string()));
    l
}

/// The cuts of no file make the document; each chapter's come back to plant when it loads.
pub fn load_cuts(text: &str) -> (Loaded, Vec<doc_format::ExternalCut>) {
    let (l, cuts) = doc_format::parse_cuts(text);
    (loaded(l), cuts)
}

pub fn serialize_cuts(m: &DocModel, chapters: &[doc_format::ExternalCut]) -> String {
    doc_format::serialize_cuts(&m.doc, m.layer, &m.kept, chapters)
}

fn loaded(l: doc_format::LoadedDoc) -> Loaded {
    let mut warnings = l.warnings;
    if let Some(v) = l.newer_version {
        warnings.insert(
            0,
            tr!("This file was saved by a newer Dostoevsky (format v{v}). Its alternatives are kept but not shown.", v = v),
        );
    }
    Loaded {
        doc: l.doc,
        layer: l.layer,
        kept: l.kept,
        warnings,
    }
}

/// The whole document, a chapter's cuts included.
pub fn serialize(m: &DocModel) -> String {
    doc_format::serialize(&m.doc, m.layer, &m.kept)
}

/// A chapter goes without the cuts its project keeps; those are returned for the cuts file.
pub fn serialize_for_disk(m: &DocModel) -> (String, Option<(String, Vec<doc_format::ExternalCut>)>) {
    match m.doc.cut_source() {
        Some(file) => {
            let (text, cuts) = doc_format::serialize_page(&m.doc, m.layer, &m.kept, file);
            (text, Some((file.to_string(), cuts)))
        }
        None => (serialize(m), None),
    }
}
