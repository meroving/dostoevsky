//! Writes every export of a sample book to `target/export-sample/`.

use export::*;

fn main() {
    let md1 = "On an exceptionally hot evening early in July a young man came out of the garret in which he lodged in S. Place and walked slowly, as though in hesitation, towards K. bridge.\n\nHe had *successfully* avoided meeting his landlady on the staircase. His garret was under the roof of a high, five-storied house and was more like a cupboard than a room.\n\n---\n\nThe landlady who provided him with garret, dinners, and attendance, lived on the floor below.\n";
    let b = md1.find("exceptionally hot").unwrap();
    let book = Book {
        title: "Crime and Punishment".into(),
        author: "Fyodor Dostoevsky".into(),
        language: Some("en".into()),
        chapters: vec![
            Chapter { part: Some("Part One".into()), file_title: "The garret".into(), markdown: md1.into(), notes: vec![Note { range: b..b + 17, others: vec!["stifling".into(), "close and airless".into()], comment: None }] },
            Chapter { file_title: "The tavern".into(), markdown: "# The tavern\n\nHe was not used to crowds, and, as we said before, he avoided society of every sort.\n\n> A quote from someone.\n\n- one\n- two\n".into(), ..Default::default() },
        ],
    };
    let dir = std::path::Path::new("target/export-sample");
    std::fs::create_dir_all(dir).unwrap();
    let mut ms = Options::new(Style::Manuscript, Text::manuscript(), Paper::Letter);
    ms.comments = true;
    ms.chapter_titles = ChapterTitles::FileNames;
    std::fs::write(dir.join("manuscript.docx"), docx::write(&book, &ms)).unwrap();
    let mut bk = Options::new(Style::Book, Text::book(Paper::A5), Paper::A5);
    bk.title_page = true;
    bk.chapter_titles = ChapterTitles::FileNames;
    std::fs::write(dir.join("book.docx"), docx::write(&book, &bk)).unwrap();
    std::fs::write(dir.join("book.epub"), epub::write(&book, &bk)).unwrap();
    std::fs::write(dir.join("book.html"), html::page(&book, &bk)).unwrap();
    std::fs::write(dir.join("book.md"), text::markdown(&book, ChapterTitles::FileNames)).unwrap();
    std::fs::write(dir.join("book.txt"), text::plain(&book, ChapterTitles::FileNames)).unwrap();
    println!("{}", dir.display());
}
