# xterm.js inside Tauri 2's WKWebView

Research for issue #5 (part of the map, #1). Date: 2026-09-10. Vocabulary per `CONTEXT.md`: a
**Session** is one shell on its own pty; a **Tab** is its sidebar entry.

## Recommendation

Use `@xterm/xterm` 6.0.0 with the **WebGL renderer on the active Tab only**, the built-in DOM
renderer as the automatic fallback (on context loss, on load failure, and via a user setting), and
`allowTransparency: false`. Keep one `Terminal` instance alive per Session for the whole app
lifetime (buffer, modes, cursor all live in the instance); mount only the active Tab's element into
the visible DOM and load `WebglAddon` on show / dispose it on hide. Do not use the serialize addon
for Tab switching; reserve it for persistence across relaunch. Route OSC 52 clipboard writes
through the Tauri clipboard-manager plugin, not `navigator.clipboard`. Keep Tauri's default macOS
menu (it is what makes Cmd-C / Cmd-V work) and claim any app shortcut in a capture-phase `keydown`
handler before xterm.js sees it. Treat WKWebView's dead-key and IME handling as a known-broken
surface: three open xterm.js issues filed from Tauri hosts in 2026 need workarounds or upstream
fixes before the app can be used with a non-US layout.

### Addon set (versions are the npm `latest` tags on 2026-09-10)

