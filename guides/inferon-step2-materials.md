# Inferon — Step 2 materials: first messages + interview guide

This is the working kit for turning the Step 1 leads (HN Ask HN threads, r/SaaS
posts, X accounts once you are logged in) into real conversations. Every message
leads with the cost audit as *value*, never with a pitch.

---

## Part A — The first message (use this, not a pitch)

**The rule:** your first message gives something concrete about *them*. It does not
ask for anything. Every opener below names one finding from the cost audit and ends
with a question they can answer, not a link.

### Template 1 — reply on an "Ask HN / cost help" thread (best return for your time)

> "Happy to share what I see when I look at AI bills directly. What surprises
> founders most is not the total — it is not knowing *where* it goes: which model,
> which endpoint, which single key is eating the margin. One thing I find again and
> again: people paying GPT-4o prices for classification and extraction calls that
> `gpt-4o-mini` (or a cheap open model) does just as well. They also send the same
> big system prompt again on every request without caching it.
>
> If you are up for it, point your SDK at a base URL and key for a couple of days.
> I will send back a one-page cost audit showing exactly where the money goes —
> free, no obligation. The report is yours either way."

**Why it works:** it *shows* the checks (over-provisioning and cache misses) before
offering anything. "Free audit, keep it either way" is the Step 3 test. The ask — one
base URL — is small.

### Template 2 — reply to a specific "my bill doubled" comment

> "That 'it doubled' moment can usually be traced in about five minutes if you can
> see where the spend went. It is rarely 'usage went up'; it is usually 'this one
> model, key or feature changed'. I run free AI cost-leak audits. If you want, send
> me a sample and I will tell you exactly which call pattern is doing it."

**Why it works:** it names their exact problem ("doubled"), promises a specific
answer rather than a general one, and gives something (the audit) with no commitment.

### Template 3 — direct message (for a handle you found on X or HN)

> "I saw your post about [their specific bill or frustration]. I spend a lot of time
> inside AI traffic, and the leak is almost never where founders think it is. It is
> usually an uncached system prompt, or GPT-4o on simple tasks. Want a free cost
> audit? Point me at your proxy for a day and I will show you the exact line item.
> No product, no pitch — just the report."

**Why it works:** it refers to *their* post (peer, not vendor), leads with an
unexpected insight, and "no product, no pitch" is both disarming and true — the
report is the value.

---

## Part B — The interview guide (Step 2, once someone replies)

These questions test the refined hypothesis from Step 1:

> **"A founder with an AI SaaS product gets a bill they cannot explain, and will pay
> for the cost audit, then for the cost reduction."**

And the opposite case:

> **"Cost is already handled — I moved to 4o-mini early and I am fine."**

### The questions (open, not leading — ask them in this order)

1. **Opening:** "What is your AI bill these days, roughly, and do you know off the
   top of your head where it goes?" *(Gets the number and tests whether they can see
   it.)*

2. **Bill shock:** "Has there been a month where you opened the dashboard and the
   number surprised you? What did you do?" *(Tests Finding 5, the event that opens
   the conversation.)*

3. **Explaining the spend:** "If I asked which *one* model or feature of your app
   drives your spend, could you answer?" *(Tests our strongest finding — almost
   nobody can.)*

4. **Model choice:** "How do you decide which model each part of your product uses?
   Did you ever think 'this is probably overkill' and move it down?" *(Tests Findings
   1 and 2.)*

5. **Caching:** "Do you use prompt caching at all? *(pause)* A lot of people send the
   same system prompt on every call without realising what it costs." *(Tests Finding
   3 and whether they know the concept.)*

6. **The value test:** "If I gave you a one-page report that named the exact call
   pattern wasting $X a month, would that be useful? What would you do with it?"
   *(Tests whether the report itself is worth paying for.)*

7. **The reality check:** "Honestly, is inference cost a real problem for you right
   now, or is it basically handled?" *(Finds the 'already solved' people early, so we
   do not build for them.)*

### How to score a conversation (30 seconds, afterwards)

| Signal | Meaning |
|---|---|
| They cannot say where the spend goes | Strong fit for Finding 5 — lead with the cost audit |
| They can say it, and already moved to a cheaper model | Opposite case — they may not be a buyer |
| They react to "uncached system prompt" with "wait, what?" | Finding 3 is news to them — high value |
| They ask "but will quality drop?" | Finding 6 objection — answer with the error and outcome data |
| They say "just tell me the number" | Show, do not tell — invite them to send traffic |

---

## Part C — The 2 hours a week routine

**Week 1 (2 hours):**
1. Reply to the 4 Ask HN threads (40995465, 36705621, 44554293, 46838390) with
   Template 1 — about 20 minutes. Each one has dozens of founders who named
   themselves.
2. Read the `teilom` thread (46838390) and `j_filipe`'s CostLens — about 30 minutes.
   It sharpens our checks against someone who already shipped a similar router.
3. Log into X once in the visible browser (when you have a minute). That saves the
   session so I can drive it later.

**Week 2 (2 hours):**
- Follow up on every reply with Template 2 or 3, and run the interview (Part B) with
  the first 3–5 people who reply.

**Weeks 3–4:**
- Keep interviewing until about 20 conversations, and point the warm ones at the live
  proxy for a real cost audit (Step 3 — it already exists).

---

## Note on the X session (needed before X becomes a channel)

X search needs a login; we tested it. To make X a discovery channel we need a saved
session: open a visible browser, log in once, then I save the session and drive
search as you. It takes about a minute at the keyboard. Flag it whenever it suits
you — it does not block the HN work above.
