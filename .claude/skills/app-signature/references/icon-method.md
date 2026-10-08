# The icon method

## Where it comes from

- **DropNote** (chosen 2026-09-24, three rounds): "Çanlı iğne", a bell cut out of a map pin's head, a place and
  an alert, "the product's sentence in one shape". The designer: "extremely creative, and I liked it". Rejected
  on the way: five flat one-colour glyphs ("amateur-looking": thin parts, drawn by eye), and four more of
  good craft but a wrong idea ("the first icon reminds me of a documentation app").
- **Velino** (chosen 2026-10-05, eleven rounds): a wallet with the notification shade cut out of it. The ten
  rounds before it taught the rules in `SKILL.md`.
- **ArenaGo** builds its mark from its own domain (the lines of a pitch) and generates all eight assets from
  the tokens by a script, so two apps' icons cannot drift. It keeps the mark at 64 % of the canvas, inside
  Android's adaptive-icon safe area (72 of 108 dp, 66.7 %).

## Steps

1. **The product's sentence.** What it does, plus the one fact no other app has. List the two or three
   strongest *things* in it (Velino: a notification, a wallet/money, the initial V).
2. **Pair them.** The union is a silhouette from one idea with the other cut out, or two ideas sharing one
   outline. Try the reverse (A in B, B in A) and every container: the container decides what the icon is
   read as first.
3. **Check each pair against the misread list** (`SKILL.md`) and against a store sample of the category.
4. **Draw it as a real silhouette** (below), with cut-outs that are holes, not shapes in the ground colour.
5. **Render with depth** (below), in three modes.
6. **Compare** at 72 / 54 / 42 px, in the three launcher masks, as a one-colour glyph (status bar and the
   Android 13 themed icon), and next to the sibling app's icon.
7. **Record the verdict** in the designer's words.

## Construction: one silhouette, real holes

Draw in a 108 x 108 canvas (Android's adaptive icon: the visible mask is 72, the safe zone is a circle of
66, so radius 33 around 54,54). Build the object in an SVG `<mask>`: white where the object is, black where
it is cut out; paint one rectangle with the ink gradient through that mask. Everything moves, scales and
is measured as one shape, and the ground and its glow show through the holes.

```
<mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="108" height="108">
  <rect width="108" height="108" fill="#000"/>
  <rect x="32" y="24" width="40" height="14" rx="6" fill="#fff"/>                       <!-- a card in the mouth -->
  <rect x="24" y="32" width="54" height="48" rx="12" fill="#000" stroke="#000" stroke-width="6"/>  <!-- a gap around the body -->
  <rect x="24" y="32" width="54" height="48" rx="12" fill="#fff"/>                      <!-- the body -->
  <rect x="62" y="48" width="26" height="16" rx="8" fill="#000"/>                       <!-- a clasp notch (a hole) -->
  <path d="..." fill="#000"/>                                                            <!-- the idea that is cut out -->
</mask>
<rect width="108" height="108" fill="url(#ink)" mask="url(#m)"/>
```

Separate two white shapes with a black stroke drawn first (the "gap"), so they do not merge.

## Rendering recipe (DropNote's, in Velino's variables)

- **Ground:** a three-stop linear gradient (light to dark) in one hue, plus a radial white glow from the top
  left (opacity .3 to 0). Velino light: `oklch(73% .2 H)`, `oklch(53% .21 H+8)`, `oklch(40% .17 H+14)`.
- **Object:** a vertical gradient, white to a tint of the hue (`#fff` to `oklch(92% .05 H)`).
- **Depth:** a soft shadow under the object (`drop-shadow(0 2.4px 2.6px rgba(0,25,30,.38))`) and a rim
  highlight, a short white arc (opacity .55, 2.6 wide, round caps) on the top-left curve. Keep the arc off
  the cut-outs.
- **Three modes**, as DropNote ships them: light (the above), dark (a near-black ground `oklch(34% .03 H)`
  to `oklch(12% .015 H)`, the object in the accent, `oklch(90% .12 H)` to `oklch(70% .15 H)`), tinted
  (greyscale: ground `#5a5a5d` to `#161618`, object `#fff` to `#cfcfd3`; the system recolours it).
- Hand the master over **unmasked, 1024 x 1024**, the platform applies its own mask; for iOS 26 as layers
  (ground, object with its cut-out, the small parts) so the system glass can act on the object.
- Brand colours are literals in an exported asset; a splash never reuses the icon's gradient.

## Checks

- **Centring** is measured on the silhouette's pixels (render, take the near-white pixels, compare their
  bounding-box centre with the icon's). Velino's wallet was 3 units left; the vertical was 2 units high.
- **Misread**: write, for each proposal, what it could be taken for, before the designer says it.
- **Small**: 72 / 54 / 42 px and the 24 px status-bar glyph; fine cut-outs (two lines of text) vanish
  below 54 px, so say it.
- **Resemblance**: the same device as the sibling app makes the two look related; ask whether that is wanted.
- **Not verified unless done**: look-alike icons in the store; a real launcher. Say so.

## The store sample (optional, for "what do apps like ours look like")

The App Store search catalogue returns icons without a key: `https://itunes.apple.com/search?term=<query>
&entity=software&country=us&limit=14` (JSON with `artworkUrl512`). Take six queries, dedupe, build a contact
sheet, count the patterns (letters, abstract marks, objects, colours), and state its limits: one store, one
country, search results and not a ranking. Velino's sample of 35 had 7-8 letter-marks, about 9 abstract
marks, 4 piggy banks, 3 wallets, mostly green, and no notification motif.
