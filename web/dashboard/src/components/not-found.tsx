import { Link } from "@tanstack/react-router";
import { Button } from "@/components/ui/button";
import { Wordmark } from "./wordmark";

export function NotFound() {
  return (
    <main className="flex min-h-dvh flex-col items-center justify-center gap-5 px-4 text-center">
      <Wordmark />
      <div className="flex flex-col gap-2">
        <h1 className="text-2xl font-bold">Nothing here</h1>
        <p className="text-muted-foreground">
          This page or workspace doesn’t exist, or you’re not a member of it.
        </p>
      </div>
      <Button asChild className="h-11 rounded-xl">
        <Link to="/">Go to my workspaces</Link>
      </Button>
    </main>
  );
}
