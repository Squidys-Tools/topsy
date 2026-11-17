# Clipboard history

Decisions behind [clipboard history](../user/clipboard-history.md).

- **The strip takes focus**, unlike the island: it is typed into. It remembers the window that was in front when
  the hotkey fired and hands focus back to it on Esc and after a paste. Paste puts the entry on the clipboard,
  waits up to 500 ms for that window to be in front again, and only then sends Ctrl+V: keys sent sooner land
  wherever the focus is. Being a copy, a paste moves the entry to the front.
- **A cut preview is cut by topsy, not by GPUI's `line_clamp`.** `line_clamp` draws no ellipsis and counts only
  soft wraps, so a text of short lines is not clamped at all and a line is cut through; adding `text_ellipsis`
  truncates the whole string to `width × lines` before it wraps, which lands past the last visible line
  (wrapping wastes the end of each line). So `clipboard::preview` shapes the text at the card's width, finds
  where line N ends and cuts it there with `…` (`model::clamp`). The element must keep the width and size it was
  measured at, which `Face` carries.
- **Nothing polls.** The listener (`AddClipboardFormatListener`) wakes on each copy; the store thread wakes on a
  command or when the oldest entry is due to expire. Copies marked private
  (`ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` = 0) are never read. Many apps
  announce one copy twice, so the same thing copied again within 2 s is one copy.
- **On disk** each entry is `<id>.json` in the history folder, an image's pixels `<id>.png` beside it, each
  written through a temp file. A kill between the two can leave a `.tmp` or an image with no entry; the store
  removes those (and a half-written site icon) when it starts. At most 2000 unpinned entries are kept, and text
  over 1 MB is not. One topsy keeps a folder; a second one started on it keeps no history, since both would
  number and expire the same files.
- **Site icons are the only network traffic.** A link card that is shown asks its site for `/favicon.ico`, else
  the icon its home page names, once per host, through WinHTTP (the system's TLS, no crate of our own). Only
  https and public names: never a bare name, an address, a local-network suffix, or a name that resolves to a
  private address, and a redirect or icon link is vetted the same way. The icon is kept as a small PNG in
  `icons\<host>.png` beside the history; an empty file there means the site has none, so it isn't asked again.
