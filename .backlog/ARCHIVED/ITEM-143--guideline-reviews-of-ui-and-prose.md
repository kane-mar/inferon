## ITEM-143: Guideline reviews of the interface and the prose

**As** the maintainer
**I want** an outside checklist applied to what we shipped
**So that** a house style we invented is not mistaken for a standard we met

**Requested** 2026-11-09: "install skill vercel-labs/agent-skills … run
web-design-guidelines against the dashboard and writing-guidelines against the
guides."

**Skills installed** (9, from `vercel-labs/agent-skills`, pinned at `1a2c79d`):
web-design-guidelines, writing-guidelines, vercel-react-best-practices,
vercel-composition-patterns, vercel-react-view-transitions,
vercel-react-native-skills, deploy-to-vercel, vercel-cli-with-tokens,
vercel-optimize.

## Web Interface Guidelines — findings, and what was fixed

Fixed (7):

- `components/ui/buttonVariants.ts` — `transition-all` animated every property;
  now `transition-colors`.
- `AutoRefresh` — the live dot pulses with no reduced-motion variant; now
  `motion-reduce:animate-none`.
- Root layout — no `color-scheme`, so dark mode kept white scrollbars and light
  native form controls. Added, plus `themeColor` for the mobile browser chrome.
  A first attempt put `themeColor` on `metadata`, where this Next version
  ignores it: it must live on the `viewport` export.
- `KeysPanel` — **Revoke fired on one click**, ending a key irreversibly. It now
  arms first, matching the payload purge's two-step.
- Five surfaces printed dates via `slice(0, 16).replace('T',' ')`. `2026-11-10
  09:48` reads as 11 September to an American reader. New `libs/datetime.ts`
  formats with `Intl` (fixed to UTC, zone labelled) and has tests; `title`
  attributes keep the exact ISO value, as the product's own rule allows.
- No skip link: keyboard users tabbed through nine nav links on every page. Added,
  visible only when focused, targeting `#main`.
- The three controls added in ITEM-141 had no `name`, no explicit focus ring and
  let spellcheck run on a numeric field.

Checked and compliant: no icon-only button lacks `aria-label`; no `div`/`span`
carries a click handler; heading order is `h1` → `h2`; table numbers use
`tabular-nums`; filters live in the URL; empty states are handled; no
`user-scalable=no`, no `onPaste` blocking.

Not changed, with reasons: `outline-none` stays on the shared input and button
styles because each pairs it with a `focus-visible` ring (the rule targets the
unpaired case); lists are bounded (≤50 rows) so virtualization would add a
dependency for nothing.

## Writing Guidelines — findings, and the decision this needs

**One systemic finding, deliberately not "fixed" by a machine: 172 em dashes** in
the published guides (the other 6 sit inside quoted API error bodies and cannot
change). The guideline bans them outright. This corpus uses them consistently as
the appositive mark, and `check-guides.py` already enforces the rules that matter
to a reader (mean sentence under 25 words, five troubleshooting sections, dated
footers). Rewriting 172 marks across 30 pages by regex would trade a documented
house style for a worse one, so it is a decision, not a fix:

- adopt the rule (I convert the ~64 colon-shaped ones by hand and rewrite the
  rest), or
- record the deviation: this project uses em dashes, and says so.

**Also found, judged not to be defects:** 13 uses of "simple" are the router's own
term for a request class ("a request that scores simple"), and 11 of "just" mean
"only" ("retrying earlier just extends the wait"). The guideline's ban targets
marketing filler, and replacing the domain term would make the docs less precise.

**Not applicable:** `meta.contentType`, `meta.navLabel`, `<Steps/>`, Vercel model
strings and `vercel.com/d?` deep links are Astro/Vercel-docs conventions; this site
is MkDocs with Markdown pages.

**Verified live** after the changes: landing and four dashboard pages 200, two
`theme-color` tags, `color-scheme` on the html element, `id="main"` present, keys
page renders `05 Oct 2026` style dates, real completion 200, 0 web errors, 0 proxy
errors. tsc, eslint, i18n-check clean.

**Source:** PO request, 2026-11-09.
