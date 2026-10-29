//! Automatic numbering (ComicRack "Autonumber Wizard").

use crate::model::Book;
use crate::util::parse_number;

#[derive(Clone, Debug, PartialEq)]
pub struct AutoNumberOptions {
    /// Number the alternate series instead of the series.
    pub alternate: bool,
    pub start: i32,
    /// Also store the total number of books (Count / AlternateCount).
    pub set_count: bool,
    pub count: i32,
}

impl AutoNumberOptions {
    /// Defaults like the script: start at the first book's number, total = start + n - 1.
    pub fn for_books(books: &[Book], alternate: bool) -> Self {
        let first = books.first().map(|b| {
            if alternate { b.info.alternate_number.clone() } else { b.info.number.clone() }
        });
        let start = first
            .as_deref()
            .and_then(parse_number)
            .map(|n| n.trunc() as i32)
            .unwrap_or(1);
        AutoNumberOptions {
            alternate,
            start,
            set_count: false,
            count: start + books.len() as i32 - 1,
        }
    }
}

/// Numbers the books in the given (display) order.
pub fn apply(books: &mut [Book], opts: &AutoNumberOptions) {
    for (i, b) in books.iter_mut().enumerate() {
        let n = (opts.start + i as i32).to_string();
        if opts.alternate {
            b.info.alternate_number = n;
            if opts.set_count {
                b.info.alternate_count = opts.count;
            }
        } else {
            b.info.number = n;
            if opts.set_count {
                b.info.count = opts.count;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering() {
        let mut books = vec![Book::default(), Book::default(), Book::default()];
        books[0].info.number = "4".into();
        let mut o = AutoNumberOptions::for_books(&books, false);
        assert_eq!((o.start, o.count), (4, 6));
        o.set_count = true;
        apply(&mut books, &o);
        assert_eq!(books[2].info.number, "6");
        assert_eq!(books[1].info.count, 6);
    }
}
