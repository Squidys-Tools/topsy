//! The tray icon's menu: where it and its submenu open, and how the keyboard moves through it.
//! Physical screen pixels; no OS calls.

use crate::float::Rect;

/// The next row the arrow keys land on from `from`, `down` or up, skipping rows that can't be
/// chosen and wrapping round. With nothing highlighted, down starts at the top and up at the
/// bottom. `None` if no row can be chosen.
pub fn step(choosable: &[bool], from: Option<usize>, down: bool) -> Option<usize> {
    let len = choosable.len();
    let start = match (from, down) {
        (Some(at), _) => at.min(len.saturating_sub(1)),
        (None, true) => len.checked_sub(1)?,
        (None, false) => 0,
    };
    (1..=len)
        .map(|offset| {
            if down {
                (start + offset) % len
            } else {
                (start + len - offset % len) % len
            }
        })
        .find(|&at| choosable[at])
}

/// The row whose access key (the letter after `&` in its label) is `key`, ignoring case.
pub fn by_access_key<'a>(labels: impl IntoIterator<Item = &'a str>, key: char) -> Option<usize> {
    let key = key.to_lowercase().next()?;
    labels
        .into_iter()
        .position(|label| access_key(label).and_then(|c| c.to_lowercase().next()) == Some(key))
}

/// The letter after the first `&`.
pub fn access_key(label: &str) -> Option<char> {
    let (_, after) = label.split_once('&')?;
    after.chars().next()
}

/// The label as shown, without its `&`.
pub fn shown(label: &str) -> String {
    label.replacen('&', "", 1)
}

/// The top-left of a menu of `size` opened at `at`: below and to the right of it, as a context
/// menu opens, flipped to the other side where that would leave `work` (the monitor less its
/// taskbar, so a menu from the tray icon rises above the taskbar), and kept inside it.
pub fn place(at: (i32, i32), size: (i32, i32), work: Rect) -> (i32, i32) {
    let axis = |at: i32, len: i32, start: i32, end: i32| {
        let origin = if at + len <= end { at } else { at - len };
        origin.clamp(start, (end - len).max(start))
    };
    (
        axis(at.0, size.0, work.left, work.right),
        axis(at.1, size.1, work.top, work.bottom),
    )
}

/// The top-left of a submenu of `size` beside `parent`, its first row level with the row that
/// opened it (`row_top`, less the submenu's own padding `inset`): to the right where it fits,
/// else to the left, and kept inside `work`.
pub fn beside(parent: Rect, row_top: i32, inset: i32, size: (i32, i32), work: Rect) -> (i32, i32) {
    let x = if parent.right + size.0 <= work.right {
        parent.right
    } else {
        parent.left - size.0
    };
    let y = row_top - inset;
    (
        x.clamp(work.left, (work.right - size.0).max(work.left)),
        y.clamp(work.top, (work.bottom - size.1).max(work.top)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1032,
    };

    #[test]
    fn arrows_skip_what_cannot_be_chosen_and_wrap() {
        let rows = [true, false, true, true, false];
        assert_eq!(step(&rows, None, true), Some(0));
        assert_eq!(step(&rows, None, false), Some(3));
        assert_eq!(step(&rows, Some(0), true), Some(2));
        assert_eq!(step(&rows, Some(3), true), Some(0));
        assert_eq!(step(&rows, Some(0), false), Some(3));
        assert_eq!(step(&rows, Some(2), false), Some(0));
        assert_eq!(step(&[false, false], None, true), None);
        assert_eq!(step(&[], None, true), None);
        assert_eq!(step(&[true], Some(0), true), Some(0));
    }

    #[test]
    fn access_keys_ignore_case_and_the_ampersand_is_hidden() {
        let labels = ["&Show shelf", "Show bas&ket", "Se&ttings…", "No key"];
        assert_eq!(by_access_key(labels, 'K'), Some(1));
        assert_eq!(by_access_key(labels, 's'), Some(0));
        assert_eq!(by_access_key(labels, 't'), Some(2));
        assert_eq!(by_access_key(labels, 'x'), None);
        assert_eq!(shown("Show bas&ket"), "Show basket");
    }

    #[test]
    fn a_menu_from_the_tray_icon_rises_above_the_taskbar_and_stays_on_screen() {
        // Right-clicked in the taskbar's corner, below the work area: up and to the left.
        let (x, y) = place((1900, 1050), (250, 300), WORK);
        assert_eq!((x, y), (1650, 732));
        // With room, it opens below and to the right of the click.
        assert_eq!(place((900, 40), (250, 300), WORK), (900, 40));
        // A monitor left of the primary keeps its negative coordinates.
        let left = Rect {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1032,
        };
        assert_eq!(place((-10, 1050), (250, 300), left), (-260, 732));
    }

    #[test]
    fn a_submenu_opens_beside_its_row_on_the_side_with_room() {
        let parent = Rect {
            left: 1650,
            top: 700,
            right: 1900,
            bottom: 1000,
        };
        assert_eq!(beside(parent, 800, 6, (200, 250), WORK), (1450, 782));
        let parent = Rect {
            left: 100,
            top: 100,
            right: 350,
            bottom: 400,
        };
        assert_eq!(beside(parent, 200, 6, (200, 250), WORK), (350, 194));
        // Low on the screen, it is lifted to stay whole.
        assert_eq!(beside(parent, 1000, 6, (200, 250), WORK).1, 782);
    }
}
