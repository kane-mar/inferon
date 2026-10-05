export const FeatureCard = (props: {
  icon: React.ReactNode;
  title: string;
  children: React.ReactNode;
}) => (
  <div className="
    group rounded-lg border border-border bg-card p-5 transition-colors
    hover:border-accent
  "
  >
    <div className="
      size-12 rounded-md bg-linear-to-br from-accent to-chart-2 p-2
      [&_svg]:stroke-white [&_svg]:stroke-2
    "
    >
      {props.icon}
    </div>

    <div className="
      mt-3 font-mono text-sm font-semibold tracking-wide uppercase
    "
    >
      {props.title}
    </div>

    <div className="my-3 w-8 border-t border-accent" />

    <div className="mt-2 text-muted-foreground">{props.children}</div>
  </div>
);
