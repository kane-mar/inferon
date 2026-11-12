// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

/**
 * ITEM-152: the chart card, from the chart style.
 *
 * A flat tinted surface with no border and no shadow, so the figure is the only
 * thing on it. The title sits on one baseline with the aggregate it belongs to: the
 * measure in dark ink, the number in muted ink, because the shape is the subject and
 * the total is the caption.
 */
export const ChartCard = (props: {
  /** e.g. "Cost", "API requests". */
  measure: string;
  /** The total for the window, already formatted. */
  aggregate?: string;
  /** A control on the right (a segmented pill, a window chip). */
  action?: React.ReactNode;
  children: React.ReactNode;
}) => (
  <section className="rounded-chart bg-chart-surface p-5">
    <header className="flex items-baseline justify-between gap-3">
      <div className="flex items-baseline gap-1.5">
        <h2 className="text-xs font-semibold text-chart-title-ink">{props.measure}</h2>
        {props.aggregate && (
          <span className="text-xs text-chart-muted-ink tabular-nums">{props.aggregate}</span>
        )}
      </div>
      {props.action}
    </header>
    <div className="mt-4">{props.children}</div>
  </section>
);
