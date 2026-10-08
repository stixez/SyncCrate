import { useEffect, useState } from "react";
import { Check, Loader2, Search, X } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import * as cmd from "../lib/commands";
import type { BisectView } from "../lib/commands";
import { toastError, toastSuccess } from "../lib/toast";
import { plural } from "../lib/utils";
import { Banner, Button, Panel } from "./ui";

interface Props {
  gameId: string;
  gameLabel: string;
  onClose: () => void;
  /** A search is running (so the page keeps this open after a restart). */
  onActive?: (active: boolean) => void;
}

/**
 * "Find a broken mod": the 50/50 method, with SyncCrate turning the halves
 * on and off (backend `commands::bisect`).
 */
export default function FiftyFifty({ gameId, gameLabel, onClose, onActive }: Props) {
  const addLog = useLogStore((s) => s.addLog);
  const [view, setView] = useState<BisectView | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    cmd.bisectStatus(gameId)
      .then((v) => { if (!cancelled) setView(v); })
      .catch(() => {})
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [gameId]);

  useEffect(() => { onActive?.(!!view); }, [view, onActive]);

  const run = async (what: () => Promise<BisectView | null>, done?: string) => {
    setBusy(true);
    try {
      const v = await what();
      setView(v);
      if (v?.errors.length) toastError(`${plural(v.errors.length, "file")} couldn't be moved: ${v.errors[0]}`);
      if (done && !v?.errors.length) {
        toastSuccess(done);
        addLog(done, "success");
      }
      cmd.scanFiles(gameId).then((m) => useAppStore.getState().setManifest(m, gameId)).catch(() => {});
    } catch (e) {
      toastError(`${e}`);
    } finally {
      setBusy(false);
    }
  };

  const start = () => run(() => cmd.bisectStart(gameId));
  const answer = (stillBroken: boolean) => run(() => cmd.bisectAnswer(gameId, stillBroken));
  const stop = (keepCulpritOff: boolean) =>
    run(async () => {
      const v = await cmd.bisectStop(gameId, keepCulpritOff);
      // Done unless some files couldn't be moved back (then it stays, to retry).
      return v.errors.length ? v : null;
    }, keepCulpritOff && view?.culprit ? `${view.culprit.name} stays off; every other mod is back on.` : "Every mod the search turned off is back on.");

  const spinner = busy ? <Loader2 size={12} className="animate-spin" /> : undefined;

  return (
    <Panel
      tone="accent"
      label={<b>// 50/50 check</b>}
      title="Find a broken mod"
      icon={<Search size={15} className="text-neon" />}
      actions={
        !view && (
          <button onClick={onClose} className="p-1 text-txt-muted hover:text-txt" aria-label="Close">
            <X size={15} />
          </button>
        )
      }
      bodyClassName="space-y-3"
    >
      {loading ? (
        <p className="flex items-center gap-2 text-xs text-txt-dim"><Loader2 size={12} className="animate-spin" /> Checking…</p>
      ) : !view ? (
        <>
          <p className="text-[13px] text-txt-dim leading-relaxed">
            Something broken in {gameLabel} (a crash, a missing menu, weird behaviour) and you don't know which mod? SyncCrate turns
            half of your mods off, you check the game, and it halves again. Even with thousands of mods it takes only about a dozen rounds.
          </p>
          <ul className="text-[12px] text-txt-dim space-y-1 list-disc pl-5">
            <li>A creator's folder counts as one mod; your own disabled mods are left alone.</li>
            <li>Close the game between rounds. Your progress is saved, even if you close SyncCrate.</li>
            <li>At the end, every mod goes back on (you can keep the broken one off).</li>
          </ul>
          <Button variant="primary" onClick={start} disabled={busy} icon={spinner}>
            Start
          </Button>
        </>
      ) : view.culprit ? (
        <>
          <Banner tone="success" icon={<Check size={14} />} title={`Found it: ${view.culprit.name}`}>
            With it off, the problem went away. Its files:
            <ul className="font-mono text-[11px] mt-1.5 space-y-0.5 max-h-32 overflow-y-auto">
              {view.culprit.files.map((f) => <li key={f} className="truncate" title={f}>{f}</li>)}
            </ul>
          </Banner>
          <p className="text-xs text-txt-dim">
            It's off right now, and everything else is back on. Start the game once more to make sure the problem is gone, then look
            for an update on the creator's page, or keep it off.
          </p>
          <p className="text-[11px] text-txt-muted">
            Still broken with it off? Then it may not be a mod after all (for The Sims 4, try deleting localthumbcache.package).
          </p>
          <div className="flex flex-wrap gap-2">
            <Button variant="primary" onClick={() => stop(true)} disabled={busy} icon={spinner}>Keep it off, finish</Button>
            <Button onClick={() => stop(false)} disabled={busy}>Turn everything back on</Button>
          </div>
        </>
      ) : (
        <>
          <p className="font-mono text-[11px] uppercase tracking-[0.08em] text-txt-muted">
            Round <span className="text-neon tabular">{view.round}</span> · {view.suspects} of {view.total} mods still suspects · about{" "}
            {view.rounds_left} round{view.rounds_left !== 1 ? "s" : ""} to go
          </p>
          <p className="text-[13px] text-txt-dim leading-relaxed">
            <b className="font-medium text-txt">{view.off}</b> mod{view.off !== 1 ? "s are" : " is"} off right now. Start {gameLabel},
            check whether the problem is still there, then close the game and answer:
          </p>
          <div className="flex flex-wrap gap-2">
            <Button variant="danger" onClick={() => answer(true)} disabled={busy} icon={spinner}>Still broken</Button>
            <Button variant="primary" onClick={() => answer(false)} disabled={busy}>It's fixed now</Button>
            <Button variant="ghost" onClick={() => stop(false)} disabled={busy}>Stop and turn everything back on</Button>
          </div>
        </>
      )}
    </Panel>
  );
}
