export const CTABanner = (props: {
  title: string;
  description: string;
  buttons: React.ReactNode;
}) => (
  <div className="
    rounded-lg bg-accent px-6 py-12 text-center text-accent-foreground
  "
  >
    <div className="
      text-3xl font-semibold tracking-tight
      md:text-4xl
    "
    >
      {props.title}
    </div>

    <div className="mt-2 text-lg font-medium opacity-80">
      {props.description}
    </div>

    <div className="mt-6">{props.buttons}</div>
  </div>
);
