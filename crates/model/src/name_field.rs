//! A one-line text field for a new file name, as Explorer's rename box behaves: it opens with
//! the name before the extension selected, typing replaces the selection, and the arrows,
//! Home and End move the caret (with Shift, they stretch the selection).

use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameField {
    text: String,
    /// Byte offsets, always on a char boundary. The selection runs between them.
    caret: usize,
    anchor: usize,
}

impl NameField {
    /// `name` with its stem selected: everything before the last dot, unless the dot leads
    /// (`.gitignore`) or it is a folder, which selects the whole name.
    pub fn new(name: &str, folder: bool) -> Self {
        let end = match name.rfind('.') {
            Some(dot) if dot > 0 && !folder => dot,
            _ => name.len(),
        };
        Self {
            text: name.to_owned(),
            caret: end,
            anchor: 0,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn selection(&self) -> Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Type `typed` over the selection. Line breaks and other control characters are dropped,
    /// so a pasted line lands as one name.
    pub fn insert(&mut self, typed: &str) {
        let clean: String = typed.chars().filter(|c| !c.is_control()).collect();
        let range = self.selection();
        self.text.replace_range(range.clone(), &clean);
        self.caret = range.start + clean.len();
        self.anchor = self.caret;
    }

    pub fn backspace(&mut self) {
        if self.anchor == self.caret {
            self.anchor = self.before(self.caret);
        }
        self.insert("");
    }

    pub fn delete(&mut self) {
        if self.anchor == self.caret {
            self.anchor = self.after(self.caret);
        }
        self.insert("");
    }

    /// Left: with a selection and no Shift, the caret lands at its start.
    pub fn left(&mut self, extend: bool) {
        let to = match (extend, self.selection()) {
            (false, range) if !range.is_empty() => range.start,
            _ => self.before(self.caret),
        };
        self.move_to(to, extend);
    }

    pub fn right(&mut self, extend: bool) {
        let to = match (extend, self.selection()) {
            (false, range) if !range.is_empty() => range.end,
            _ => self.after(self.caret),
        };
        self.move_to(to, extend);
    }

    pub fn home(&mut self, extend: bool) {
        self.move_to(0, extend);
    }

    pub fn end(&mut self, extend: bool) {
        self.move_to(self.text.len(), extend);
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
    }

    fn move_to(&mut self, to: usize, extend: bool) {
        self.caret = to;
        if !extend {
            self.anchor = to;
        }
    }

    fn before(&self, at: usize) -> usize {
        self.text[..at].char_indices().last().map_or(0, |(i, _)| i)
    }

    fn after(&self, at: usize) -> usize {
        self.text[at..]
            .chars()
            .next()
            .map_or(at, |c| at + c.len_utf8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected(field: &NameField) -> &str {
        &field.text()[field.selection()]
    }

    #[test]
    fn opens_with_the_stem_selected() {
        assert_eq!(
            selected(&NameField::new("trip.photo.jpg", false)),
            "trip.photo"
        );
        assert_eq!(selected(&NameField::new("README", false)), "README");
        assert_eq!(selected(&NameField::new(".gitignore", false)), ".gitignore");
        assert_eq!(selected(&NameField::new("v1.2", true)), "v1.2");
    }

    #[test]
    fn typing_replaces_the_stem_and_keeps_the_extension() {
        let mut field = NameField::new("IMG_0042.HEIC", false);
        field.insert("Beach");
        assert_eq!(field.text(), "Beach.HEIC");
        assert_eq!(field.caret(), "Beach".len());
        field.insert(" day\r\n");
        assert_eq!(field.text(), "Beach day.HEIC");
    }

    #[test]
    fn backspace_and_delete_take_a_whole_character() {
        let mut field = NameField::new("café.txt", false);
        field.right(false);
        field.backspace();
        assert_eq!(field.text(), "caf.txt");
        field.delete();
        assert_eq!(field.text(), "caftxt");
        field.home(false);
        field.backspace();
        assert_eq!(field.text(), "caftxt");
        field.end(false);
        field.delete();
        assert_eq!(field.text(), "caftxt");
    }

    #[test]
    fn arrows_collapse_a_selection_then_move() {
        let mut field = NameField::new("ab.png", false);
        field.left(false);
        assert_eq!((field.caret(), selected(&field)), (0, ""));
        field.right(false);
        field.right(true);
        field.right(true);
        assert_eq!(selected(&field), "b.");
        field.right(false);
        assert_eq!((field.caret(), selected(&field)), (3, ""));
        field.home(true);
        assert_eq!(selected(&field), "ab.");
        field.select_all();
        field.insert("x");
        assert_eq!(field.text(), "x");
    }
}
