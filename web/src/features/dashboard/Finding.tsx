import { Card } from './Card';

/** One named audit finding: what we detect, the headline, and the numbers. */
export const Finding = (props: {
  title: string;
  description: string;
  highlight: string;
  metrics: { label: string; value: string }[];
}) => (
  <Card
    title={props.title}
    action={<span className="text-lg font-semibold tabular-nums">{props.highlight}</span>}
  >
    <p className="text-sm text-muted-foreground">{props.description}</p>
    <dl className="mt-4 space-y-1.5 text-sm">
      {props.metrics.map(metric => (
        <div
          key={metric.label}
          className="flex items-baseline justify-between gap-3"
        >
          <dt className="text-muted-foreground">{metric.label}</dt>
          <dd className="truncate text-right tabular-nums">{metric.value}</dd>
        </div>
      ))}
    </dl>
  </Card>
);
