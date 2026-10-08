---
name: app-signature
description: Use when designing an app's launcher icon, its animated splash screen or its launch animation in the owner's own signature (DropNote, Velino). The icon is two ideas of the product merged in one silhouette by real cut-outs, rendered with depth, in three modes. The splash starts as one flat colour that shrinks into the mark, the mark's parts appear, and loading is the mark's own motion on an irregular beat. Ships the method, the rules learned from rejected rounds, the measured size rule, a spring-motion generator in plain sh and awk, and a working splash example. Triggers in any language, such as app icon, launcher icon, logo for the app, splash screen, animated splash, launch animation, ikon təklifləri, açılış ekranı, splash animasiyası. Do NOT use for a full brand identity, a wordmark, a UI icon set, store graphics or screenshots, or the splash of a web site.
metadata:
  type: design-method
---

# App signature — the icon and the splash

Two pieces of the owner's design signature, proven twice: DropNote's icon and splash (chosen 2026-09-24
and 2026-09-25) and Velino's (icon and splash, 2026-10-05). They came out of many rejected rounds, and
the rules below are what those rounds taught. Use them so a new app does not repeat the rounds.

**Read the owner's other projects first.** Before drawing anything, open the brand folder and the style
record of a sibling app (`brand/`, `docs/STYLE.md`) and render its icon and splash. The owner points at
them as the model, and "look at my other projects" was the sentence that ended eight wrong rounds.

## The icon in one paragraph

Write the product's sentence, pick its two strongest ideas, and merge them into **one silhouette with the
second idea cut out of the first by real negative space** (DropNote: a bell cut out of a map pin, a place
and an alert; Velino: a notification shade cut out of a wallet). Render it with depth, not as a flat
glyph, and ship it in three modes. Details, rules and recipes: `references/icon-method.md`.

## The splash in one paragraph

The screen starts as **one flat colour**; it shrinks into the mark; the mark's own parts appear inside it
(springs, never a fixed bezier beat); the name rises at the foot; until the app is ready, **the mark's own
motion on an irregular beat is the loading state**. The mark is sized so its white area is about 3.4 % of
the screen. A generator for the spring motion and a working example ship here: `scripts/motion.sh`,
`assets/splash-example.html`. Details and the checks: `references/splash-motion.md`.

## Process

1. **Read the product's own documents** (the technical task, the feature notes) and write its sentence:
   what it does, in one line, with its one unusual fact. Velino: it listens to bank-app notifications,
   reads the amount, offers to record the payment, the user always confirms, nothing leaves the phone.
2. **Look at the owner's other apps** (above) and, for a comparison that is not the owner's, collect the
   icons of apps in the same category (the store's search catalogue gives the first 35 in one request; keep
   the contact sheet in the references folder, never copy a mark).
3. **Propose a round of 6-8 that differ in the *source* of the mark**, not in a detail. Each card shows the
   mark at full size, in the three modes, in the launcher masks, at 72/54/42 px, as the status-bar glyph,
   with a plain "wins / risk" and **what it could be misread as, said before the designer has to**.
4. **Record every verdict in the designer's words**, with the reason, and turn each into a rule for the
   next round. A round that changes a detail of the last one is not an alternative; a designer who says
   "I did not see much difference" is right.
5. **When one is chosen, ask what stays open** (colour, the resemblance to the sibling app) and then build
   the splash from the chosen mark, taking apart its silhouette into the parts that move.
6. **Measure, do not eyeball**: the mark's centring, its size against the sibling's splash, contrast at
   twelve hues, the loop actually moving. Say what was checked and what was not.

## Rules that cost rounds (do not relearn them)

- **A flat white glyph on a gradient disc is "amateur".** The icon is a silhouette with real cut-outs and
  depth. Thin parts and shapes drawn by eye are the same fault.
- **A symbol that belongs to another kind of app sinks the icon**: a bell is a notification app, a tick a
  to-do app, a lock a lock app, a bank a bank app, a dot is a real unread notification, an envelope a mail
  app, a receipt does not read, a sparkle is "AI", a coin stack is a database, a ring with a gap is a power
  button, a pie or a rising line is a finance cliché. The merge has to make a *new* thing.
- **Generic objects are the crowded pile** (piggy bank, wallet alone, coins); a store sample of 35
  budget apps had 4 piggy banks and 3 wallets, and no notification motif at all.
- **Same device twice is a sibling, not a new app.** Say so; it can be wanted.
- **Silhouettes with notches are off-centre by construction** (the clasp notch of the Velino wallet moved
  its centre 3 units left of the icon's). Measure the silhouette's bounding box, not the drawing.
- **A spring over a big range breaks**: a plain spring scaling 26x to 1x overshoots to a negative scale.
  Interpolate in log space (`scripts/motion.sh shrink`).
- **In SVG, `transform-box: fill-box` changes the origin of `scale()` set by attribute** on groups; keep
  the wrapper groups on `view-box` with origin `0 0` and centre by nested `translate()`.

## Placement

| Kind | Default root | Visibility | Ownership |
|---|---|---|---|
| App icon master (three modes, 1024, unmasked) | `brand/icon-light.svg`, `icon-dark.svg`, `icon-tinted.svg` | shared | own |
| Splash screen | `screens/splash.html` (or the project's screen folder) | shared | own |
| Icon and splash decisions, verdicts and rules | the project's style record (`docs/STYLE.md`) | shared | own |
| Proposal boards (one per round) | the design root, `app-icons-<n>.html` | shared | own |
| Reference sheets of other apps' icons | `moodboard/` for the sheet, the scratch folder for the originals | shared / local | foreign |

A project overrides any row in its workspace configuration; this skill reads the override.
The splash example and `scripts/motion.sh` are copied into the project's own tree, and the project edits them.

## Files in this skill

- `references/icon-method.md` — the method, the construction, the rendering recipe, the checks.
- `references/splash-motion.md` — the timeline, the structure, the size rule, the review controls, the checks.
- `scripts/motion.sh` — spring easings, log-space shrink and damped nudge keyframes (`sh`, `awk`).
- `assets/splash-example.html` — a working splash (Velino's mark) to copy and re-draw.