| Package | Pin | Role | Notes |
|---|---|---|---|
| `@xterm/xterm` | 6.0.0 | core | Published 2025-12-22. 6.1.0 is at beta.304 and carries the WebGL atlas fix (#5883, merged 2026-05-21) that 6.0.0 lacks; move to 6.1.0 the day it ships. |
| `@xterm/addon-webgl` | 0.19.0 | renderer for the active Tab | WebGL2 only. Exposes `onContextLoss`, `clearTextureAtlas()`, `customGlyphs`. |
| `@xterm/addon-fit` | 0.11.0 | size to container | `fit()` / `proposeDimensions()`; needs a visible, sized container. |
| `@xterm/addon-web-links` | 0.12.0 | URL detection | Constructor takes a handler `(event, uri)`; we decide the modifier (Cmd-click) in that handler. |
| `@xterm/addon-search` | 0.16.0 | find in buffer | Refactored in 6.0 with a line cache. |
| `@xterm/addon-unicode11` | 0.9.0 | Unicode 11 widths | Activate with `terminal.unicode.activeVersion = '11'`. |
| `@xterm/addon-clipboard` | 0.2.0 | OSC 52 | Ship with a custom `IClipboardProvider` backed by `@tauri-apps/plugin-clipboard-manager`. |
| `@xterm/addon-serialize` | 0.14.0 | persistence snapshots | Experimental per its README; one open correctness bug filed today (#6165). |
| `@xterm/addon-image` | 0.9.0 | optional: sixel / IIP / kitty | Load only when WebGL is active (VS Code's rule); 128 MB storage cap by default. |
| `@xterm/addon-ligatures` | 0.10.0 | optional: ligatures | In WKWebView it can only use its fallback ligature list (see Fonts). |
| `@xterm/addon-web-fonts` | 0.1.0 | optional: bundled fonts | Only if we ship a font file with the app. |

Not recommended: `@xterm/addon-canvas` (0.7.0, last published 2024-04-05; the canvas renderer was
removed in 6.0 and the addon no longer exists for 6.x), `@xterm/addon-unicode-graphemes` 0.4.0
(marked experimental in the README addon list), `@xterm/addon-attach` (websocket transport; the pty
bridge is Tauri IPC, decided elsewhere).

## Renderer: WebGL vs DOM in WKWebView

**Facts**

- xterm.js 6.0 ships two renderers: the built-in DOM renderer and `@xterm/addon-webgl`. The canvas
  renderer was removed in 6.0 (#5105: "this addon no longer exists and we recommend using either the
  DOM renderer or WebGL"). The removal rationale (#4779) cites "webgl2 has shipped in Safari/WebKit".
- Tauri uses WKWebView on macOS and the WebKit build is whatever the OS ships; unsupported macOS
  versions "do not receive WebKit updates". We cannot pin the engine.
- WebGL2 arrived in Safari 15 (macOS 12 era), where "the WebGL implementation now runs on top of
  Metal". Any macOS this app targets has WebGL2.
- WebKit caps live WebGL contexts at **16 on the main thread** (`maxActiveContexts = 16` in
  `WebGLRenderingContextBase.cpp`); creating a 17th recycles the least-recently-used context, which
  fires `webglcontextlost` on a terminal that is still on screen.
- `WebglAddon.dispose()` removes the canvas but does not release the GL context; contexts linger
  until GC, so "one WebGL addon per visible pane, disposed on hide" still hits the cap after roughly
  16 Tab switches (#6068, open, reproduced on Chromium and "also seen on macOS"). A commenter
  confirms the issue's suggested workaround (explicitly losing the context on dispose) fixes it.
- The WebGL addon's `onContextLoss` event is the documented hook; the README's own example is
  `addon.onContextLoss(e => addon.dispose())`. VS Code does exactly that and falls back to the DOM
  renderer; it also falls back to DOM when loading the addon throws.
- The DOM renderer and the WebGL renderer compute different device cell widths (WebGL floors,
  DOM does not), so swapping renderers at runtime reflows the grid by up to ~1 device px per cell.
  Maintainer position: won't fix (#6015). VS Code re-fires its resize after loading WebGL for
  this reason.
- Maintainer statement on DOM renderer performance: "the dom renderer is actually way faster now
  than it used to be" and it "only considers the viewport", so scrollback size does not affect it
  (#4779 thread, 2024). It is the renderer VS Code uses when GPU acceleration is off.
- `customGlyphs` (box drawing, block elements, braille, Powerline, progress, git-branch, legacy
  computing ranges) is WebGL-only; the DOM renderer draws those from the font.
- `allowTransparency` "can negatively impact performance" and must be set before `open()`.

**Open WKWebView-specific WebGL bugs (anecdotal, unresolved)**

- #5847 (open, 2026-04): partial row ghosting on rows with ANSI backgrounds after heavy streaming
  output on Tauri/WKWebView with `allowTransparency: true` and an alpha theme background; the
  reporter's trigger was Claude Code output. #5883 (merged 2026-05-21, in 6.1 betas only) fixed the
  texture-atlas page-merge corruption jerch identified, but a later commenter reproduces residual
  corruption on 6.1.0-beta.302 and shows it is not WKWebView-specific (also Chromium at dpr 1.5).
- #5816 (open, 2026-04): totally broken WebGL rendering in Safari on the macOS 26.5 beta, filed
  with a Tauri repro; multiple hosts reverted to canvas (on 5.x) as a workaround; one report on
  macOS 15.7.
- #4728 (open): WebGL/canvas font scaling with DPR > 1 (Retina is DPR 2).

**Decision.** WebGL on the active Tab, DOM everywhere else and as fallback, `allowTransparency`
off, `customGlyphs` on. Agent sessions stream exactly the colored-background output that triggers
#5847, so the fallback path and a user-visible "GPU rendering" toggle are not optional. Because the
engine tracks the user's macOS version, the app must survive a WebGL regression it cannot patch.

## Clipboard, IME and dead keys

**How xterm.js does input.** All keyboard input goes through a hidden `<textarea>`; `keydown` is
the primary path, `keypress` / `input` / `composition*` events are secondary paths, and
`CompositionHelper` diffs the textarea value for IME input. The textarea is moved to the cursor so
IME candidate windows appear at the caret. Copy is handled on the DOM `copy` event of the terminal
element (`copyHandler` sets `text/plain` to the selection); paste is handled on the DOM `paste`
event of the textarea and the element, with bracketed-paste wrapping and `\n` to `\r` conversion.
`Terminal.paste(text)` performs the same transformations for programmatic pastes.

**Clipboard in WebKit**

- `navigator.clipboard.writeText` and `readText` reject immediately outside a user gesture.
  A read that is not an explicit paste gesture (Cmd-V) pops a "Paste" context-menu item on macOS
  before granting access. `navigator.clipboard` is limited to secure contexts.
- `@xterm/addon-clipboard`'s default `BrowserClipboardProvider` calls exactly those two APIs. OSC 52
  writes originate from the program (tmux, nvim yank), never inside a gesture, so the default
  provider will reject in WKWebView. Provide an `IClipboardProvider` whose `writeText` calls the
  Tauri clipboard-manager plugin (`clipboard-manager:allow-write-text`). Do not grant
  `allow-read-text` to the terminal: OSC 52 `?` reports clipboard contents back to the program.
- Cmd-C / Cmd-V / Cmd-A / Cmd-X reach the page as the DOM `copy` / `paste` / selectall / `cut`
  events only when a menu item with that accelerator exists; Tauri's default macOS menu has an
  Edit submenu with undo, redo, cut, copy, paste, select all (see next section).
- Right-click: `rightClickSelectsWord` is documented as "standard behavior in a lot of macOS
  applications"; xterm moves the textarea under the pointer so the native context-menu Paste
  works. WKWebView's native context menu is what appears unless we replace it.

**IME and dead keys: open issues filed from WKWebView / Tauri hosts (all unresolved)**

- #5894 (2026-05): dead key followed by a non-combining char (`~` then `/`) sends `~~` and drops
  `/`. WebKit-only; Chromium unaffected. Affects any layout with dead keys (US-International, ABNT2,
  Spanish, French AZERTY where `^` and `¨` are unmodified dead keys). Reproduced back to 5.5.0 and on
  macOS 15 and 26. Root cause is documented in the issue (`keypress` re-emits the committed dead
  char; the following key arrives with `event.key === "~/"`). A reporter has a host-side workaround
  that tracks the composition and suppresses the duplicate keypress.
- #6144 (2026-08): with the macOS Simplified Chinese input source, the first full-width punctuation
  character is dropped because WKWebView fires `input` before `keydown` and xterm's `_keyDownSeen`
  guard assumes the opposite order. Workaround in the issue: a capture-phase `beforeinput` listener
  on the host element that resets a private field.
- #5887 / #6045 / #6078 (2026): keystrokes reported as keyCode 229 (press-and-hold accent picker,
  some IMEs, dictation) can duplicate or drop characters, and the hidden textarea accumulates
  capitals and spaces until Enter or Ctrl-C, which a later 229 keystroke can re-emit wholesale.
  Confirmed on both Chrome and WebKit; the observed trigger in the wild was WebKit.
- #5374 (Safari, 2025, still confirmed on Safari 26.0.1): with a Japanese Romaji layout, Shift-3
  produces nothing the first time, and key rollover (press S before releasing A) drops the second
  key. A commenter notes Safari fires the custom key handler after `onData`, unlike other engines.
- #5499 (Safari): first character after Caps Lock and auto-repeat characters dropped on 5.5.0;
  reporter says still present on 6.
- #4272: `macOptionIsMeta` disables dead keys typed with Option (xterm ignores composition when
  `macOptionIsMeta && altKey`, `CoreBrowserTerminal.ts` line 855). #2831 is the umbrella issue for
  Option handling on macOS.
- #6084 was retracted: the Korean IME "bug" was a missing UTF-8 locale because the app was launched
  from Finder. Shells spawned by this app must get `LANG`/`LC_CTYPE` set explicitly.

Fixed in 6.0.0: CapsLock triggering input twice on macOS (#5282); duplicate input for some IMEs
(#5024).

## Cmd-key shortcuts: who sees them first

AppKit routes a Command-key event as a key equivalent down the key window's view hierarchy first,
then to the menu bar, and only then as a normal key down. WKWebView's `performKeyEquivalent` sends
the event to the web content and returns YES whenever the web view is first responder ("This lets
webpages have a crack at intercepting key-modified keypresses"); if the page does not handle it,
WebKit re-sends the event through `[NSApp sendEvent:]`, at which point the menu bar's accelerators
fire. Consequences:

- The page sees every Cmd combination first. A `keydown` handler that calls `preventDefault()`
  stops the matching menu item. xterm.js's `attachCustomKeyEventHandler` runs before its own
  handling and lets us return `false` for shortcuts the app owns; a capture-phase listener on the
  window is stronger and does not depend on xterm's keypress ordering quirk in Safari (#5374).
- Anything the page does not claim goes to the menu. Tauri sets `Menu::default` on macOS unless
  `Builder::enable_macos_default_menu(false)` is called. That menu is: app submenu (About,
  Services, Hide, Hide Others, Quit), File (Close Window), Edit (Undo, Redo, Cut, Copy, Paste,
  Select All), View (Fullscreen), Window (Minimize, Maximize, Close), Help. The predefined items
  carry the standard accelerators: Cmd-Q, Cmd-H, Cmd-Option-H, Cmd-W, Cmd-M, Cmd-Z, Cmd-Shift-Z,
  Cmd-X, Cmd-C, Cmd-V, Cmd-A, Ctrl-Cmd-F.
- Cmd-W therefore closes the **window** by default. A terminal app wants Cmd-W to close the Tab;
  the spec must replace `close_window` (custom menu, or claim Cmd-W in the page).
- Copy and Paste menu items are what turn Cmd-C / Cmd-V into DOM `copy` / `paste` events. If the
  Edit submenu is removed, those keys stop working in the terminal. (Inferred from the AppKit
  routing above; verify in the prototype.)
- Tauri `zoomHotkeysEnabled` (default false) injects a Cmd-plus/minus zoom polyfill on macOS; leave
  it off and implement font-size shortcuts ourselves.
- Tauri `devtools` is on in debug builds (Cmd-Option-I on macOS via private API); off in release
  unless the feature flag is enabled.
- xterm.js 6.0 removed the alt-to-ctrl-arrow hack (#5346): word-wise Option-arrow movement must be
  bound by the embedder.
- Tauri's `dragDropEnabled` (default true) installs its own drag-drop handler to emit
  `DragDropEvent`s (file paths dropped onto a Session). The documented conflict with HTML5 drag and
  drop is Windows-only; sidebar Tab reordering with HTML5 DnD needs a macOS check in the prototype.

## Font rendering: ligatures, Nerd Fonts, fallback

- The WebGL renderer rasterizes each glyph run into a texture atlas with a 2D canvas: it sets
  `ctx.font = "<style> <weight> <size*dpr>px <fontFamily>"` and calls `fillText`. The DOM
  renderer sets the same CSS font. In both cases **font fallback is the browser's**: a glyph missing
  from the first family in `fontFamily` is taken from the next family, then from system fallback.
  There is no xterm-level fallback logic (#1983 was closed as a usage question). So `fontFamily`
  must be a list: user font, then a symbols font (e.g. a Nerd Font "Mono" variant), then
  `monospace`.
- Metrics come from the **first** family only; a fallback glyph wider than a cell overlaps the
  next cell. `rescaleOverlappingGlyphs` shrinks such glyphs horizontally. Nerd Font "Mono" variants
  fit glyphs to one cell; non-Mono variants do not.
- `customGlyphs` (default true, WebGL only) draws box drawing, block elements, braille, Powerline
  (U+E0A0-E0D4), progress indicators (U+EE00-EE0B), git-branch symbols (U+F5D0-F60D) and Symbols
  for Legacy Computing itself, independent of the font. Everything else in a Nerd Font (icons in
  U+E000-F8FF, U+F0000+) needs the font installed or bundled.
- Web fonts are lazily loaded by the browser; xterm measures synchronously on first use, so an
  unloaded bundled font yields wrong metrics. `@xterm/addon-web-fonts` (0.1.0) exists to preload
  them; alternatively `<link rel="preload" as="font">` in the document head.
- Ligatures: `@xterm/addon-ligatures` needs the font file to parse `calt` tables. In the browser it
  uses the Local Font Access API (`window.queryLocalFonts`) when present, otherwise a **fallback
  list** (Iosevka's default `calt` set: `->`, `=>`, `!=`, `::`, `</>` and so on). WebKit has not
  implemented Local Font Access and its standards-positions thread (#506) lists privacy,
  complexity and platform-dependence concerns. So in WKWebView ligatures are the fallback list
  only, unless the app bundles the font and feeds bytes to the addon. The addon also sets
  `font-feature-settings: "calt" on` (configurable) and must be activated **before** the WebGL
  addon so the atlas picks it up; VS Code recreates the WebGL addon when ligature settings change.
  Ligatures are disabled under the cursor (#5277) and selection over ligatures is handled in both
  renderers (#5276).
- Unicode width: load `addon-unicode11` and set `activeVersion = '11'` so emoji and CJK widths
  match what modern shells and the three agents assume.

## Many Sessions: mount everything, or only the active Tab?

**Facts**

- Buffer memory is a `Uint32Array` of 3 words (12 bytes) per cell per line, plus sparse maps for
  combined characters and extended attributes. At 200 columns with the default `scrollback: 1000`
  plus a 50-row viewport that is about 2.5 MB per Session; 5000 rows of scrollback is about 12 MB.
  The alternate buffer has no scrollback. Twenty Sessions at default scrollback is ~50 MB of buffer.
- The WebGL texture atlas is shared between terminals with identical font/theme config
  (`CharAtlasCache`), so per-instance GPU cost is the context and its canvas, not the glyph cache.
- `Terminal.open()` requires a visible, sized parent "as several DOM-based measurements need to be
  performed", and the docs say to call `open` again if the element changes window. VS Code keeps
  every `TerminalInstance` alive, detaches the wrapper element from the DOM for hidden terminals
  (`detachFromElement`), re-appends and re-opens on `attachToElement`, and on `setVisible(true)`
  flushes pending resizes and resizes again because dimensions may have changed while hidden.
- Only the viewport is rendered by either renderer; a hidden (detached) terminal costs parsing CPU
  for incoming pty data plus its buffer memory, nothing for rendering.
- `SerializeAddon.serialize()` produces a VT string that restores content, cursor and modes;
  "it is best to do before `Terminal.open`" and to restore into a terminal of the same size then
  resize. The addon is labelled experimental; #4470 tracks its state and #6165 (2026-09-10) reports
  the cursor restored one column left after a full-row write.
- WebKit's cap of 16 contexts plus #6068 means "WebGL for every mounted Session" is impossible
  beyond a handful of Tabs, and "WebGL per visible pane, disposed on hide" needs the explicit
  context-release workaround. Kolu (a many-terminal host) holds WebGL only on the focused terminal
  for the same reason (#6015).
- xterm.js's write buffer is capped at 50 MB and data beyond it is discarded; the flow-control
  guide recommends watermark-based backpressure (HIGH 100 KB, LOW 10 KB) using the `write`
  callback. Hidden Sessions still receive pty output and must be backpressured the same way.
- Tauri `backgroundThrottling`: WebKit on macOS 14+ throttles timers and can unload the view about
  5 minutes after it is minimized or hidden; Tauri exposes a policy to change this. A minimized
  window with 20 Sessions streaming would otherwise stall the parser.

**Decision.** One live `Terminal` per Session, never disposed while the Session lives. Only the
active Tab's element is attached and only it has `WebglAddon` loaded; on hide, dispose the addon
and explicitly lose its context; on show, attach, `open`, load WebGL, then `fit()`. Serialize is for
persistence across relaunch (write the snapshot before `open`, into the saved size, then fit), not
for Tab switching. Scrollback is a user setting with a sane default (1000-5000); memory scales
linearly and the number of Tabs is user-controlled.

## Constraints the interaction model must respect

1. The engine is the user's macOS WebKit; the app cannot pin or patch it. Every rendering path
   needs a DOM fallback and a user-facing GPU toggle; corruption bugs (#5847, #5816) are the reason.
2. At most one WebGL context alive per visible terminal; never more than a few in total (WebKit cap
   16, disposal leaks). Split panes, if ever added, count against the same budget.
3. Renderer swaps reflow the grid; run `fit()` only after the intended renderer is loaded, and
   expect a pty resize on every Tab activation when the window size changed while hidden.
4. `allowTransparency` stays off. Transparent window effects are out of the terminal surface.
5. The page sees Cmd-keys before the menu. Every app shortcut must be claimed in a capture-phase
   handler; anything unclaimed falls through to Tauri's default menu. Cmd-W must be re-bound from
   "close window" to "close Tab". Keep an Edit submenu so Cmd-C / Cmd-V produce DOM events.
6. Copy is DOM-event driven; OSC 52 writes go through the Tauri clipboard plugin with write-only
   permission. Paste is bracketed and newline-normalised by xterm; multi-line paste confirmation, if
   wanted, wraps `Terminal.paste`.
7. `macOptionIsMeta` is a per-user setting, default off: on it breaks Option dead keys and
   third-level characters. Option-arrow word movement is the app's job (hack removed in 6.0).
8. Dead keys and several IMEs are broken in WKWebView today (#5894, #6144, #5887/#6045/#6078).
   The spec must budget a host-side input shim (capture-phase `beforeinput`/`keydown` listeners
   around xterm's textarea) or accept "US layout only" for v1, and must test AZERTY, US-International
   and Pinyin explicitly.
9. Spawned shells need a UTF-8 `LANG`/`LC_CTYPE` because Finder-launched apps inherit none
   (#6084 retraction).
10. `fontFamily` is a list; the first family sets metrics. Bundled fonts must be preloaded before
    the first `open`. Ligatures beyond the fallback list require bundling the font file.
11. Backpressure every Session's pty stream (watermarks around `write` callbacks), including hidden
    ones; set `backgroundThrottling` so a minimized window does not stall parsers.
12. Persistence snapshots use `serialize()` written before `open` at the saved size; the addon is
    experimental, so restore must tolerate a cursor off by one.
13. `open()` needs a sized, visible container: the layout must give the active Tab's pane real
    dimensions before mounting, and the sidebar collapse/resize must trigger `fit()`.
14. Link activation is our handler's decision (Cmd-click on macOS); plain click must stay with the
    foreground process when it has mouse tracking on.

## Not verified

- Whether `tauri://localhost` counts as a secure context for `navigator.clipboard` (moot if the
  Tauri plugin is used).
- That removing the Edit submenu breaks Cmd-C / Cmd-V in WKWebView (follows from AppKit routing;
  needs a prototype check).
- Real memory per Session under load (buffer math is from the cell layout; atlas, DOM and parser
  overhead not measured).
- Whether HTML5 drag and drop in the sidebar conflicts with Tauri's drag-drop handler on macOS
  (documented for Windows only).
- The exact WKWebView (macOS) behaviour of `#6068`'s context leak; it is reported on Chromium and
  "also seen on macOS".

## Sources

- xterm.js README (addon list, supported browsers): https://github.com/xtermjs/xterm.js/blob/master/README.md
- xterm.js 6.0.0 release notes (canvas removal #5105, alt-arrow hack removal #5346, CapsLock fix
  #5282, IME duplicate fix #5024, ligature work #5285/#5208/#5277/#5276, OSC 52 #4220):
  https://github.com/xtermjs/xterm.js/releases/tag/6.0.0
- npm versions and publish dates: `npm view @xterm/<pkg> version time` on 2026-09-10
- `ITerminalOptions` docs (allowTransparency, customGlyphs, scrollback, rescaleOverlappingGlyphs,
  macOptionIsMeta, rightClickSelectsWord): https://xtermjs.org/docs/api/terminal/interfaces/iterminaloptions/
- `Terminal` class docs (open needs visible element, attachCustomKeyEventHandler, paste, resize
  debounce): https://xtermjs.org/docs/api/terminal/classes/terminal/
- Flow control guide (50 MB write buffer, watermarks): https://xtermjs.org/docs/guides/flowcontrol/
- WebGL addon README (`onContextLoss`, dispose on loss): https://github.com/xtermjs/xterm.js/blob/master/addons/addon-webgl/README.md
- WebGL addon typings (`customGlyphs` ranges, `clearTextureAtlas`): https://github.com/xtermjs/xterm.js/blob/master/addons/addon-webgl/typings/addon-webgl.d.ts
- WebGL texture atlas draws via 2D canvas `fillText` with `fontFamily`: https://github.com/xtermjs/xterm.js/blob/master/addons/addon-webgl/src/TextureAtlas.ts
- Atlas shared between terminals with equal config: https://github.com/xtermjs/xterm.js/blob/master/addons/addon-webgl/src/CharAtlasCache.ts
- Buffer cell layout (`CELL_INDICIES = 3`, `Uint32Array`): https://github.com/xtermjs/xterm.js/blob/master/src/common/buffer/BufferLine.ts
- Clipboard handling (`copyHandler`, `handlePasteEvent`, `paste`, `rightClickHandler`): https://github.com/xtermjs/xterm.js/blob/master/src/browser/Clipboard.ts and https://github.com/xtermjs/xterm.js/blob/master/src/browser/CoreBrowserTerminal.ts
- Clipboard addon source (`BrowserClipboardProvider` uses `navigator.clipboard`, OSC 52 `?` reads): https://github.com/xtermjs/xterm.js/blob/master/addons/addon-clipboard/src/ClipboardAddon.ts
- Serialize addon typings (restore before `open`, same size): https://github.com/xtermjs/xterm.js/blob/master/addons/addon-serialize/typings/addon-serialize.d.ts
- Serialize README ("experimental"): https://github.com/xtermjs/xterm.js/blob/master/addons/addon-serialize/README.md
- Fit addon typings: https://github.com/xtermjs/xterm.js/blob/master/addons/addon-fit/typings/addon-fit.d.ts
- Web-links addon typings (handler, hover, urlRegex): https://github.com/xtermjs/xterm.js/blob/master/addons/addon-web-links/typings/addon-web-links.d.ts
- Unicode11 README: https://github.com/xtermjs/xterm.js/blob/master/addons/addon-unicode11/README.md
- Image addon README (protocols, limits): https://github.com/xtermjs/xterm.js/blob/master/addons/addon-image/README.md
- Ligatures addon README, typings and font loading (`queryLocalFonts`, fallback list,
  `fontFeatureSettings`, activate before WebGL): https://github.com/xtermjs/xterm.js/blob/master/addons/addon-ligatures/README.md ,
  https://github.com/xtermjs/xterm.js/blob/master/addons/addon-ligatures/typings/addon-ligatures.d.ts ,
  https://github.com/xtermjs/xterm.js/blob/master/addons/addon-ligatures/src/font.ts
- Web-fonts addon README (lazy font loading vs synchronous measurement): https://github.com/xtermjs/xterm.js/blob/master/addons/addon-web-fonts/README.md
- Issues, xterm.js: #5847 https://github.com/xtermjs/xterm.js/issues/5847 ; #5816 https://github.com/xtermjs/xterm.js/issues/5816 ;
  PR #5883 https://github.com/xtermjs/xterm.js/pull/5883 ; #6068 https://github.com/xtermjs/xterm.js/issues/6068 ;
  #4379 https://github.com/xtermjs/xterm.js/issues/4379 ; #4779 https://github.com/xtermjs/xterm.js/issues/4779 ;
  #6015 https://github.com/xtermjs/xterm.js/issues/6015 ; #4728 https://github.com/xtermjs/xterm.js/issues/4728 ;
  #5894 https://github.com/xtermjs/xterm.js/issues/5894 ; #6144 https://github.com/xtermjs/xterm.js/issues/6144 ;
  #5887 https://github.com/xtermjs/xterm.js/issues/5887 ; #6045 https://github.com/xtermjs/xterm.js/issues/6045 ;
  #6078 https://github.com/xtermjs/xterm.js/issues/6078 ; #5374 https://github.com/xtermjs/xterm.js/issues/5374 ;
  #5499 https://github.com/xtermjs/xterm.js/issues/5499 ; #4272 https://github.com/xtermjs/xterm.js/issues/4272 ;
  #2831 https://github.com/xtermjs/xterm.js/issues/2831 ; #6084 https://github.com/xtermjs/xterm.js/issues/6084 ;
  #1983 https://github.com/xtermjs/xterm.js/issues/1983 ; #3807 https://github.com/xtermjs/xterm.js/issues/3807 ;
  #4470 https://github.com/xtermjs/xterm.js/issues/4470 ; #6165 https://github.com/xtermjs/xterm.js/issues/6165
- VS Code reference behaviour (dispose WebGL on context loss, DOM fallback, refresh dimensions after
  WebGL load, image addon only with WebGL, recreate WebGL when ligatures change):
  https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/terminal/browser/xterm/xtermTerminal.ts
- VS Code detach/attach/setVisible pattern: https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/terminal/browser/terminalInstance.ts
- WebKit WebGL context cap (`maxActiveContexts = 16`, LRU recycle): https://github.com/WebKit/WebKit/blob/main/Source/WebCore/html/canvas/WebGLRenderingContextBase.cpp
- WebKit key-equivalent routing (`performKeyEquivalent`, re-send of unhandled events): https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/mac/WebViewImpl.mm
- Apple, Handling Key Events (key equivalents: view hierarchy, then menu bar, then responder chain): https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/EventOverview/HandlingKeyEvents/HandlingKeyEvents.html
- WebKit Async Clipboard API (gesture requirement, paste callout, secure context): https://webkit.org/blog/10855/async-clipboard-api/
- WebKit, New Features in Safari 15 (WebGL2, Metal): https://webkit.org/blog/11989/new-webkit-features-in-safari-15/
- WebKit standards position on Local Font Access: https://github.com/WebKit/standards-positions/issues/506
- MDN `queryLocalFonts` (limited availability, permission): https://developer.mozilla.org/en-US/docs/Web/API/Window/queryLocalFonts
- Tauri webview versions (WKWebView on macOS, no WebKit updates on unsupported macOS): https://v2.tauri.app/reference/webview-versions/
- Tauri default macOS menu source (`Menu::default`, `enable_macos_default_menu`):
  https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/src/menu/menu.rs and
  https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/src/app.rs
- Tauri window/webview config source (`dragDropEnabled`, `transparent` + `macOSPrivateApi`,
  `zoomHotkeysEnabled`, `devtools`, `backgroundThrottling`, `acceptFirstMouse`):
  https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-utils/src/config.rs and https://v2.tauri.app/reference/config/
- Tauri window menu guide: https://v2.tauri.app/learn/window-menu/
- Tauri clipboard-manager plugin (permissions): https://v2.tauri.app/plugin/clipboard/
