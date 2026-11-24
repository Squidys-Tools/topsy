# Clipboard history

topsy keeps what you copy: text, links, colours, code, images and files, each with the app it came from. Press
**Ctrl+Alt+V** and the **palette** drops out of the top of the monitor under the cursor, like the notch: a search
field over a list of your copies, pinned ones first and the rest by day, with a preview of the selected one beside
it.

## Using it

- **Type** to search. **Up/Down** pick an entry.
- **Enter** (or a double-click) pastes into the window that was in front; the footer names it.
  **Ctrl+Enter** or **Ctrl+C** only copies.
- **Ctrl+P** pins an entry, **Delete** removes it, **Tab** shows only the pinned ones.
- **Ctrl+K**, the footer's *Actions* or a right-click opens the same actions in a panel; type to narrow it.
- **Esc** or the ✕ clears the search, then folds the palette away.

The preview shows the whole start of a text, code coloured, a link's site, a colour's swatch, the image or the
files, with the app and when it was copied in its tag and the size below it.

## Settings

History is kept 7 days by default. Choose 1, 7 or 30 days or forever under Clipboard on the settings page,
whose *Clear* empties it but for the pins; pinned entries never expire. Turn history off there or from the tray
icon's menu (see [Tray icon and settings](settings.md)).

History is kept in `%LOCALAPPDATA%\topsy\clipboard`.

## Good to know

- Blank copies, text over 1 MB and copies marked private (as password managers mark them) are never kept.
  The same thing copied again within 2 seconds counts once.
- Unlike the notch, the palette takes the keyboard while it is open. It hands focus back to the window that was
  in front when you close it or paste.
- A paste waits for that window to come back to the front before sending Ctrl+V. If it never does, no keys are
  sent and the entry stays on the clipboard. A terminal that pastes on Ctrl+Shift+V ignores the Ctrl+V; paste
  there yourself.
- If another app holds Ctrl+Alt+V, the palette can't open, and the hotkey can't be changed.
- Rich text and HTML are kept as plain text. The palette lists the newest 100 matches.
- Links show their site's icon, and a selected link shows its page's preview: its picture, title and
  description. Both are fetched once, over https, from public sites only; that is the only network traffic
  history makes. A link with a query (`?…`) or what looks like a key in it is never opened, as some links act
  when opened. *Clear unpinned* removes the previews but leaves the saved icons, which name the sites you
  copied from.
