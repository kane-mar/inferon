// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * Where the interface sends a reader who wants a term explained.
 *
 * The rule this exists for: the interface states data and nothing else. If a
 * label needs a sentence to be understood, the label is wrong — and the sentence
 * belongs in the guides, not on the page. A page therefore carries at most one
 * `DocsLink` per section, and only for a concept a reader may genuinely not know.
 *
 * `NEXT_PUBLIC_DOCS_URL` is the hosted guides site (ITEM-112). Until a host is
 * chosen there is no URL, and `docsHref` returns null so the link is not
 * rendered at all: a dead link is worse than no link, and an empty page is not a
 * place to explain itself.
 */

/** A guide that exists in the repository, e.g. `config/04-auto-router-and-threshold`. */
export const docsHref = (guide: string): string | null => {
  const base = process.env.NEXT_PUBLIC_DOCS_URL?.trim();

  if (!base) {
    return null;
  }

  return `${base.replace(/\/$/, '')}/${guide.replace(/^\//, '')}.md`.replace('.md', '');
};

/** Whether a link is worth rendering at all (the host is configured). */
export const hasDocs = (): boolean => docsHref('using/first-request') != null;
