// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/** The standard page title, description and (optional) page-level action. */
export const PageHeader = (props: {
  title: React.ReactNode;
  description?: React.ReactNode;
  /** One control for the whole page — a window chip, a filter, a refresh. */
  action?: React.ReactNode;
}) => (
  <div className="mb-8 flex flex-wrap items-start justify-between gap-4">
    <div>
      <h1 className="text-2xl font-semibold">{props.title}</h1>

      {props.description && (
        <p className="text-sm font-medium text-muted-foreground">
          {props.description}
        </p>
      )}
    </div>
    {props.action}
  </div>
);
