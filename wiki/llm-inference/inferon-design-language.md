# Inferon design language

_Established 2026-11-10 (ITEM-134, ITEM-146, ITEM-149). The tokens live in
`web/src/styles/global.css`; the rules that can be measured are enforced by
`web/src/libs/contrast.test.ts` and `web/src/libs/minimalCopy.test.ts`._

## The one idea

**Data is the loudest thing on the page.** Colour, type weight, gradients and
motion exist to separate data from the furniture, never to compete with it. Every
rule below is that idea applied.

## Colour

### Roles, not names

| Role | Light | Dark | Because |
|---|---|---|---|
| Canvas | `#fcfdff` | `#15171c` | The page. Slightly cool, not grey: pure grey is what a page looks like when nobody chose a colour |
| Card | `#ffffff` | `#1d2026` | The raised surface. Separated from the canvas by a hairline, never by a shadow |
| Sunken | `#f2f4f7` | `#25282f` | Track of a bar, well of an input. **Not a text background** (see below) |
| Ink | `#1b1e25` | `#f9fafb` | Body text, 16:1 on the canvas |
| Muted ink | `#5b616b` | `#babec6` | Labels and captions, 6.1:1 on canvas and card |
| Accent | `#f386a1` | `#f386a1` | The brand pink. **A fill only** (2.2:1): a wash, a rule, a dot |
| Accent ink | `#932b45` | `#f1a6b0` | The accent as text: active nav, links, the one emphasis |
| Success / Warning / Danger | `#156f41` / `#945a00` / `#af2938` | `#73ce95` / `#ebb25f` / `#f88d96` | Verdicts, each ≥ 4.5:1 on canvas and card |
| Chart 1–5 | pink step → deep pink → teal → slate → ink | same family, lightened | One hue family ordered by prominence, never a rainbow |

Neutrals carry 2–6% chroma on hue 264 and are stepped in OKLCH, so "one step
darker" means the same thing everywhere.

### The budget

Roughly **90% neutral, 8% accent, 2% semantic**. Colour appears only where
something is happening: a selected thing, a focused thing, a failing thing.

### Where colour is allowed to appear

- **Interaction**: active nav (15% accent wash), nav hover (8%), table row hover
  (5%), text selection (28%).
- **Verdict**: a value that breaches a threshold, and its glyph. Never a filled
  card.
- **Charts**: series marks at ≥ 3:1 against the surface.
- **Nowhere else.** A card is not tinted because it is important; a heading is not
  coloured because it is a heading.

### The muted-surface rule

Muted ink on the *sunken* surface measures 5.66:1: below the floor. So the sunken
surface is a track, not a page. The workspace page is the **canvas** and its cards
are white, which is why the dashboard changed from a muted page to a canvas page
in 2026-11-11.

## Gradients

Three roles and no others, all single-hue and all interpolated in OKLab (a plain
`sRGB` ramp passes through grey and looks dirty):

| Class | Where | Strength (light / dark) | Bound by |
|---|---|---|---|
| `surface-glow` | Behind a page title, once per viewport | 8% / 5% accent | body ink ≥ 12:1 at the peak |
| `surface-wash` | The top 12rem of a content area | 2% / 2% accent | muted ink ≥ 6:1 at the peak |
| `surface-depth` | A card header, top to bottom | 2% / 1.5% ink | muted ink ≥ 6:1 at the peak |

Those percentages are **measured, not chosen**: an accent wash over this canvas
can only reach 2% before muted ink drops under 6:1, so a wash over content is 2%
and a glow (which sits behind a heading, where the text is body ink) may be 8%.

### Where they are applied, today

| Utility | Used on |
|---|---|
| `surface-glow` | the landing hero, behind the badge and the title |
| `surface-wash` | the top of the workspace content area (12rem) |
| `surface-depth` | every card header |

Auth pages carry none: they are one card on the canvas, and a third glow would
break the one-light rule.

### Rules

1. **One hue.** A gradient fades a colour into the surface. Two hues is decoration.
2. **Never behind data.** No gradient under a number, a table cell or a chart: it
   changes how the value reads. This is why the KPI cards are flat.
3. **Never text.** No `bg-clip-text`. Gradient text fights the 6:1 floor and ages
   badly (the landing hero had one until 2026-11-07).
4. **Never a border.** Gradient borders compete with the hairline that carries the
   whole elevation system.
5. **Removable.** Every page reads correctly with gradients off; they are depth,
   not information.
6. **Halve it in dark mode.** The same alpha is loud on a near-black canvas.

## Charts

