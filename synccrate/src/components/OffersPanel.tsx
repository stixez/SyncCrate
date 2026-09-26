import { useEffect, useState } from "react";
import { friendlyError } from "../lib/errors";
import { Check, Gift, X } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { Badge, Button, Panel, cx } from "./ui";
import { fileName, formatBytes } from "../lib/utils";
import * as cmd from "../lib/commands";
import { toastError } from "../lib/toast";
import type { IncomingOffer, OfferedFile, OfferState, OutgoingOfferView } from "../lib/types";

const STATE_LABEL: Record<OfferState, string> = {
  pending: "Waiting",
  accepted: "Sending…",
  declined: "Declined",
  received: "Added",
  failed: "Failed",
};

function StateBadge({ state }: { state: OfferState }) {
  const tone = state === "received" ? "green" : state === "failed" ? "red" : state === "declined" ? "neutral" : state === "accepted" ? "neon" : "amber";
  return <Badge tone={tone}>{STATE_LABEL[state]}</Badge>;
}

/** Offers between friends and the host (backend `crate::offers`): the host
 * picks what to take from each friend; a friend sees how their offer went. */
/** Ticked paths: everything except what the host unticked, and scripts only
 * when the host ticked them (`toggled` flips each file's default). */
function chosenPaths(pending: OfferedFile[], toggled?: Set<string>): string[] {
  return pending.filter((f) => (f.script ? !!toggled?.has(f.file.relative_path) : !toggled?.has(f.file.relative_path))).map((f) => f.file.relative_path);
}

