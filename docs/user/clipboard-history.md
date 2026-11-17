# Clipboard history

topsy keeps what you copy: text, links, colours, code, images and files, each with the app it came from. Press
**Ctrl+Alt+V** to open the **strip**, a black band along the bottom of the monitor under the cursor, with a card
per copy, newest first.

## Using it

- **Type** to search. **Left/Right** pick a card.
- **Enter** pastes into the window that was in front. **Ctrl+C** only copies.
- **Ctrl+P** pins a card, **Delete** removes it, **Tab** shows the pinned ones.
- **Esc** clears the search, then closes the strip.
- Right-click a card, or its ⋯ button, for the same actions.

Code cards are coloured, and link cards show the site's icon.

## Settings

History is kept 7 days by default. Choose 1, 7 or 30 days or forever in the settings window, whose *Clear
unpinned* empties it but for the pins; pinned entries never expire. Turn history off from the tray icon's menu
or the settings window (see [Tray icon and settings](settings.md)).

History is kept in `%LOCALAPPDATA%\topsy\clipboard`.

## Good to know

- Blank copies, text over 1 MB and copies marked private (as password managers mark them) are never kept.
  The same thing copied again within 2 seconds counts once.
- Unlike the notch, the strip takes the keyboard while it is open. It hands focus back to the window that was in
  front when you close it or paste.
- A paste waits for that window to come back to the front before sending Ctrl+V. If it never does, no keys are
  sent and the entry stays on the clipboard. A terminal that pastes on Ctrl+Shift+V ignores the Ctrl+V; paste
  there yourself.
- If another app holds Ctrl+Alt+V, the strip can't open, and the hotkey can't be changed.
- Rich text and HTML are kept as plain text. The strip shows the newest 100 matches.
- Link cards fetch the site's icon once, over https, from public sites only; that is the only network traffic
  history makes. *Clear unpinned* leaves the saved icons, which name the sites you copied from.
