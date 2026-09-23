import { useState, type ReactNode } from "react";
import { useGameArt } from "../../hooks/useGameArt";
import type { GameArtKind } from "../../lib/commands";
import { cx } from "./cx";

/**
 * Game artwork image (Steam art or a custom cover) that fades in once loaded.
 * Renders `fallback` when the game has no art, art is turned off, or the image
 * fails, so every caller keeps its generated tile as the base look.
 */
export default function GameArt({
  gameId,
  kind,
  className,
  imgClassName,
  fallback = null,
  children,
}: {
  gameId: string;
  kind: GameArtKind;
  /** Wrapper classes (size/position; defaults to `relative`). Always `overflow-hidden`. */
  className?: string;
  imgClassName?: string;
  fallback?: ReactNode;
  /** Overlays drawn above the image (gradients, badges). Only shown with art. */
  children?: ReactNode;
}) {
  const url = useGameArt(gameId, kind);
  const [loaded, setLoaded] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);

  if (!url || failed === url) return <>{fallback}</>;

  return (
    // Position comes from the caller: a hardcoded `relative` would beat an
    // `absolute inset-0` passed in (Tailwind emits `relative` later) and collapse it.
    <div className={cx("overflow-hidden", className ?? "relative")}>
      <img
        src={url}
        alt=""
        draggable={false}
        onLoad={() => setLoaded(url)}
        onError={() => setFailed(url)}
        className={cx(
          "absolute inset-0 w-full h-full object-cover transition-opacity duration-500",
          loaded === url ? "opacity-100" : "opacity-0",
          imgClassName,
        )}
      />
      {children}
    </div>
  );
}
