// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { docsHref } from '@/libs/docs';

/**
 * The one way an explanation may appear near the data: a quiet link out.
 *
 * Renders nothing when the guides are not hosted, so a page never carries a
 * broken or placeholder link — and never substitutes prose for it either.
 */
export const DocsLink = (props: { guide: string; label: string }) => {
  const href = docsHref(props.guide);

  if (!href) {
    return null;
  }

  return (
    <a
      href={href}
      className="
        text-xs font-medium text-accent-ink underline-offset-2
        hover:underline
      "
      target="_blank"
      rel="noopener noreferrer"
    >
      {props.label}
    </a>
  );
};
