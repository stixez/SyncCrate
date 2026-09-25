import { useEffect, useState } from "react";
import { AlertTriangle, ChevronDown, ChevronRight, ExternalLink, Stethoscope, Wrench } from "lucide-react";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { useAppStore } from "../stores/useAppStore";
import { Banner, Button } from "./ui";
import * as cmd from "../lib/commands";
import { toastError, toastSuccess } from "../lib/toast";
import type { CompatIssue } from "../lib/types";

/** Problems that stop mods from loading (backend `crate::compat`), re-checked
 * whenever the scanned files change. `summary` is the one-line dashboard form. */
export default function CompatIssues({ gameId, summary }: { gameId: string; summary?: boolean }) {
  const manifest = useAppStore((s) => s.manifest);
  const activeGame = useAppStore((s) => s.activeGame);
  const navigateToGame = useAppStore((s) => s.navigateToGame);
  const [issues, setIssues] = useState<CompatIssue[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [fixing, setFixing] = useState(false);

  const load = () => {
    if (gameId !== activeGame) {
      setIssues([]);
      return;
    }
    cmd.checkCompat(gameId).then(setIssues).catch(() => setIssues([]));
  };

  useEffect(load, [gameId, activeGame, manifest]); // eslint-disable-line react-hooks/exhaustive-deps

  if (issues.length === 0) return null;

  if (summary) {
    const errors = issues.filter((i) => i.severity === "error").length;
    return (
      <Banner
        tone="warn"
        icon={<Stethoscope size={16} />}
        title={issues.length === 1 ? issues[0].title : `${issues.length} problems may stop your mods from loading`}
        actions={<Button size="sm" variant="secondary" onClick={() => navigateToGame(gameId, "content")}>Show</Button>}
      >
        {issues.length > 1 && errors > 0 && <>{errors} of them will stop mods from loading. </>}
        Details and fixes are on the Content page.
      </Banner>
    );
  }

  const fix = async (i: CompatIssue) => {
    if (!i.fix) return;
    setFixing(true);
    try {
      await cmd.fixCompatIssue(gameId, i.fix);
      toastSuccess("Fixed. Start the game again for it to take effect.");
      load();
    } catch (e) {
      toastError(`${e}`);
    } finally {
      setFixing(false);
    }
  };

  return (
    <div className="space-y-2" aria-label="Mod health">
      {issues.map((i) => (
        <Banner
          key={i.kind}
          tone={i.severity === "error" ? "danger" : "warn"}
          icon={<AlertTriangle size={14} />}
          title={i.title}
          actions={
            <>
              {i.fix && (
                <Button size="sm" variant="primary" onClick={() => fix(i)} disabled={fixing} icon={<Wrench size={12} />}>
                  Fix it
                </Button>
              )}
              {i.url && (
                <Button size="sm" variant="secondary" onClick={() => openUrl(i.url!).catch(() => {})} icon={<ExternalLink size={12} />}>
                  Get it
                </Button>
              )}
            </>
          }
        >
          <p>{i.detail}</p>
          {i.count > 0 && (
            <button className="mt-1 flex items-center gap-1 font-mono text-[10.5px] uppercase tracking-[0.06em] text-txt-muted hover:text-txt" onClick={() => setOpen(open === i.kind ? null : i.kind)}>
              {open === i.kind ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
              {i.count} file{i.count !== 1 ? "s" : ""}
            </button>
          )}
          {open === i.kind && (
            <ul className="mt-1 max-h-40 overflow-y-auto">
              {i.paths.map((p) => (
                <li key={p} className="font-mono text-[11px] text-txt-dim truncate" title={p}>{p}</li>
              ))}
              {i.count > i.paths.length && <li className="text-[11px] text-txt-muted">…and {i.count - i.paths.length} more</li>}
            </ul>
          )}
        </Banner>
      ))}
    </div>
  );
}
