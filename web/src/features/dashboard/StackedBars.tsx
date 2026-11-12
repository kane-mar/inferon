// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

import { barSlots, stackHeights, ticks } from '@/libs/chart';

/**
 * ITEM-152: the daily stacked column, per the chart style.
 *
 * What makes it quiet: three gridlines and no vertical ones, a third of each slot
 * left empty, the top corners rounded and the seam between segments flat, and a
 * sliver on any day with traffic so the cadence stays unbroken.
 *
 * Colour comes in pairs — a warm duo for money, a cool duo for tokens — because two
 * segments of one measure are a comparison, not two categories.
 */
const PALETTE = {
  cost: { bottom: 'var(--chart-cost)', top: 'var(--chart-cost-soft)' },
  tokens: { bottom: 'var(--chart-tokens)', top: 'var(--chart-tokens-soft)' },
  requests: { bottom: 'var(--chart-line)', top: 'var(--chart-tokens-soft)' },
} as const;

export type BarPalette = keyof typeof PALETTE;

export const StackedBars = (props: {
  /** One entry per day, oldest first. `[bottom, top]`. */
  values: [number, number][];
  labels: string[];
  palette: BarPalette;
  /** Formats an axis value and the readout. */
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
  const colours = PALETTE[props.palette];

  // A day is the identity of a column, so its date is the key.
  const days = props.values.map((value, index) => ({
    label: props.labels[index] ?? '',
    key: props.labels[index] ?? `day-${index}`,
    value,
  }));
  const totals = props.values.map(([bottom, top]) => bottom + top);
  const max = Math.max(...totals, 0);
  const [zero, mid, top] = ticks(max);
  const { pitch, width: barW } = barSlots(props.values.length, innerW);

  const yAt = (value: number): number => padTop + plotH - (value / (top || 1)) * plotH;
  // Four labels across, centred under their bar, whatever the day count.
  const labelStep = Math.max(1, Math.ceil(props.values.length / 4));

  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      className="w-full"
      role="img"
      aria-label={`${props.palette} per day`}
    >
      {/* Three gridlines, the bottom one doubling as the x spine. */}
      {[zero, mid, top].map((tick, index) => (
        <g key={tick}>
          <line
            x1={padLeft}
            x2={width - padRight}
            y1={yAt(tick)}
            y2={yAt(tick)}
            stroke="var(--chart-grid)"
            strokeWidth={1}
          />
          <text
            x={padLeft - 8}
            y={yAt(tick) + (index === 0 ? 0 : 3)}
            textAnchor="end"
            fontSize={10}
            fill="var(--chart-axis-ink)"
          >
            {props.format(tick)}
          </text>
        </g>
      ))}

      {days.map(({ key, value: [bottom, topValue] }, index) => {
        const [bottomH, topH] = stackHeights([bottom, topValue], top, plotH);
        if (bottomH === 0 && topH === 0) {
          return null;
        }
        const x = padLeft + index * pitch + (pitch - barW) / 2;
        const bottomY = padTop + plotH - bottomH;
        const topY = bottomY - topH;
        return (
          <g key={key}>
            {bottomH > 0 && (
              <rect
                x={x}
                y={bottomY}
                width={barW}
                height={bottomH}
                fill={colours.bottom}
                rx={topH > 0 ? 0 : 1.5}
              />
            )}
            {topH > 0 && (
              <rect
                x={x}
                y={topY}
                width={barW}
                height={topH}
                fill={colours.top}
                rx={1.5}
              />
            )}
          </g>
        );
      })}

      {props.labels.map((label, index) =>
        index % labelStep === 0
          ? (
              <text
                key={label}
                x={padLeft + index * pitch + pitch / 2}
                y={height - 8}
                textAnchor="middle"
                fontSize={10}
                fill="var(--chart-axis-ink)"
              >
                {label.slice(5)}
              </text>
            )
          : null,
      )}
    </svg>
  );
};
