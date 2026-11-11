// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { cn } from '@/utils/Helpers';

/**
 * A centered, max-width page band — the shared layout primitive for both the
 * marketing page and the dashboard.
 */
export const Section = (props: {
  children: React.ReactNode;
  title?: string;
  subtitle?: string;
  description?: string;
  className?: string;
}) => (
  <div className={cn('@container px-3 py-20', props.className)}>
    {(props.title || props.subtitle || props.description) && (
      <div className="mx-auto mb-12 max-w-3xl text-center">
        {props.subtitle && (
          <div className="
            inline-flex items-center gap-2 text-xs font-medium text-accent-ink
          "
          >
            <span aria-hidden className="h-px w-6 bg-accent" />
            {props.subtitle}
          </div>
        )}

        {props.title && (
          <div className="
            mt-2 text-3xl font-semibold tracking-tight
            md:text-4xl
          "
          >
            {props.title}
          </div>
        )}

        {props.description && (
          <div className="mt-2 text-lg text-muted-foreground">
            {props.description}
          </div>
        )}
      </div>
    )}

    <div className="mx-auto max-w-5xl">{props.children}</div>
  </div>
);
