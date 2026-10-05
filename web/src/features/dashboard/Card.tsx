export const Card = (props: {
  title: string;
  action?: React.ReactNode;
  children: React.ReactNode;
}) => (
  <section className="rounded-lg border bg-card text-card-foreground shadow-sm">
    <header className="
      flex items-center justify-between gap-3 border-b px-4 py-3
    "
    >
      <h2 className="font-semibold">{props.title}</h2>
      {props.action}
    </header>

    <div className="p-4">{props.children}</div>
  </section>
);
