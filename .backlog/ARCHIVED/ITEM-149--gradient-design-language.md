## ITEM-149: A gradient language, and the palette rules written down

**Requested** 2026-11-11: "research the use of color and background color
gradients. form a design language including colors to use, how the colors apply etc.
Update the website with this new information."

**What the research says** (recorded in `wiki/llm-inference/inferon-design-language.md`):

- Gradients read as **depth** when they are one hue fading into the surface, and as
  **decoration** when they mix hues. Every product that ages well uses the first.
- They must be *felt*, not seen: a tonal ramp under ~1.15:1 across its length is
  depth; anything stronger competes with the content.
- One light source per page. A soft radial at the top implies light from above; two
  glows imply nothing.
- **Never behind data.** A gradient under a number changes how the value reads, which
  is why the KPI cards stay flat.
- **Never text, never a border.** Gradient text fights the contrast floor and dates
  fast (this product shipped one until 2026-11-07); gradient borders fight the
  hairline that carries the whole elevation system.
- Interpolate in OKLab: an sRGB ramp passes through grey and looks dirty.
- Halve the strength in dark mode: the same alpha is loud on a near-black canvas.

**Delivered: three roles and no others**

| Utility | Where | Strength | Bound |
|---|---|---|---|
| `surface-glow` | behind a page title, once per viewport | 8% / 5% accent | body ink ≥ 12:1 |
| `surface-wash` | the top 12rem of a content area | 2% / 2% accent | muted ink ≥ 6:1 |
| `surface-depth` | top of a card header | 2% / 1.5% ink | muted ink ≥ 6:1 |

The percentages are **measured, not chosen**: an accent wash can only reach 2% on
this canvas before muted ink (#5b616b, 6.13:1 to start) drops under its floor. A
glow may be 8% because it sits behind a heading, where the text is body ink. That
distinction is now a rule in the doc rather than a judgement call in a commit.

**Two defects the work exposed**

1. **The workspace page was the muted surface with white cards**, which puts muted
   ink on muted at **5.66:1**: already under this product's own 6:1 floor, before any
   gradient. The page is the canvas now and the cards are the raised surface.
2. **The build flattened `color-mix(… var(--pct) …)` to the bare accent.** The first
   implementation would have shipped an opaque pink glow, and only a look at the
   compiled CSS caught it. The mixed colour is its own token now, declared *only*
   inside `@supports`, so a browser without `color-mix` paints no gradient at all
   instead of a pink page.

**Guarded.** `contrast.test.ts` reads the shipped gradient tokens (not the source
percentages) and asserts each peak keeps its floor, and that there are exactly
three gradients. The parser had the same selector-swallows-first-declaration bug
that once hid `--radius`; fixed and noted.

**Verified live:** 3 utilities served, 6 `color-mix` tokens, **0** opaque fallbacks;
the landing page carries the glow, the dashboard the wash (and four card headers the
depth); all pages 200; 188 tests, tsc, eslint, knip, i18n-check clean.

**Not done:** no gradient on the auth pages (they are a single card on the canvas;
adding one would need a third glow and the language says one is enough), and no
noise/dither texture (banding is invisible at 2%, and a texture is a new material
with its own rules).

**Source:** PO request, 2026-11-11.
