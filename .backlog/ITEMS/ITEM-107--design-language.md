## ITEM-107: Design language — line icons, restrained colour, minimalist hierarchy

**As a** reader
**I want** the interface to point at what matters without decoration
**So that** I can scan a page in seconds and trust what stands out

**Sources.** The dashboard-design principles (visual hierarchy, consistency,
minimal cognitive load, accessibility, highlight key metrics, avoid overload) and
the brief's direction: line icons, colour sparingly and mostly for highlights,
minimalist.

**Current state (2026-11-03).** No icons. KPI verdicts tint the whole card
(background + border), so five cards carry five colours at once and the colour is
decoration, not emphasis. Headline and secondary numbers share the same treatment.
`Status` conveys met/not-met by colour alone.

**Acceptance Criteria**
1. **One line icon per section/measure**, from `slimicons` (MIT, 1.5 px stroke),
   rendered at 16–20 px in the section header and at 14 px beside a KPI label.
   Never two icon styles on one page.
2. **Colour is a highlight, not a surface**: cards keep the neutral surface; the
   only colour is the value text and a small marker when a metric is actionable
   (bad/warn). Good is the absence of colour, not green fill.
3. **No colour-only meaning**: a tone also changes an icon and, where space
   allows, a word (met/not met, above/below).
4. **One type scale**: a page has at most one primary number per section
   (larger), with secondary metrics smaller; no bold–bold competition.
5. **Whitespace over borders**: section separation by spacing; borders only where
   they group a table.
6. **Contrast**: every text/background pair meets 4.5:1 in light and dark; icons
   are decorative (`aria-hidden`) unless they carry meaning.
