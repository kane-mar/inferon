/** The standard page title + description for a dashboard surface. */
export const PageHeader = (props: {
  title: React.ReactNode;
  description?: React.ReactNode;
}) => (
  <div className="mb-8">
    <h1 className="text-2xl font-semibold">{props.title}</h1>

    {props.description && (
      <p className="text-sm font-medium text-muted-foreground">
        {props.description}
      </p>
    )}
  </div>
);
