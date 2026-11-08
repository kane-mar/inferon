## ITEM-134: Colour that is felt, not noticed

**As** someone reading a dashboard for the fifth time today
**I want** the surface to have a temperature and a point of view
**So that** the product looks made rather than default, without any page shouting

**Request** (2026-11-07, PO): research how modern minimalist sites use colour, and
apply it to the pages.

**What the research says**

1. **Neutrals are tinted, not grey.** Linear, Vercel, Stripe and the shadcn token
   sets all put 2–6% chroma on one hue. `#808080` is the colour of a page whose
   author never chose one; a 3% cool tint is invisible token by token and is most
   of the difference between "designed" and "default".
2. **One accent, three intensities.** A brand colour earns its keep as a fill, a
   5–15% wash, and an ink — never as a second and third accent.
3. **Colour is a signal, not decoration.** It marks *state*: where you are, what
   has focus, what is wrong. A KPI card that is green because it is healthy
   teaches the eye to stop reading the cards.
4. **Soft backgrounds with strong ink** beat saturated text: a 10% wash plus a
   darkened ink keeps AA contrast and reads calmer than the same hue at full
   saturation in the text itself.
5. **Elevation from light and hairlines**, not shadow. Shadows are for layers that
   float, not for every card.
6. **Charts need one hue family, not five hues.** The rainbow ramp is the single
   fastest way to make a restrained report look noisy.
7. **Dark mode is not an inversion.** Less chroma, no pure black, borders carry
   what shadows carry in light.
8. **A budget**: roughly 90% neutral, 8% accent, 2% semantic. If everything is
   tinted, nothing is.

**Delivered**

- `global.css` is now a tinted OKLCH-stepped neutral ramp on one cool hue (264),
  in both themes: canvas `#fcfdff` / `#15171c`, ink `#1b1e25` / `#f9fafb`,
  surfaces and borders as perceptual steps. Every ratio is stated in a comment.
- New semantic tokens, so no component carries a `dark:` variant of its own ink:
  `--accent-ink` (the accent as *text*: 7.7:1 light, 9.2:1 dark), `--success`
  (6.1 / 9.4), `--warning` (5.5 / 9.5), and `--destructive` retuned to 6.4 / 7.9.
  `--ring` is the accent ink, so focus is the one place colour always appears.
- **The accent stays a fill in light mode** (2.2:1) and is never used as text —
  that rule is now enforced, not remembered (see the test below). The chart ramp
  is one family ordered by prominence: a pink step, its deep ink, a desaturated
  teal, a slate, then plain ink. `--chart-1` is deliberately *not* `--accent`: a
  2 px data mark needs 3:1 (WCAG 1.4.11) and the accent is a 2.2:1 wash.
- **Interaction carries the colour.** The active nav is a 15% accent wash with
  accent ink instead of a solid near-black pill; nav hover is 8%; table rows hover
  at 5%; `::selection` is 28% accent. That is the whole of the added colour.
- The landing page loses its pink→magenta gradient headline and gradient icon
  tiles: the emphasised phrase is accent ink, the section eyebrow is a small rule
  plus accent-ink label, and the feature tiles are a 15% wash with the icon in
  accent ink. Loud colour here competed with the product's own charts.
- 36 `emerald`/`amber` utilities are gone; verdicts use `text-success` /
  `text-warning`, so a report cannot invent a green.

**The rule is now tested** (`src/libs/contrast.test.ts`, 9 cases): it reads
`global.css`, measures WCAG ratios from both theme blocks, and fails the build if
body ink, muted ink (≥6:1), any verdict ink (≥4.5:1), accent ink (≥4.5:1) or the
focus ring (≥3:1) drops below its floor, if the accent becomes legible as text in
light mode, if a chart series falls under 3:1 on its surface, or if the two themes
drift apart or stop exposing a token through `@theme`.

**Verified live** on the rebuilt web image: landing `/` and `/fr` 200, nine
dashboard pages 200 in both locales, the served CSS carries both themes'
`--accent-ink`, `--success`, `--warning`, the `tbody tr:hover` wash and
`::selection`; proxy healthy, no web errors. 114 unit tests, tsc, build and
`i18n-check` clean.

**Notes and limits**

- The `ui` vitest project cannot run on this machine (Playwright has no chromium
  for macOS 11) and CI runs only the unit project, so the visual result was
  verified by reading the served CSS and HTML, exactly as ITEM-125's regression
  was caught.
- Known cost, accepted: tinted neutrals shift every surface slightly, so a future
  screenshot comparison of a previous build will differ everywhere. The palette is
  one file, and the test is what stops it drifting.

**Source:** PO request, 2026-11-07 ("Research modern minimalist websites especially
their use of color … update the pages of the website with your research").
