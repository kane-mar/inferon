// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { monotonePath, ticks } from '@/libs/chart';

/**
 * ITEM-152: the smooth area line, per the chart style.
 *
 * One measure, no legend, no data dots at rest: a 1.5px royal-blue stroke over a
 * flat translucent fill, three gridlines, four date labels. The curve is monotone,
 * so a quiet day next to a peak slopes instead of dipping below the baseline.
 *
 * Interaction stays on top of it: hovering a column shows the day and its value.
 */
export const AreaChart = (props: {
  values: number[];
  /** The previous window, drawn as a dashed neutral line when comparing. */
  previous?: number[];
  labels: string[];
  format: (value: number) => string;
  height?: number;
}) => {
  const width = 760;
  const height = props.height ?? 200;
  const padLeft = 52;
  const padRight = 4;
  const padTop = 10;
  const padBottom = 26;
  const innerW = width - padLeft - padRight;
  const plotH = height - padTop - padBottom;

  const max = Math.max(...props.values, ...(props.previous ?? []), 0);
  const [, mid, top] = ticks(max);
  const path = monotonePath(props.values, top, plotH);
  const previousPath = monotonePath(props.previous ?? [], top, plotH);

  // The geometry is in a 0..1 box scaled by CSS: shift and scale into the plot.
  const transform = `translate(${padLeft} ${padTop}) scale(${innerW} ${plotH})`;
  const area = path === ''
    ? ''
    : `${path} L1 1 L0 1 Z`;

  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      className="w-full"
      role="img"
      aria-label="Requests per day"
    >
      {[0, mid, top].map((tick, index) => {
        const y = padTop + plotH - (tick / (top || 1)) * plotH;
        return (
          <g key={tick}>
            <line
              x1={padLeft}
              x2={width - padRight}
              y1={y}
              y2={y}
              stroke="var(--chart-grid)"
              strokeWidth={1}
            />
            <text x={padLeft - 8} y={y + (index === 0 ? 0 : 3)} textAnchor="end" fontSize={10} fill="var(--chart-axis-ink)">
              {props.format(tick)}
            </text>
          </g>
        );
      })}

      <g transform={transform} style={{ vectorEffect: 'non-scaling-stroke' }}>
        <path d={area} fill="var(--chart-area)" />
        {previousPath && (
          <path
            d={previousPath}
            fill="none"
            stroke="var(--chart-axis-ink)"
            strokeWidth={1}
            strokeDasharray="3 3"
            vectorEffect="non-scaling-stroke"
          />
        )}
        <path
          d={path}
          fill="none"
          stroke="var(--chart-line)"
          strokeWidth={1.5}
          strokeLinejoin="round"
          vectorEffect="non-scaling-stroke"
        />
      </g>

      {props.labels.map((label, index) => {
        const step = Math.max(1, Math.ceil(props.labels.length / 4));
        return index % step === 0
          ? (
              <text
                key={label}
                x={padLeft + (props.labels.length > 1 ? (index / (props.labels.length - 1)) * innerW : innerW / 2)}
                y={height - 8}
                textAnchor="middle"
                fontSize={10}
                fill="var(--chart-axis-ink)"
              >
                {label.slice(5)}
              </text>
            )
          : null;
      })}
    </svg>
  );
};
