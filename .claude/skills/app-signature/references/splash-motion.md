# The splash

## What the owner's apps do

- **DropNote** (decided 2026-09-25, four sets): the screen starts as one flat white, the white shrinks into the
  pin (a stiff spring, 720 ms), the bell pops in before the shrink has settled, rings (a decaying pendulum,
  the clapper lagging, the pin answering about its tip), the name rises at the foot, and until the app is ready
  the bell rings softly on an irregular beat (strong, small, long rest, medium, 6.4 s): that loop **is** the
  loading state. No gradient, no shadow, no subtitle. Rejected: drawn/dot/ring/out-of-white ("not striking"),
  drop/radar/swarm/route ("not liked"), liquid/bold/3D/typographic ("too much"), and a first pass of the
  chosen idea ("robotic": a fixed curve on a fixed beat).
- **AutoZap** (review rounds 3-6): start from an empty coloured screen, then the logo; what never looked
  professional was how the pieces complete the mark, so the mark is one whole thing arriving, not parts
  stuck together; loading is a rhythm (a line that fills there); review plays slowly on request (`?slow=`).
- **Velino** (2026-10-05): the pattern above, with the wallet: white shrinks into it, the card rises, the clasp
  bites in, two banners drop in from the top edge as notifications do, the wallet squashes as each lands, the
  name rises, and loading is the banners nudging on an irregular beat.

## Versions to offer

- **A** the screen starts as one flat white and shrinks into the mark. (The designer chose A: "it is our
  signature, as in DropNote".)
- **B** the screen starts as one flat ground colour and the white mark pops in (a springy pop from scale 0).
  Offer both: "the first full screen is one colour" can be read either way.

## Timeline (Velino, ms)

| t | what |
|---|---|
| 0-760 | the shrink (A) or the pop (B) |
| 480-520 | the card rises from the mouth; the clasp bites in (springs) |
| 700 / 900 | banner 1 / banner 2 drop in from the top edge, clipped by the body |
| 820 | the body squashes as the first lands |
| 1050-1250 | dots pop, lines draw |
| 1100 | the name rises at the foot (white, bold, 22 px, no subtitle) |
| 2400 -> | loading: nudge of banner 1 (strong), banner 2 (small), a long rest, a medium pair; 6400 ms loop |
| hand-over | in the app, "ready", not a timer; in a prototype one beat (3.6 s), but inside the review frame it holds |

## Structure (SVG, viewBox 360 x 780, `preserveAspectRatio="xMidYMid slice"`)

Centre the mark by nested wrappers so every animated group scales about its own origin:

```
<g transform="translate(180 330) scale(2.25) translate(-54 -52)">       <!-- place the icon's coordinates -->
 <g transform="translate(54 80)"><g class="k-squash"><g transform="translate(0 -24)">   <!-- squash about the bottom centre -->
  <g class="k-body"><g transform="translate(-54 -56)"> ...parts in icon coordinates... </g></g>
```

`k-body` animates `scale()` about (0,0) = the body's centre. Banners and similar parts are clipped by a
`clipPath` of the body, so they slide in from its edge. The ground colour is flat; holes are shapes in the
ground colour on top of the white (the ground is flat, so this is exact).
`.sp-art g[transform]` and the animated groups need `transform-box: view-box; transform-origin: 0 0`
(see the pitfall in `SKILL.md`).

## Motion rules

- **Springs, not curves.** `scripts/motion.sh spring <zeta> <wn>` gives a `linear()` easing from a damped
  spring: a pop `0.46 / 9`, a drop `0.5 / 8.5`, a rise `0.8 / 9`. Chromium 113+ and Safari 17.2+.
- **The shrink is a spring in log space**: `scripts/motion.sh shrink 26 .86 10.5` (scale 26 covers any phone
  from a mark of this size). Do not feed a plain spring a 26x range.
- **The loop is irregular**: `scripts/motion.sh nudge o-b1L 6400 ty 0:4.4 3600:3.1`, then the same for the
  second banner (`1000:3.1 3720:2.8`), the dots (`scale`), and the squash (`squash`).
- **Everything is multiplied by `--k`** (durations and delays through `calc(var(--k) * ...)`), so `?slow=4`
  plays four times slower and the review can see what the eye misses.
- **Reduced motion**: no animations are applied, so the page shows the last frame; then the hand-over.
- **The system launch screen** must match the first frame (plain white for A, the plain ground colour for B)
  or the hand-over flashes.

## Size rule (measured, not guessed)

Render both splashes on the same screen (412 x 915), take the near-white pixels between 12 % and 85 % of
the height (the name and any Skip button are outside it), and compare:

| | DropNote | Velino first | Velino now |
|---|---|---|---|
| Width | 134 px (32.5 %) | 246 px (59.7 %) | 142 px (34.5 %) |
| Height | 180 px (19.7 %) | 256 px (28.0 %) | 147 px (16.1 %) |
| White area | **3.4 %** of the screen | 10.3 % | 3.4 % |
| Centre from the top | 42.2 % | 43.6 % | 42.3 % |

Match the **white area** (about 3.4 %) and the **vertical centre** (about 42 % from the top). A wide, low mark
and a tall, narrow one then weigh the same. Velino's first pass was 3x the area and the designer saw it at
once ("is the logo not too big?"). If asked, the alternatives are the same width or the same height.

## Review controls (the prototype wrapper's contract)

Expose `window.<App>.controls = { states: [...], current, onPick(id), skip: { label, href } }`:
A, B, A slow x4, B slow x4, and Skip; a tap on the screen replays; `?stay=1`, `?slow=N`, `?v=b` in the URL.

## Checks (a splash is not done until these were run)

1. A filmstrip of both versions at `?slow=8` (screenshots at fixed times), looking at the first frame (one
   flat colour), the shrink, the parts, the final frame, the loop.
2. The hand-over opened alone (it goes on), and inside the frame (it holds).
3. Reduced motion: `document.getAnimations().length` is 0 and the last frame is shown.
4. The loop really moves (sample a transform for 9 s and count the displaced samples).
5. Contrast of the name at twelve hues (0, 30, ... 330) in light and dark.
6. The size table above, measured.
7. Say what was not checked: a real phone's frame rate, and the Android system splash hand-over.
