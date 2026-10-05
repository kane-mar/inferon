export const CenteredHero = (props: {
  banner: React.ReactNode;
  title: React.ReactNode;
  description: string;
  buttons: React.ReactNode;
}) => (
  <>
    <div className="flex justify-center">{props.banner}</div>

    <h1 className="
      mx-auto mt-6 max-w-4xl text-center text-4xl font-semibold tracking-tighter
      text-balance
      sm:text-5xl
      md:text-7xl
    "
    >
      {props.title}
    </h1>

    <p className="
      mx-auto mt-6 max-w-2xl text-center text-lg text-pretty
      text-muted-foreground
      md:text-xl
    "
    >
      {props.description}
    </p>

    <div className="
      mt-10 flex flex-wrap justify-center gap-3
      max-sm:flex-col max-sm:items-center
    "
    >
      {props.buttons}
    </div>
  </>
);
