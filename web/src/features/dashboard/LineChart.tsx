// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

'use client';

import { useState } from 'react';
import { formatCompact, formatInt, formatMs } from '@/libs/dashboardMetrics';
import { money } from './money';

export type ValueFormat = 'int' | 'usd' | 'compact' | 'ms';

export type LineSeries = {
  name: string;
  /** Any CSS color (theme tokens like `var(--chart-1)` work). */
  color: string;
  values: (number | null)[];
  dashed?: boolean;
  area?: boolean;
};

const formatter
  = (kind: ValueFormat): ((value: number) => string) => {
    switch (kind) {
      case 'usd':
        return value => money(value);
      case 'ms':
        return value => formatMs(value);
      case 'compact':
        return formatCompact;
      case 'int':
        return formatInt;
    }
  };

/** Short axis label for a `YYYY-MM-DD` bucket. */
const shortDate = (date: string): string => date.slice(5).replace('-', '/');

const buildLine = (
  values: (number | null)[],
  xAt: (i: number) => number,
  yAt: (value: number) => number,
): string => {
  let path = '';
  let open = false;
  values.forEach((value, index) => {
    if (value == null) {
      open = false;
      return;
    }
    path += `${open ? 'L' : 'M'}${xAt(index).toFixed(1)} ${yAt(value).toFixed(1)} `;
    open = true;
  });
  return path.trim();
};

const buildArea = (
  values: (number | null)[],
  xAt: (i: number) => number,
  yAt: (value: number) => number,
  baseline: number,
): string => {
  if (values.some(value => value == null) || values.length === 0) {
    return '';
  }
  const points = values.map((value, index) => `${xAt(index).toFixed(1)} ${yAt(value as number).toFixed(1)}`);
  return `M${xAt(0).toFixed(1)} ${baseline.toFixed(1)} L${points.join(' L')} L${xAt(values.length - 1).toFixed(1)} ${baseline.toFixed(1)} Z`;
};

export const LineChart = (props: {
  labels: string[];
  series: LineSeries[];
  valueFormat: ValueFormat;
  height?: number;
}) => {
  const format = formatter(props.valueFormat);
  const [hover, setHover] = useState<number | null>(null);

  const width = 760;
  const height = props.height ?? 224;
  const padLeft = 58;
  const padRight = 14;
  const padTop = 12;
  const padBottom = 30;
  const innerW = width - padLeft - padRight;
  const innerH = height - padTop - padBottom;
  const count = props.labels.length;

  const observed = props.series.flatMap(s => s.values).filter((value): value is number => value != null);
  const max = observed.length > 0 ? Math.max(...observed) : 0;
  const yMax = max > 0 ? max * 1.12 : 1;

  const xAt = (index: number): number =>
    padLeft + (count <= 1 ? innerW / 2 : (index / (count - 1)) * innerW);
  const yAt = (value: number): number => padTop + innerH - (value / yMax) * innerH;
  const baseline = padTop + innerH;

  const ticks = [0, 0.25, 0.5, 0.75, 1].map(fraction => fraction * yMax);
  const labelStep = Math.max(1, Math.ceil(count / 7));

  const move = (clientX: number, target: SVGRectElement): void => {
    const rect = target.getBoundingClientRect();
    const relative = ((clientX - rect.left) / rect.width) * width;
    const index = Math.round(((relative - padLeft) / innerW) * (count - 1));
    setHover(Math.min(count - 1, Math.max(0, index)));
  };

  return (
    <div className="w-full">
      {props.series.length > 1 && (
        <div className="mb-2 flex flex-wrap gap-4 text-xs text-muted-foreground">
          {props.series.map(item => (
            <span key={item.name} className="inline-flex items-center gap-1.5">
              <span
                className="inline-block size-2 rounded-full"
                style={{ backgroundColor: item.color }}
              />
              {item.name}
            </span>
          ))}
        </div>
      )}

      <div className="relative">
        <svg
          viewBox={`0 0 ${width} ${height}`}
          className="w-full"
          role="img"
          aria-label="Usage over time"
          onMouseLeave={() => setHover(null)}
        >
          {ticks.map(tick => (
            <g key={tick}>
              <line
                x1={padLeft}
                x2={width - padRight}
                y1={yAt(tick)}
                y2={yAt(tick)}
                stroke="var(--border)"
                strokeWidth={1}
              />
              <text
                x={padLeft - 8}
                y={yAt(tick)}
                textAnchor="end"
                dominantBaseline="middle"
                className="fill-muted-foreground"
                fontSize={10}
              >
                {format(tick)}
              </text>
            </g>
          ))}

          {props.labels.map((label, index) =>
            index % labelStep === 0 || index === count - 1
              ? (
                  <text
                    key={label}
                    x={xAt(index)}
                    y={height - 8}
                    textAnchor="middle"
                    className="fill-muted-foreground"
                    fontSize={10}
                  >
                    {shortDate(label)}
                  </text>
                )
              : null)}

          {hover != null && (
            <line
              x1={xAt(hover)}
              x2={xAt(hover)}
              y1={padTop}
              y2={baseline}
              stroke="var(--border)"
              strokeWidth={1}
            />
          )}

          {props.series.map(item => (
            <g key={item.name}>
              {item.area && (
                <path
                  d={buildArea(item.values, xAt, yAt, baseline)}
                  fill={item.color}
                  fillOpacity={0.12}
                  stroke="none"
                />
              )}
              <path
                d={buildLine(item.values, xAt, yAt)}
                fill="none"
                stroke={item.color}
                strokeWidth={2}
                strokeLinejoin="round"
                strokeLinecap="round"
                strokeDasharray={item.dashed ? '5 4' : undefined}
              />
              <circle
                cx={xAt(hover ?? 0)}
                cy={yAt(item.values[hover ?? 0] ?? 0)}
                r={hover != null && item.values[hover] != null ? 3.5 : 0}
                fill={item.color}
                stroke="var(--card)"
                strokeWidth={2}
              />
            </g>
          ))}

          <rect
            x={padLeft}
            y={padTop}
            width={innerW}
            height={innerH}
            fill="transparent"
            onMouseMove={event => move(event.clientX, event.currentTarget)}
          />
        </svg>

        {hover != null && (
          <div
            className="
              pointer-events-none absolute top-0 z-10 -translate-x-1/2
              rounded-md border bg-popover px-2.5 py-1.5 text-xs
              text-popover-foreground shadow-md
            "
            style={{ left: `${(xAt(hover) / width) * 100}%` }}
          >
            <div className="mb-0.5 font-medium">{props.labels[hover]}</div>
            {props.series.map(item => (
              <div key={item.name} className="flex items-center gap-2">
                <span
                  className="inline-block size-1.5 rounded-full"
                  style={{ backgroundColor: item.color }}
                />
                <span className="text-muted-foreground">{item.name}</span>
                <span className="ml-auto tabular-nums">
                  {item.values[hover] == null ? '—' : format(item.values[hover] as number)}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};
