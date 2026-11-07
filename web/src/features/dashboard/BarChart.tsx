// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

export const BarChart = (props: {
  rows: { label: string; value: number; display: string }[];
  empty: string;
}) => {
  const max = props.rows.reduce((top, row) => Math.max(top, row.value), 0);
  if (props.rows.length === 0 || max <= 0) {
    return <p className="text-sm text-muted-foreground">{props.empty}</p>;
  }
  return (
    <ul className="space-y-3">
      {props.rows.map(row => (
        <li key={row.label}>
          <div className="flex items-baseline justify-between gap-3 text-sm">
            <span className="truncate font-medium">{row.label}</span>
            <span className="shrink-0 text-muted-foreground tabular-nums">
              {row.display}
            </span>
          </div>
          <div className="mt-1 h-2 w-full overflow-hidden rounded-full bg-muted">
            <div
              className="h-full rounded-full bg-chart-1"
              style={{ width: `${Math.max(2, (row.value / max) * 100)}%` }}
            />
          </div>
        </li>
      ))}
    </ul>
  );
};
