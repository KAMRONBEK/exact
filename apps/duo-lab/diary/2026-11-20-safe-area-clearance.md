surfaces: ios, web
agent: Claude (Opus 5.5)
task: Clear the safe area once on the screens that inset themselves, under main's route clearance, and keep the Arrange screen's panes off a browser's fold.

### Rough

- A route clears its own edges now (LLP 1123 D3), and Duo Lab's screens also padded by `env(-exact-<region>-inset-*)`, so on the iPhone Duo every screen that insets itself lost the side strip's 84 points twice and the home indicator's 34 twice, and its pane and sheet backgrounds stopped short of the display's edges. The build said nothing; `docs/agent-pitfalls.md` and the vocabulary entry for `-exact-safe-area` describe the double count.
- `-exact-safe-area-ignore="container"` on the route, which LLP 1123 D4 names as the replacement, lowers but changes nothing on iOS until the host sends owner updates (stage 5), so the routes use `-exact-safe-area`, which that stage retires.
- `-exact-safe-area="auto"`, a value the vocabulary lists, is refused at lowering with "expected text": the lowerer reads `auto` as CSS's keyword before the row reads its own keywords. The Panes route, which turns its clearance off only while its panes lie on the segments, binds an empty value for the default instead.
- The per-box observations, `var(--exact-<region>-inset-*)`, read no band on iOS yet, so the screens keep the document's `env()` regions until activation.
- Safari on the iPhone Duo simulator (iOS 27.1) reports the fold only with two of its feature flags on (Settings > Apps > Safari > Advanced > Feature Flags): "Device Posture API" and "Viewport Segments API". With them it reports `folded` and two segments that meet at the fold's centre, with no band between them; without them neither API exists.

### Lean in

- One attribute on each route that insets itself clears every inset once: the panes pad by the larger of the corner-adapted `env()` inset and `var(--exact-safe-area-inset-*)`, the route's own, which holds UIKit's bars, and their backgrounds reach under the bars and the home indicator.
- On the web the posture and segments are the browser's own, so Chromium's emulation and Safari's flags drive the same layout. The Arrange screen without UIKit's arrangement places its panes as the Panes screen does, so both follow a book or a laptop fold in either reading direction.

### Needed

- per-box region observations on iOS (LLP 1123 stage 5): not yet
- `-exact-safe-area-ignore` on iOS (LLP 1123 stage 5): not yet
- `-exact-safe-area="auto"` in Contract: refused

### Checkpoints

- On the iPhone Duo simulator (iOS 27.1) open, at 130°, closed and turned upright, with UIKit's bars: Fold's panes reach the display's edges and their text ends 16 points before the strip; Images, Reflow and the keyboard editor run from the readable region's leading edge to 16 points before the strip; the keyboard list's cells end at the strip as before; the note sheet reaches the display's bottom and, with the keyboard up, ends at its top; turned upright, where UIKit's tab bar runs along the bottom, Fold's detail reaches under the bar and its text stops at the bar's top; the Panes screen in UIKit's split keeps its clearance; and `check` reports nothing on these screens.
- `bun test host/web-js/duo-panes.test.mjs` passes; with main's Arrange screen its Arrange checks fail.
- The Duo smoke's `arrangement` and `reading` legs pass, and its `rotate` leg passes now that its taller note sheet compiles; the `lab` leg passes its Fold checks open, folded and closed, then stops at a tap after Reflow reopens, as on main.
- In Safari on the folded Duo simulator, with the two flags on, the Fold screen reports `folded` and two segments and lays its list and detail on them.
