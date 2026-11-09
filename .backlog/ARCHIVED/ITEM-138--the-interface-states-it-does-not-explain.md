## ITEM-138: The interface states, it does not explain

**As** someone using the dashboard
**I want** numbers and labels, not paragraphs
**So that** the screen is scannable and a term I do not know is one click away in
the guides instead of three lines of apology on the page

**Requested** 2026-11-09 (PO): "There is too much text on the interface now. The
interface should not contain explanations … if there is a need for an explanation
or definition of a term then there should be a link to some documentation. But the
interface itself should be minimal. The terms and data NEEDS to be self
explanatory."

**What it reverses, deliberately.** ITEM-125 shipped "the page explains itself — no
documentation required" and printed a definition under every metric. That was the
right call while no guides existed. They exist now (30 published pages), so the
choice inverted: prose on the screen competes with the number it is describing, and
it grows every time somebody adds a metric. ITEM-127 then ITEM-136/137 each added
another line to the Runtime page; by the end it read like a document with charts
attached.

**Delivered**

- `KpiCard` carries a label, value, unit, window and at most a two-word verdict.
  `definition` is gone from the type, so it cannot come back; `kpiCaption` and its
  test are deleted. 27 definitions and 14 hints removed across seven pages.
- **A term that needs defining is now a link, not a sentence.** `libs/docs` builds a
  URL from `NEXT_PUBLIC_DOCS_URL` and returns `null` when it is unset, and
  `DocsLink` renders nothing in that case — no dead links, and no prose standing in
  for one. Runtime (claim, bands, drift), Payloads (four cards) and the audit report
  link to their guides. It switches on when ITEM-112 picks a host.
- **Labels carry the scope** instead of a caption explaining it: "Gateway p50 · ≤4k
  tokens", "Under 1 ms · ≤4k tokens", "Requests measured", "p50 < 1 ms". Bands are
  "≤4k", "4k–50k", "50k–150k", ">150k"; the drift legend is "ok / slow / failing".
- **Every page-header description is gone** (nine of them), along with the section
  description on the landing page. A title plus data.
- **`labels.ts` drops the second "line of meaning"** per error class. One phrase,
  which must stand alone: "Provider unreachable", "Checker rejected the answer".
  The longest is 29 characters and the test enforces a ceiling — a phrase that needs
  a clause is a phrase that needs renaming.
- 57 `_help` / `_desc` / `_note` keys deleted from both locales; 55 long strings cut
  to labels ("No provider yet — add one so requests have somewhere to go" ->
  "No providers yet").

**The rule is now enforced** — `libs/minimalCopy.test.ts`:

1. no string in a screen namespace may be a sentence (more than 12 words, or two
   sentences in one string), except SEO metadata and values built from data (they
   carry a `·` separator);
2. no `_help` / `_desc` / `_note` / `_body` keys in those namespaces;
3. no page header may carry a `description`.

A guard rather than a habit, because the pressure to explain is constant: every
future metric arrives with a sentence attached.

**Verified live** on the rebuilt web image: the Runtime page's rendered text is
labels and numbers only — `Gateway p50 · ≤4k tokens | No traffic | … | The claim |
p50 < 1 ms | p95 < 5 ms | last 24 h · 0 samples · median — at 0 tokens` — and the
six former explanation strings return zero matches. Landing and eight dashboard
pages 200 in en and fr.

**Not done**

- `EmptyState` bodies that remain are instructions to act ("Create a key"), not
  definitions, and they are capped by the guard like everything else.
- The docs links are inert until ITEM-112 sets `NEXT_PUBLIC_DOCS_URL`. That is the
  intended failure mode: no link is better than a broken one, and the alternative
  would be putting the paragraph back.
- Toasts and inline save errors keep short two-clause forms ("Could not save. Try
  again.") — they are responses to an action, not descriptions of a concept, and
  they are exempt in the guard by shape rather than by namespace.

**Source:** PO request, 2026-11-09.
