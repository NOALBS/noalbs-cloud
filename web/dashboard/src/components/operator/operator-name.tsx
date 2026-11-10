export function OperatorName({
  workspace,
  switcher,
  multi = true,
}: {
  workspace: string;
  switcher: string;
  multi?: boolean;
}) {
  return multi ? (
    <>
      <span className="font-semibold">{workspace}</span>
      <span className="font-normal text-muted-foreground"> / {switcher}</span>
    </>
  ) : (
    <span className="font-semibold">{switcher}</span>
  );
}
