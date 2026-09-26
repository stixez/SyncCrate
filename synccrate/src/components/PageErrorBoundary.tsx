import { Component, type ReactNode } from "react";
import { AlertTriangle } from "lucide-react";
import { Button } from "./ui";

/**
 * A page that throws while rendering shows this instead of blanking the
 * whole window (sidebar included), and a page chunk that fails to load gets
 * a way to retry. Resets when you go to another page or game.
 */
export default class PageErrorBoundary extends Component<{ resetKey: string; children: ReactNode }, { error: Error | null }> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidUpdate(prev: { resetKey: string }) {
    if (prev.resetKey !== this.props.resetKey && this.state.error) this.setState({ error: null });
  }

  componentDidCatch(error: Error) {
    console.error("Page crashed:", error);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="max-w-[640px] mx-auto mt-16 border border-status-red/50 bg-status-red/[0.06] p-6 space-y-3">
        <p className="flex items-center gap-2 font-display font-semibold uppercase tracking-[0.06em] text-status-red">
          <AlertTriangle size={16} /> This page ran into a problem
        </p>
        <p className="text-sm text-txt-dim">
          Nothing was changed on your PC. Try again, or reload SyncCrate. If it keeps happening, please report it on GitHub with the text below.
        </p>
        <pre className="font-mono text-[11px] text-txt-muted whitespace-pre-wrap break-words max-h-40 overflow-y-auto">{String(this.state.error?.message ?? this.state.error)}</pre>
        <div className="flex gap-2">
          <Button size="sm" variant="primary" onClick={() => this.setState({ error: null })}>Try again</Button>
          <Button size="sm" onClick={() => window.location.reload()}>Reload SyncCrate</Button>
        </div>
      </div>
    );
  }
}
