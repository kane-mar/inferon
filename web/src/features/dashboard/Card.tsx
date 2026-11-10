// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import type { IconName } from './Icon';
import { Icon } from './Icon';

export const Card = (props: {
  title: string;
  /** A line icon for the section; decorative, so the title carries the meaning. */
  icon?: IconName;
  action?: React.ReactNode;
  children: React.ReactNode;
}) => (
  <section className="
    overflow-hidden rounded-lg border bg-card text-card-foreground
  "
  >
    <header className="
      flex items-center justify-between gap-3 border-b px-5 py-3.5
    "
    >
      <h2 className="flex items-center gap-2 font-semibold text-balance">
        {props.icon && (
          <Icon name={props.icon} size={16} className="text-muted-foreground" />
        )}
        {props.title}
      </h2>
      {props.action}
    </header>

    <div className="p-5">{props.children}</div>
  </section>
);