export default function OffersPanel() {
  const session = useAppStore((s) => s.session);
  const version = useAppStore((s) => s.offersVersion);
  const isHost = session?.session_type === "Host";
  const [incoming, setIncoming] = useState<IncomingOffer[]>([]);
  const [outgoing, setOutgoing] = useState<OutgoingOfferView | null>(null);
  // Per friend: files the host unticked. New files start ticked.
  const [unticked, setUnticked] = useState<Record<string, Set<string>>>({});

  useEffect(() => {
    if (isHost) cmd.getIncomingOffers().then(setIncoming).catch(() => {});
    else cmd.getOutgoingOffer().then(setOutgoing).catch(() => {});
  }, [isHost, version, session?.session_type]);

  const [deciding, setDeciding] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const decide = async (offer: IncomingOffer, acceptAll?: boolean) => {
    if (deciding) return;
    setDeciding(true);
    const pendingFiles = offer.files.filter((f) => f.state === "pending");
    const pending = pendingFiles.map((f) => f.file.relative_path);
    const chosen = new Set(acceptAll === false ? [] : acceptAll ? pending : chosenPaths(pendingFiles, unticked[offer.peer_id]));
    try {
      await cmd.decideOffer(offer.peer_id, pending.filter((p) => chosen.has(p)), pending.filter((p) => !chosen.has(p)));
      setUnticked((prev) => {
        const next = { ...prev };
        delete next[offer.peer_id];
        return next;
      });
      setIncoming(await cmd.getIncomingOffers());
    } catch (e) {
      toastError(friendlyError(e));
    } finally {
      setDeciding(false);
    }
  };

  if (isHost) {
    const open = incoming.filter((o) => o.files.length > 0);
    if (open.length === 0) return null;
    return (
      <Panel label={<b>// Offers</b>} title="Files friends want to give you" icon={<Gift size={16} className="text-neon" />}>
        <p className="text-xs text-txt-dim mb-3">
          Nothing is added until you accept it. Accepted files are checked again when they arrive (same file, mod folders only, never replacing yours).
        </p>
        <div className="space-y-4">
          {open.map((o) => {
            const pending = o.files.filter((f) => f.state === "pending");
            const chosen = new Set(chosenPaths(pending, unticked[o.peer_id]));
            const scripts = pending.filter((f) => f.script).length;
            const toggle = (p: string) =>
              setUnticked((prev) => {
                const next = new Set(prev[o.peer_id] ?? []);
                if (next.has(p)) next.delete(p);
                else next.add(p);
                return { ...prev, [o.peer_id]: next };
              });
            return (
              <div key={o.peer_id}>
                <div className="flex items-center gap-2 mb-2">
                  <p className="text-[13px] text-txt flex-1">
                    <span className="font-medium">{o.peer_name}</span> offers {o.files.length} file{o.files.length !== 1 ? "s" : ""}
                  </p>
                  {pending.length > 0 && (
                    <>
                      <Button size="sm" variant="primary" onClick={() => decide(o)} disabled={chosen.size === 0 || deciding} icon={<Check size={12} />}>
                        Accept selected ({chosen.size})
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => decide(o, false)} disabled={deciding} icon={<X size={12} />}>
                        Decline all
                      </Button>
                    </>
                  )}
                </div>
                {scripts > 0 && (
                  <p className="text-xs text-amber mb-2">
                    {scripts} of these file{scripts !== 1 ? "s run" : " runs"} code on your PC (scripts or plugins), so {scripts !== 1 ? "they aren't" : "it isn't"} ticked. Only accept scripts from friends you trust.
                  </p>
                )}
                <ul className="border border-border divide-y divide-border max-h-56 overflow-y-auto">
                  {o.files.map((f) => (
                    <li key={f.file.relative_path} className="row-y px-3 py-1.5 flex items-center gap-3">
                      {f.state === "pending" ? (
                        <input
                          type="checkbox"
                          className="check shrink-0"
                          checked={chosen.has(f.file.relative_path)}
                          onChange={() => toggle(f.file.relative_path)}
                          aria-label={`Accept ${fileName(f.file.relative_path)}`}
                        />
                      ) : (
                        <span className="w-[14px] shrink-0" />
                      )}
                      <span className="flex-1 min-w-0 truncate text-[12.5px] text-txt" title={f.file.relative_path}>{f.file.relative_path}</span>
                      {f.script && <span className="font-mono text-[10px] uppercase tracking-[0.08em] text-amber shrink-0">Runs code</span>}
                      <span className="font-mono text-[10.5px] text-txt-muted tabular">{formatBytes(f.file.size)}</span>
                      <StateBadge state={f.state} />
                    </li>
                  ))}
                </ul>
              </div>
            );
          })}
        </div>
      </Panel>
    );
  }

  const offer = outgoing?.offer;
  if (!offer || offer.files.length === 0) return null;
  const counts = offer.files.reduce<Record<string, number>>((acc, f) => ({ ...acc, [f.state]: (acc[f.state] ?? 0) + 1 }), {});
  const active = offer.files.some((f) => f.state === "pending" || f.state === "accepted");
  return (
    <Panel
      label={<b>// Your offer</b>}
      title={active ? "Waiting for the host" : "Offer finished"}
      icon={<Gift size={16} className="text-neon" />}
      actions={
        <Button
          size="sm"
          variant="ghost"
          disabled={cancelling}
          onClick={() => {
            setCancelling(true);
            cmd
              .cancelOffer()
              .then(() => setOutgoing({ available: outgoing?.available ?? false, offer: null }))
              .catch((e) => toastError(`Couldn't cancel the offer: ${friendlyError(e)}`))
              .finally(() => setCancelling(false));
          }}
        >
          {active ? "Cancel offer" : "Clear"}
        </Button>
      }
    >
      <p className="text-xs text-txt-dim mb-2">
        {[
          counts.received && `${counts.received} added`,
          counts.accepted && `${counts.accepted} sending`,
          counts.pending && `${counts.pending} waiting`,
          counts.declined && `${counts.declined} declined`,
          counts.failed && `${counts.failed} failed`,
        ]
          .filter(Boolean)
          .join(" · ")}
      </p>
      <ul className="border border-border divide-y divide-border max-h-48 overflow-y-auto">
        {offer.files.map((f) => (
          <li key={f.file.relative_path} className={cx("row-y px-3 py-1.5 flex items-center gap-3", f.state === "declined" && "opacity-60")}>
            <span className="flex-1 min-w-0 truncate text-[12.5px] text-txt" title={f.message ?? f.file.relative_path}>
              {fileName(f.file.relative_path)}
              {f.message && <span className="text-txt-muted"> · {f.message}</span>}
            </span>
            <StateBadge state={f.state} />
          </li>
        ))}
      </ul>
    </Panel>
  );
}