The chart style is deliberately its own surface: a chart holds a figure, not text, so
it gets a flat tint and a softer corner rather than the hairline that separates
content cards.

| Token | Light | Dark | Role |
|---|---|---|---|
| `--chart-surface` | `#f7f8fa` | `#1b1e24` | the card, no border, no shadow, 20px padding, 12px radius |
| `--chart-grid` | `rgba(0,0,0,0.05)` | `rgba(255,255,255,0.08)` | the only lines on the plot: **three** horizontal ones |
| `--chart-axis-ink` | `#5b616b` | `#8f96a1` | 10px labels |
| `--chart-title-ink` | `#1f2937` | `#e9ecf1` | the measure, 13px, semibold |
| `--chart-muted-ink` | `#8c939d` | `#9aa1ab` | the aggregate beside it |
| `--chart-cost` / `--chart-cost-soft` | `#fa8c16` / `#ffd666` | `#f59e0b` / `#7c4a12` | warm duo: money |
| `--chart-tokens` / `--chart-tokens-soft` | `#69b1ff` / `#c6e2ff` | `#3b82f6` / `#1e3a5f` | cool duo: a measure with two parts |
| `--chart-line` / `--chart-area` | `#3b82f6` / 45% | `#60a5fa` / 35% | the area chart |

**Rules**

1. **Three gridlines, never five**, and no vertical gridlines. Zero, the midpoint and
   a maximum rounded *up* to two significant figures, so the tallest bar cannot cross
   the top line (`libs/chart.ts`, tested).
2. **The type answers to the measure**: a smooth area when the value is a quantity
   over time, stacked columns when it has two parts. A metric switch changes the type,
   not just the numbers.
3. **Monotone cubic, not Catmull-Rom.** A plain spline dips below the baseline between
   two quiet days and a peak, which in an area chart reads as negative traffic.
4. **Leave a third of the slot empty.** Bars are 62% of their slot; the top corners
   are 1.5px and the seam between segments is flat.
5. **A near-zero day keeps a 1.5px sliver; a zero day gets nothing.** A sliver on an
   empty day is a lie about the data.
6. **Sparse axis labels** — four dates across thirty columns. Every fifth label is a
   label nobody reads.
7. **No data dots at rest, no legend for one series, no gradients in a fill.**

**One deviation from the reference style, and why:** its axis labels are `#9CA3AF`,
which is **2.39:1** on `#F7F8FA` — under any small-text floor. They use the product's
muted ink instead (5.9:1). Everything else is the reference's own value.

## Type

- Scale at the tokens: 13 / 15 / 17px, roomier leading. Nothing renders below 13px.
- Mono is for identifiers only. **Labels are sans, sentence case, no tracking**:
  uppercase 12px mono with wide tracking was the least readable combination this
  product ever shipped, and it came back once.
- Metric values are 1.75rem, tight tracking, `tabular-nums`, no leading.
- Headings use `text-balance` so a title does not end in a widow.

## Space and shape

- Rhythm: 6 between sections, 5 inside a card, 4 inside a grid.
- Elevation by hairline, not shadow. The one shadow is the selected nav tab, where
  a raised affordance is the point.
- Radius: 8px cards, 6px controls, 4px chips.

## Motion

- **Colour, opacity, transform or shadow, never geometry.** `transition-all` is
  banned: it animates layout properties and the browser has to reflow. Colour is
  allowed because it costs nothing to composite.
- 120–150ms, and only on interaction. Nothing animates on load.
- A global `prefers-reduced-motion` rule sets every animation and transition to
  0.01ms, so the product is static for anyone who asks.
- No motion that carries meaning on its own: the live dot pulses, but the state it
  shows is also written down.

## The tests that hold this

- `contrast.test.ts`: body ink ≥ 12, muted ≥ 6, verdict inks ≥ 4.5, focus ≥ 3,
  chart marks ≥ 3; the accent stays *below* 4.5 in light mode (so it can never be
  used as text); each gradient's peak keeps its floor; both themes define the same
  colour tokens.
- `minimalCopy.test.ts`: no sentence in a screen namespace, no `_help`/`_desc`/
  `_note` keys, no page-header description.
- `designLanguage.test.ts`: sentence case and no tracking on labels, nothing below
  13px, no raw accent as text, no gradient text, no `transition-all`, transitions
  only on colour/opacity/transform/shadow and never longer than 200ms, radii from
  the three in this document, and every page section separated by the same value.
- `check-guides.py`: no em dash in prose.

A rule that is not in one of these is a preference, and preferences drift.
