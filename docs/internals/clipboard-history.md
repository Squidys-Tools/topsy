# Clipboard history

Decisions behind [clipboard history](../user/clipboard-history.md).

- **The palette takes focus**, unlike the island: it is typed into. It remembers the window that was in front when
  the hotkey fired and hands focus back to it on Esc and after a paste. Paste puts the entry on the clipboard,
  waits up to 500 ms for that window to be in front again, and only then sends Ctrl+V: keys sent sooner land
  wherever the focus is. Being a copy, a paste moves the entry to the front.
- **Its actions are a panel drawn in the palette, not a Win32 menu.** A popup menu takes the foreground, which
  closes a palette that closes on losing it, and is drawn light on the dark glass.
- **It is a notch of its own, not the island** (`frame::open_window` with `Shape::Hanging`): the same frame,
  hung from the top edge of the monitor under the cursor with the notch's ears, fill and rim, and sprung open and
  folded with the island's spring. It isn't the island because it takes the focus, which the island never may.
  Its content is laid out at full height and revealed as the notch grows, so nothing reflows while it springs.
  Esc folds it before the window goes; a paste or losing the focus closes it at once.
- **A preview is cut by topsy, not by GPUI's `line_clamp`.** `line_clamp` draws no ellipsis and counts only
  soft wraps, so a text of short lines is not clamped at all and a line is cut through; adding `text_ellipsis`
  truncates the whole string to `width × lines` before it wraps, which lands past the last visible line
  (wrapping wastes the end of each line). So `clipboard::preview` shapes the text at the preview's width, finds
  where line N ends and cuts it there with `…` (`model::clamp`). The element must keep the width and size it was
  measured at, which `Face` carries. A single line (a row's title, a fact) is cut with `line_clamp(1)` and
  `text_ellipsis` at a set width, which is exact for one line.
- **What Windows must look up is looked up off the UI thread, once** (`clipboard::pictures`): app icons, image
  and file thumbnails at each size they are drawn, site icons, and the names apps give themselves (their exe's
  file description, which needs COM). An image is loaded at the preview's size only once it is selected, so a
  list of screenshots costs a row-sized thumbnail each.
- **Nothing polls.** The listener (`AddClipboardFormatListener`) wakes on each copy; the store thread wakes on a
  command or when the oldest entry is due to expire. Copies marked private
  (`ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` = 0) are never read. Many apps
  announce one copy twice, so the same thing copied again within 2 s is one copy.
- **On disk** each entry is `<id>.json` in the history folder, an image's pixels `<id>.png` beside it, each
  written through a temp file. A kill between the two can leave a `.tmp` or an image with no entry; the store
  removes those (and a half-written site icon) when it starts. At most 2000 unpinned entries are kept, and text
  over 1 MB is not. One topsy keeps a folder; a second one started on it keeps no history, since both would
  number and expire the same files.
- **It stays in front of the island.** It hangs where the island does, and the island raises itself over every
  topmost window whenever it appears (see [Notch](notch.md)). While the palette is open, every frame that
  appears is raised to just under it instead (`frame::keep_on_top`), which covers the edge reveal, HUDs and
  activities alike.
- **A link's preview reads the page itself, so it is asked for only when the link is selected**, and only for a
  link that is safe to open unasked (`model::link_preview::fetchable`): a public https name, and no query, user
  name or token-like path segment, since some links act when opened (a one-time sign-in, a password reset). Its
  Open Graph title, description and picture are kept in `previews\<hash>.json` and `.png`, the picture cut to
  the preview's shape before it is drawn. Whenever the history changes, previews of links no longer in it are
  removed: they hold the pages' titles.
- **Site icons and link previews are the only network traffic.** A link that is listed asks its site for `/favicon.ico`, else the
  icon its home page names, once per host, through WinHTTP (the system's TLS, no crate of our own). Only https
  and public names: never a bare name, an address, a local-network suffix, or a name that resolves to a private
  address, and a redirect or icon link is vetted the same way. The icon is kept as a small PNG in
  `icons\<host>.png` beside the history; an empty file there means the site has none, so it isn't asked again.
