# Answers that are empty, refused or cut short

You paid for the request and the answer is not usable. The trace tells you which of the
four it was.

## Symptom

One of these, with a `200` status:

- `choices[0].message.content` is `""` or missing;
- `finish_reason` is `"length"` and the text stops mid-sentence;
- the model declined ("I can't help with that");
- the answer arrived but the outcome on the trace is not `ok`.

## What it means

| Trace outcome | What happened | Where it shows |
|---|---|---|
| `empty` | a `200` with no content — usually the token budget was consumed by reasoning | **Quality → Empty rate** |
| `truncated` | the answer hit `max_tokens` before finishing | **Quality → Cut short** |
| `refused` | the model declined | **Quality → Refusal rate** |
| `error` | the provider failed, timed out or was unreachable | **Quality → Errors by cause** |

**Empty is the surprise one.** On reasoning models, `reasoning_content` (the model's
internal working) is billed as completion tokens. A `max_tokens` that looks generous can be
entirely consumed by reasoning, leaving nothing for the answer. DeepSeek's responses make
this visible:

```json
"usage": {"completion_tokens": 18, "completion_tokens_details": {"reasoning_tokens": 16}}
```

Sixteen of eighteen tokens went to reasoning.

## Fix

1. **Read `finish_reason` and `usage` on the response**, not just the content. `length` +
   no content is the reasoning case above.
2. **Raise `max_tokens`** for reasoning models: budget for reasoning *plus* the answer. A
   short answer often needs 300+ tokens of thinking.
3. **Or ask for less thinking**: a simpler prompt, or a non-reasoning model for trivial
   work.
4. **A refusal is content, not an error.** Reword or add instructions; retrying the same
   prompt usually refuses again.
5. **For `error`,** follow the class
   ([429](../troubleshooting/too-many-requests.md),
   [5xx](../troubleshooting/upstream-5xx.md)).

## How to confirm it worked

- **Quality** shows the truncation and empty rates falling in the same window.
- A new request's trace has `outcome: ok` and `truncated: false`.
- The response's `finish_reason` is `"stop"`, not `"length"`.

## When to stop and ask

- `max_tokens` is large, the answer is still empty, and `reasoning_tokens` is near the
  limit every time — the model is spending everything on thinking.
- Answers are cut short although the prompt does not ask for anything long.
- The refusal rate is high for prompts that worked last week: something upstream changed
  (a model update, a system prompt, a policy) — compare the two prompt versions in
  **Datasets → Prompts**.

_Last verified 2026-11-06 against the live host (master acdaffc)._
