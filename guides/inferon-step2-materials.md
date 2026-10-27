# Inferon — Step 2 Materials: value-first outreach + interview guide

This is the operating kit for turning the Step 1 leads (HN Ask HN threads,
r/SaaS posts, X handles once logged in) into real conversations — leading with
the leak report as *value*, never with a pitch.

---

## Part A — The value-first opener (use this, not a pitch)

**The rule:** your first message must give something concrete and specific *about
them*, not ask for something. Every opener below leads with one named finding
from the leak report and ends with an *answerable* question, not a link.

### Template 1 — reply on an "Ask HN / cost-help" thread (highest ROI)

> "Happy to share what I see when I look at LLM bills directly. The thing that
> surprises founders most isn't the total — it's the *attribution*: which model,
> which endpoint, which single key is eating the margin. One thing I routinely
> find: people paying GPT-4o prices for classification/extraction calls that
> `gpt-4o-mini` (or a cheap open model) does identically — and re-sending a big
> un-cached system prompt on every request.
>
> If you're open to it, point your SDK at a base URL + key for a couple days and
> I'll send back a one-page leak report showing exactly where it's going — free,
> no obligation. You keep the report either way."

**Why it works:** it *demonstrates* the linter (names over-provisioning + cache
miss) before offering anything; the "free audit, keep it either way" is the
Step 3 test; the ask (change a base URL) is low-friction.

### Template 2 — reply to a specific "my bill doubled" comment

> "That '$X doubled' moment is usually traceable in about five minutes if you
> can see attribution — it's rarely 'usage went up', it's usually 'this one
> model/key/feature changed'. I run free LLM cost-leak audits; if you want, send
> me a sample and I'll tell you precisely which call pattern is doing it."

**Why it works:** names their exact pain ("doubled"), promises specificity not
generality, gives value (the audit) with zero commitment.

### Template 3 — DM version (for a handle you've identified on X/HN)

> "Saw your post about [their specific bill/frustration]. I spend a lot of time
> inside LLM traffic and the leak is almost never where founders think it is —
> it's usually an un-cached system prompt or GPT-4o on simple tasks. Want a free
> leak report? Point me at your proxy for a day and I'll show you the exact line
> item. No product, no pitch — just the report."

**Why it works:** references *their* post (peer, not vendor), leads with a
counterintuitive insight, and the "no product, no pitch" line is disarming and
true — the report *is* the value.

---

## Part B — The interview guide (Step 2, once someone engages)

These test the *refined* hypothesis from Step 1:

> **"A founder with an LLM SaaS wrapper gets bill shock they can't explain, and
> will pay for the leak-report → then the cost-reduction."**

And the counter-hypothesis:

> **"Cost is already solved — I downgraded to 4o-mini early and I'm fine."**

### The hypothesis questions (non-leading, ask in order)

1. **Entry:** "What's your LLM bill these days, roughly, and do you know off the
   top of your head where it goes?" *(Gets the number + tests visibility.)*

2. **Bill shock:** "Has there been a month where you opened the dashboard and
   the number surprised you? What did you do?" *(Tests Finding 5 — the entry
   event.)*

3. **Attribution:** "If I asked you which *one* model or feature of your app
   drives your spend, could you answer?" *(Tests the core wedge — nobody can.)*

4. **Over-provisioning:** "How do you decide which model each part of your
   product uses — did you ever go 'this is probably overkill' and downgrade?"
   *(Tests Findings 1–2.)*

5. **Caching:** "Do you use prompt caching at all? *(pause)* — a lot of people
   re-send the same system prompt every call and don't realize what it costs."
   *(Tests Finding 3 + the education gap.)*

6. **The value test:** "If I handed you a one-page report that named the exact
   call pattern leaking $X/month, would that be useful to you? What would you
   actually do with it?" *(Tests willingness-to-pay for the report itself.)*

7. **The counter-hypothesis filter:** "Honestly — is inference cost even a real
   problem for you right now, or is it basically handled?" *(Catches the
   'already solved' folks so you don't over-build.)*

### How to score a conversation (30 seconds, after)

| Signal | Meaning |
|---|---|
| They can't name where spend goes | Strong Finding 5 fit — lead with the leak report |
| They *can* name it and downgraded already | Counter-hypothesis — they may not be your buyer |
| They react to "un-cached system prompt" with "wait, what?" | Finding 3 education gap — high value |
| They ask "but will quality drop?" | Finding 6 trust objection — counter with the error/outcome data |
| They say "just tell me the number" | Show, don't tell — invite them to run traffic |

---

## Part C — The 2 hrs/week operating rhythm

**Week 1 (2 hrs):**
1. Reply to the 4 Ask HN threads (40995465, 36705621, 44554293, 46838390) using
   Template 1 — ~20 min. These are *dozens* of self-identified founders each.
2. Read the `teilom` thread (46838390) + `j_filipe`'s CostLens — ~30 min. This
   sharpens your linter against someone who already shipped your router idea.
3. Log into X once in the headed browser (when you have a minute) — saves the
   session for me to drive later.

**Week 2 (2 hrs):**
- Follow up on every reply with Template 2/3, run the interview (Part B) on the
  first 3–5 people who engage.

**Weeks 3–4:**
- Keep interviewing to ~20 conversations; point the warm ones at the live proxy
  for a real leak-report audit (Step 3 — it already exists).

---

## Note on the X session (unblock when ready)

X search is login-walled (tested). To make it a discovery channel, we need a
saved login session. The process: open a headed stealth browser → you log in
once → I save the session → I drive search as you. This needs ~1 minute of your
time at the keyboard. Flag it whenever convenient; it's not blocking the HN
work above.
