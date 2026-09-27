import { useEffect, useMemo, useState } from "react";
import { ArrowLeftRight, Download, Gamepad2, Upload } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { friendlyError } from "../lib/errors";
import { getGameDef } from "../lib/games";
import { formatBytes, formatRelative, plural } from "../lib/utils";
import { toastError, toastSuccess } from "../lib/toast";
import * as cmd from "../lib/commands";
import type { SharedSaveRow, SharedSavesView } from "../lib/types";
import { Badge, Button, Panel } from "./ui";

const LAN_NOTE = "Save handoff works when you join through your crew (Crews, then Join): that connection proves who's who. A LAN connection can't.";

/** Saves a crew takes turns on (backend `crate::handoff`): who has the
 * newest copy, who's playing, and take / give with the host. */
export default function SharedSaves({ gameId }: { gameId: string }) {
  const crewsVersion = useAppStore((s) => s.crewsVersion);
  const manifest = useAppStore((s) => s.manifest);
  const [view, setView] = useState<SharedSavesView | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<{ unit: string; what: "takeover" | "unshare" } | null>(null);
  const [pick, setPick] = useState("");
  const [crewPick, setCrewPick] = useState("");
  const hasSaves = useMemo(() => getGameDef(gameId)?.content_types.some((ct) => ct.file_type === "Save" && ct.save_unit_depth != null) ?? false, [gameId]);

  const load = () => {
    if (!hasSaves) return;
    cmd.getSharedSaves(gameId).then(setView).catch(() => setView(null));
  };
  useEffect(load, [gameId, crewsVersion, manifest, hasSaves]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!hasSaves || !view || view.crews.length === 0) return null;
  const shared = view.rows.filter((r) => r.record);
  const local = view.rows.filter((r) => !r.record && r.files > 0);
  if (shared.length === 0 && local.length === 0) return null;

  const isClient = view.session === "client";
  const canMove = isClient && view.host_supports && view.proven;
  const host = view.host_name ?? "the host";

  const run = async (unit: string, action: () => Promise<unknown>, done?: string) => {
    setBusy(unit);
    try {
      await action();
      if (done) toastSuccess(done);
      setConfirm(null);
      load();
    } catch (e) {
      toastError(friendlyError(e));
    } finally {
      setBusy(null);
    }
  };

  const share = () => {
    const unit = pick || local[0]?.unit;
    const crew = crewPick || view.crews[0]?.id;
    if (!unit || !crew) return;
    const name = view.rows.find((r) => r.unit === unit)?.name ?? unit;
    run(unit, () => cmd.shareSave(crew, gameId, unit), `${name} is shared with your crew. It now only moves when someone takes it or gives it back.`).then(() => setPick(""));
  };

  return (
    <Panel label={<b>// Shared saves</b>} title="Take turns on a save" icon={<ArrowLeftRight size={16} className="text-neon" />}>
      <p className="text-xs text-txt-dim mb-3">
        A shared save moves only when someone takes it or gives it back, never in a normal sync, so nobody plays an old copy. Close the game before handing a save over.
      </p>
      {isClient && !view.host_supports && <p className="text-xs text-amber mb-3">The host's SyncCrate is too old for save handoff. Ask them to update.</p>}
      {isClient && view.host_supports && !view.proven && <p className="text-xs text-amber mb-3">{LAN_NOTE}</p>}

      {shared.length > 0 && (
        <ul className="divide-y divide-border border border-border mb-3">
          {shared.map((r) => (
            <SaveItem
              key={r.unit}
              row={r}
              isClient={isClient}
              canMove={canMove}
              host={host}
              busy={busy === r.unit}
              confirm={confirm?.unit === r.unit ? confirm.what : null}
              setConfirm={(what) => setConfirm(what ? { unit: r.unit, what } : null)}
              onPlaying={(playing) => run(r.unit, () => cmd.setSavePlaying(r.crew!, gameId, r.unit, playing), playing ? "Your friends see that you're playing it." : undefined)}
              onTake={() => run(r.unit, () => cmd.takeSave(r.crew!, gameId, r.unit), `${r.name} is yours now. Have fun, then give it back.`)}
              onGive={() => run(r.unit, () => cmd.giveSave(r.crew!, gameId, r.unit), `${host} has ${r.name} now.`)}
              onTakeOver={() => run(r.unit, () => cmd.takeOverSave(r.crew!, gameId, r.unit), `You have ${r.name} now, as it is on this PC.`)}
              onUnshare={() => run(r.unit, () => cmd.unshareSave(r.crew!, gameId, r.unit), `${r.name} isn't shared any more.`)}
              onAccept={() => run(r.unit, () => cmd.acceptSaveCopy(r.crew!, gameId, r.unit), `${r.record!.holder_name} can give you their copy now.`)}
            />
          ))}
        </ul>
      )}

      {local.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          <label className="text-xs text-txt-dim" htmlFor="share-save">Share a save:</label>
          <select id="share-save" className="input input-sm w-auto! max-w-[16rem]" value={pick || local[0].unit} onChange={(e) => setPick(e.target.value)}>
            {local.map((r) => (
              <option key={r.unit} value={r.unit}>
                {r.name} ({formatBytes(r.bytes)})
              </option>
            ))}
          </select>
          {view.crews.length > 1 && (
            <select aria-label="Crew" className="input input-sm w-auto!" value={crewPick || view.crews[0].id} onChange={(e) => setCrewPick(e.target.value)}>
              {view.crews.map((c) => (
                <option key={c.id} value={c.id}>{c.name}</option>
              ))}
            </select>
          )}
          <Button size="sm" variant="secondary" onClick={share} disabled={busy !== null}>Share</Button>
        </div>
      )}
    </Panel>
  );
}

function SaveItem({
  row: r,
  isClient,
  canMove,
  host,
  busy,
  confirm,
  setConfirm,
  onPlaying,
  onTake,
  onGive,
  onTakeOver,
  onUnshare,
  onAccept,
}: {
  row: SharedSaveRow;
  isClient: boolean;
  canMove: boolean;
  host: string;
  busy: boolean;
  confirm: "takeover" | "unshare" | null;
  setConfirm: (what: "takeover" | "unshare" | null) => void;
  onPlaying: (playing: boolean) => void;
  onTake: () => void;
  onGive: () => void;
  onTakeOver: () => void;
  onUnshare: () => void;
  onAccept: () => void;
}) {
  const rec = r.record!;
  const who = rec.holder_name;
  // The holder is our host (we're a client) or, as host, a connected friend.
  const withHost = isClient && r.holder_connected;
  const status = r.holder_is_me ? (
    rec.playing ? <Badge tone="neon" dot>You're playing</Badge> : <Badge tone="green">With you</Badge>
  ) : rec.playing ? (
    <Badge tone="amber" dot>{who} is playing</Badge>
  ) : (
    <Badge tone="neutral">With {who}</Badge>
  );
  // Only their own word (a take over, or a session this PC wasn't in): as
  // host, their give waits until we accept.
  const needsAccept = !isClient && !r.holder_is_me && rec.claimed;
  const hint = needsAccept
    ? `${who} says they have the newest copy. Accept it if that's right; then they can give it to you.`
    : r.holder_is_me
    ? isClient && !rec.playing && canMove ? `When you're done, give it to ${host}.` : null
    : withHost
      ? rec.playing ? `Wait until ${who} is done.` : null
      : r.holder_connected
        ? `${who} can give it to you from their Dashboard.`
        : `To play it, ${who} gives it to whoever hosts; then you take it from them.`;

  return (
    <li className="px-3 py-2.5 flex flex-wrap items-center gap-x-3 gap-y-2">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="font-semibold text-sm truncate">{r.name}</span>
          {status}
          {r.crew_name && <span className="font-mono text-[10.5px] text-txt-muted">{r.crew_name}</span>}
        </div>
        <p className="text-[11.5px] text-txt-muted mt-0.5">
          {r.files > 0 ? `Your copy: ${plural(r.files, "file")}, ${formatBytes(r.bytes)}, changed ${formatRelative(r.modified)}` : "Not on this PC yet"}
          {" · "}updated {formatRelative(rec.updated_at)}
        </p>
        {hint && <p className="text-[11.5px] text-txt-dim mt-0.5">{hint}</p>}
        {confirm === "takeover" && (
          <p className="text-[12px] text-amber mt-1.5">
            Take it as it is on this PC? Anything {who} played since they took it stays on their PC and no longer counts.
          </p>
        )}
        {confirm === "unshare" && <p className="text-[12px] text-amber mt-1.5">Stop sharing it? It then syncs like any other file again.</p>}
      </div>
      <div className="flex items-center gap-2 flex-wrap">
        {confirm ? (
          <>
            <Button size="sm" variant="danger" disabled={busy} onClick={confirm === "takeover" ? onTakeOver : onUnshare}>
              {confirm === "takeover" ? "Take over" : "Stop sharing"}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setConfirm(null)}>Cancel</Button>
          </>
        ) : (
          <>
            {r.holder_is_me && (
              <Button size="sm" variant="secondary" disabled={busy} icon={<Gamepad2 size={12} />} onClick={() => onPlaying(!rec.playing)}>
                {rec.playing ? "Done playing" : "I'm playing"}
              </Button>
            )}
            {r.holder_is_me && isClient && canMove && !rec.playing && (
              <Button size="sm" variant="primary" disabled={busy} icon={<Upload size={12} />} onClick={onGive}>
                Give to {host}
              </Button>
            )}
            {withHost && canMove && (
              <Button size="sm" variant="primary" disabled={busy || rec.playing} icon={<Download size={12} />} onClick={onTake} title={rec.playing ? `${who} is playing it` : undefined}>
                Take it
              </Button>
            )}
            {needsAccept && (
              <Button size="sm" variant="primary" disabled={busy} onClick={onAccept}>
                Accept {who}'s copy
              </Button>
            )}
            {!r.holder_is_me && r.files > 0 && (
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirm("takeover")}>Take over</Button>
            )}
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirm("unshare")}>Stop sharing</Button>
          </>
        )}
      </div>
    </li>
  );
}
