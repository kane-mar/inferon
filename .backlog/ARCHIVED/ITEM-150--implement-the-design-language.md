## ITEM-150: Implement the design language, and make its rules fail the build

**Requested** 2026-11-11: "okay, implement the design language."

The palette and the three gradients were already in place (ITEM-134, ITEM-149).
What "implement" meant here was closing the gap between the document and the code,
and turning the mechanical half of the document into tests, because a rule that is
not tested is a preference.

**Nine rules, each reading the source** (`web/src/libs/designLanguage.test.ts`):

| Rule | Why |
|---|---|
| Labels are sentence case, no tracking | Uppercase 12px mono with wide tracking was the least readable thing this product ever shipped, and it came back in three places: the hero badge, the section eyebrow, and a menu group label |
| Nothing below 13px | The legible minimum the type scale was raised to |
| The raw accent is never text | It is 2.2:1 on the canvas; `accent-ink` is the text token |
| No `bg-clip-text` | Gradient text fights the contrast floor and dates immediately |
| No `transition-all` | It animates layout properties |
| Nothing over 200ms | Motion is 120–150ms, on interaction only |
| Only colour/opacity/transform/shadow transition | What animates is what composites |
| Radii are the three in the document | One toolbar pill had an arbitrary 5px |
| Page sections share one separation | The bill's audit heading sat at 8 while every other section was 6 |

**Fixed by the rules, four real violations:** the three uppercase/tracking labels and
the 5px pill. The bill's audit heading moved from 8 to 6, and gained `text-balance`
like every other heading.

**The document was wrong once, and was corrected.** It said motion may only touch
`transform` and `opacity`, while the product transitions colour on every interactive
element. Colour transitions do not cause layout, so the honest rule is
colour/opacity/transform/shadow, never geometry, at 120–150ms. The document now says
that, and gained a table of where each gradient is applied.

**Deliberately not testable, so not claimed:** that the accent is a *fill* in a given
component (the test only forbids it as text), that a gradient never sits behind a
number (a rule about placement, checked by reading the JSX), and the 90/8/2 colour
budget, which is a design judgement rather than a pattern to grep.

**Verified live:** the landing page renders **0** uppercase labels, the gradient
utilities are served, all four pages 200, and the only `tracking-` class left is
`tracking-tight` on headings, which the language allows. 197 web tests, tsc, eslint,
knip, i18n-check clean.

**Source:** PO request, 2026-11-11.
