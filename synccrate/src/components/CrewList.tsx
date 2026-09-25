import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, Check, Copy, Link2, Loader2, LogOut, Pencil, Plug, Radar, RefreshCw, Share2, UserMinus, UserPlus, Users, X } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { useSession } from "../hooks/useSession";
import { Badge, Banner, Button, EmptyState, Input, LiveDot, Panel, SectionHeader, StatTile, cx } from "./ui";
import { gameLabel, getGameDef } from "../lib/games";
import { loadDisplayName } from "../lib/prefs";
import { formatDate } from "../lib/utils";
import * as cmd from "../lib/commands";
import { toastError, toastInfo, toastSuccess } from "../lib/toast";
import type { Crew, CrewInvite, CrewLanHost, CrewStatus } from "../lib/types";

const myName = () => loadDisplayName().trim() || "Guest";

/** Crews: remembered friend groups. Everything here is local data plus the
 * ordinary session/pack flows; nothing connects or syncs without a click. */
export default function CrewList() {
  const activeGame = useAppStore((s) => s.activeGame);
  const session = useAppStore((s) => s.session);
  const crewsVersion = useAppStore((s) => s.crewsVersion);
  const pendingCrewInvite = useAppStore((s) => s.pendingCrewInvite);
  const setPendingCrewInvite = useAppStore((s) => s.setPendingCrewInvite);
  const addLog = useLogStore((s) => s.addLog);

  const [crews, setCrews] = useState<Crew[]>([]);
  const [myNode, setMyNode] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const [inviteText, setInviteText] = useState("");
  const [preview, setPreview] = useState<CrewInvite | null>(null);
  const [lanHosts, setLanHosts] = useState<CrewLanHost[]>([]);
  const [scanning, setScanning] = useState(false);

  const reload = () =>
    cmd
      .listCrews()
      .then((list) => {
        setCrews(list);
        setSelectedId((cur) => (cur && list.some((c) => c.id === cur) ? cur : list[0]?.id ?? null));
      })
      .catch((e) => toastError(`Couldn't load crews: ${e}`));

  useEffect(() => {
    reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [crewsVersion]);

  useEffect(() => {
    cmd.getLocalNodeId().then(setMyNode).catch(() => {});
  }, []);

  // A clicked crew link (already validated by the backend).
  useEffect(() => {
    if (!pendingCrewInvite) return;
    setPreview(pendingCrewInvite);
    setPendingCrewInvite(null);
  }, [pendingCrewInvite, setPendingCrewInvite]);

  const handleCreate = async () => {
    if (!newName.trim()) return;
    try {
      const c = await cmd.createCrew(newName.trim(), myName());
      setNewName("");
      addLog(`Crew "${c.name}" created`, "success");
      await reload();
      setSelectedId(c.id);
    } catch (e) {
      toastError(`${e}`);
    }
  };

  const handlePreviewInvite = async () => {
    if (!inviteText.trim()) return;
    try {
      setPreview(await cmd.previewCrewInvite(inviteText.trim()));
      setInviteText("");
    } catch (e) {
      toastError(`${e}`);
    }
  };

  const handleAddCrew = async () => {
    if (!preview) return;
    try {
      const c = await cmd.joinCrew(preview, myName());
      setPreview(null);
      addLog(`Added crew "${c.name}"`, "success");
      toastSuccess(`Added ${c.name}. When someone in the crew hosts, connect from here.`);
      await reload();
      setSelectedId(c.id);
    } catch (e) {
      toastError(`${e}`);
    }
  };

  const handleScan = async () => {
    setScanning(true);
    try {
      const found = await cmd.scanCrewHosts();
      setLanHosts(found);
      if (found.length === 0) toastInfo("No crew members are hosting on your network right now.");
    } catch (e) {
      toastError(`Scan failed: ${e}`);
    } finally {
      setScanning(false);
    }
  };

  const selected = crews.find((c) => c.id === selectedId) ?? null;
  const alreadyIn = preview ? crews.some((c) => c.id === preview.id) : false;

  return (
    <div className="space-y-5">
      <SectionHeader
        label={<><b>// Play together</b> &nbsp;every week</>}
        title={<>Cre<span className="text-neon">ws</span></>}
        description="A crew remembers your group: who's in it, what you play, and the crew's mod set. Returning friends catch up in one click, and new members join with one invite link."
      />

      {preview && (
        <Banner
          tone="info"
          icon={<UserPlus size={14} />}
          title={alreadyIn ? `You're already in ${preview.name}` : `Add ${preview.name} to your crews?`}
          actions={
            <div className="flex gap-2">
              <Button size="sm" variant="primary" onClick={handleAddCrew} icon={<Check size={12} />}>
                {alreadyIn ? "Update" : "Add Crew"}
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setPreview(null)} icon={<X size={12} />}>
                Dismiss
              </Button>
            </div>
          }
        >
          <p>
            Invited by <span className="text-txt">{preview.from_name}</span>
            {preview.games.length > 0 && <> · plays {preview.games.map(gameLabel).join(", ")}</>}. Adding a crew doesn't
            connect or sync anything, and the host's PIN still applies.
          </p>
        </Banner>
      )}

      <div className="grid grid-cols-1 xl:grid-cols-2 gap-4">
        <Panel tone="accent" label={<b>// New</b>} title="Start a crew">
          <div className="flex gap-2">
            <Input value={newName} onChange={(e) => setNewName(e.target.value)} maxLength={64} placeholder="Crew name (e.g. Sunday Sims Crew)..." aria-label="Crew name" onKeyDown={(e) => e.key === "Enter" && handleCreate()} />
            <Button variant="primary" onClick={handleCreate} disabled={!newName.trim()} icon={<Users size={14} />}>
              Create
            </Button>
          </div>
          <p className="text-[11px] text-txt-muted mt-2">Starts with {gameLabel(activeGame)}. Other games are added when someone publishes a set for them.</p>
        </Panel>
        <Panel tone="accent" label={<b>// Invite</b>} title="Join a crew">
          <div className="flex gap-2">
            <Input value={inviteText} onChange={(e) => setInviteText(e.target.value)} placeholder="synccrate://crew/..." aria-label="Crew invite link" mono onKeyDown={(e) => e.key === "Enter" && handlePreviewInvite()} />
            <Button onClick={handlePreviewInvite} disabled={!inviteText.trim()} icon={<Link2 size={12} />}>
              Open
            </Button>
          </div>
          <p className="text-[11px] text-txt-muted mt-2">Or just click the link a friend sent you.</p>
        </Panel>
      </div>

      {crews.length === 0 ? (
        <EmptyState
          icon={<Users size={18} />}
          label="// No crews yet"
          title="Start one, or open a friend's invite"
          description="Crews live on your PC only. They're shared with the others in the crew whenever you connect to each other."
        />
      ) : (
        <>
          {crews.length > 1 && (
            <div className="flex flex-wrap gap-1.5">
              {crews.map((c) => (
                <button
                  key={c.id}
                  onClick={() => setSelectedId(c.id)}
                  className={cx(
                    "h-8 px-3 font-display font-semibold text-[11px] uppercase tracking-[0.08em] border transition-colors",
                    c.id === selectedId ? "bg-neon/10 border-neon text-neon" : "bg-bg border-line-hi text-txt-dim hover:text-txt",
                  )}
                >
                  {c.name}
                </button>
              ))}
            </div>
          )}
          {selected && (
            <CrewDetail
              key={selected.id}
              crew={selected}
              myNode={myNode}
              activeGame={activeGame}
              inSession={!!session && session.session_type !== "None"}
              isHosting={session?.session_type === "Host"}
              lanHosts={lanHosts.filter((h) => h.crew_id === selected.id)}
              scanning={scanning}
              onScan={handleScan}
              onChanged={reload}
              crewsVersion={crewsVersion}
            />
          )}
        </>
      )}
    </div>
  );
}

interface DetailProps {
  crew: Crew;
  myNode: string;
  activeGame: string;
  inSession: boolean;
  isHosting: boolean;
  lanHosts: CrewLanHost[];
  scanning: boolean;
  onScan: () => void;
  onChanged: () => Promise<void> | void;
  crewsVersion: number;
}

function CrewDetail({ crew, myNode, activeGame, inSession, isHosting, lanHosts, scanning, onScan, onChanged, crewsVersion }: DetailProps) {
  const addLog = useLogStore((s) => s.addLog);
  const navigateToGame = useAppStore((s) => s.navigateToGame);
  const setPendingImportPack = useAppStore((s) => s.setPendingImportPack);
  const { connectCrew, isLoading } = useSession();

  const [status, setStatus] = useState<CrewStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [nameDraft, setNameDraft] = useState(crew.name);
  const [confirmLeave, setConfirmLeave] = useState(false);
  const [publishing, setPublishing] = useState(false);
  const [confirmPublish, setConfirmPublish] = useState(false);

  const def = getGameDef(activeGame);
  // Saves are personal; a crew set is about the shared mods by default.
  const defaultTypes = useMemo(
    () => (def?.content_types ?? []).filter((ct) => ct.syncable !== false && ct.file_type !== "Save").map((ct) => ct.id),
    [def],
  );
  const [types, setTypes] = useState<Set<string>>(new Set(defaultTypes));
  useEffect(() => setTypes(new Set(defaultTypes)), [defaultTypes]);

  const set = crew.sets[activeGame];
  const playsActive = crew.games.includes(activeGame) || !!set;

  const check = async () => {
    setChecking(true);
    try {
      setStatus(await cmd.crewStatus(crew.id));
    } catch (e) {
      setStatus(null);
      toastError(`Couldn't compare with the crew set: ${e}`);
    } finally {
      setChecking(false);
    }
  };

  useEffect(() => {
    if (set) check();
    else setStatus(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [crew.id, activeGame, set?.version, set?.published_at, crewsVersion]);

  const members = [...crew.members].sort((a, b) => Number(!!a.removed) - Number(!!b.removed) || a.name.localeCompare(b.name));
  const hostingNow = new Set(lanHosts.map((h) => h.node_id));
  const last = crew.last_host;

  const handleCopyInvite = async () => {
    try {
      await navigator.clipboard.writeText(await cmd.crewInviteLink(crew.id, myName()));
      toastSuccess("Invite link copied. It has no PIN in it, so set a PIN when you host.");
    } catch (e) {
      toastError(`${e}`);
    }
  };

  const handleRename = async () => {
    try {
      await cmd.renameCrew(crew.id, nameDraft);
      setRenaming(false);
      await onChanged();
    } catch (e) {
      toastError(`${e}`);
    }
  };

  const handleLeave = async () => {
    try {
      await cmd.leaveCrew(crew.id);
      addLog(`Left crew "${crew.name}"`, "info");
      await onChanged();
    } catch (e) {
      toastError(`${e}`);
    }
  };

  const handlePublish = async () => {
    setConfirmPublish(false);
    setPublishing(true);
    try {
      const s = await cmd.publishCrewSet(crew.id, types.size > 0 ? Array.from(types) : undefined, myName());
      addLog(`Published crew set v${s.version} for ${gameLabel(activeGame)}: ${s.pack.files.length} files`, "success");
      toastSuccess(
        isHosting
          ? "Crew set published. Members get it the next time they connect to you."
          : "Crew set published. Host a session and members get it when they join.",
      );
      await onChanged();
    } catch (e) {
      toastError(`Couldn't publish: ${e}`);
    } finally {
      setPublishing(false);
    }
  };

  const handleCatchUp = () => {
    if (!set) return;
    // The Modpacks import view already does compare, "get missing files" and
    // "apply exactly"; the crew set is just a pack.
    navigateToGame(activeGame, "modpacks");
    setPendingImportPack(set.pack);
  };

  const handleConnect = async (nodeId: string | undefined, label: string) => {
    await connectCrew(crew.id, nodeId, myName(), label);
    // The dashboard shows progress and the PIN / wrong-game prompts.
    navigateToGame(activeGame, "dashboard");
  };

  const handleRemove = async (nodeId: string, removed: boolean) => {
    try {
      await cmd.setCrewMemberRemoved(crew.id, nodeId, removed);
      await onChanged();
    } catch (e) {
      toastError(`${e}`);
    }
  };

  return (
    <Panel
      tone="accent"
      brackets
      label={<><b>// Crew</b> &nbsp;{crew.members.filter((m) => !m.removed).length} members</>}
      title={
        renaming ? (
          <span className="flex gap-2 items-center">
            <Input value={nameDraft} onChange={(e) => setNameDraft(e.target.value)} maxLength={64} aria-label="Crew name" size="sm" onKeyDown={(e) => e.key === "Enter" && handleRename()} />
            <Button size="sm" variant="primary" onClick={handleRename} disabled={!nameDraft.trim()}>Save</Button>
            <Button size="sm" variant="ghost" onClick={() => setRenaming(false)}>Cancel</Button>
          </span>
        ) : (
          crew.name
        )
      }
      actions={
        <div className="flex gap-2">
          <Button size="sm" variant="secondary" onClick={handleCopyInvite} icon={<Copy size={12} />}>Copy Invite</Button>
          {!renaming && (
            <Button size="sm" variant="ghost" onClick={() => { setNameDraft(crew.name); setRenaming(true); }} icon={<Pencil size={12} />}>Rename</Button>
          )}
          {confirmLeave ? (
            <>
              <Button size="sm" variant="danger" onClick={handleLeave}>Leave</Button>
              <Button size="sm" variant="ghost" onClick={() => setConfirmLeave(false)}>Stay</Button>
            </>
          ) : (
            <Button size="sm" variant="ghost" onClick={() => setConfirmLeave(true)} icon={<LogOut size={12} />}>Leave</Button>
          )}
        </div>
      }
    >
      <div className="flex flex-wrap gap-1.5 mb-4">
        {crew.games.map((g) => (
          <Badge key={g} tone={g === activeGame ? "neon" : "neutral"}>{gameLabel(g)}</Badge>
        ))}
      </div>

      {/* Crew set for the active game */}
      <div className="space-y-3">
        <p className="hud-label">// Crew set · {gameLabel(activeGame)}</p>
        {!playsActive && !set ? (
          <p className="text-xs text-txt-dim">This crew doesn't play {gameLabel(activeGame)} yet. Publishing a set for it adds it.</p>
        ) : null}
        {set ? (
          <>
            <div className="grid grid-cols-3 gap-3">
              <StatTile value={`v${set.version}`} label="Version" />
              <StatTile value={set.pack.files.length} label="Files" />
              <StatTile
                value={checking ? "…" : status?.behind ?? "?"}
                label={status && status.behind === 0 ? "In sync" : "Behind"}
                highlight={!!status && status.behind === 0}
                className={status && status.behind > 0 ? "[&_p]:text-amber" : undefined}
              />
            </div>
            <p className="text-[11px] text-txt-muted">
              Published {set.publisher ? <>by <span className="text-txt-dim">{set.publisher}</span> </> : null}on {formatDate(set.published_at)}.
              {status?.comparison && status.behind > 0 && <> {status.comparison.missing.length} missing, {status.comparison.different.length} different.</>}
            </p>
            <div className="flex flex-wrap gap-2">
              {status && status.behind > 0 && (
                <Button variant="primary" onClick={handleCatchUp} icon={<RefreshCw size={14} />}>
                  Catch Up ({status.behind})
                </Button>
              )}
              <Button variant="ghost" onClick={check} disabled={checking} icon={checking ? <Loader2 size={14} className="animate-spin" /> : <RefreshCw size={14} />}>
                Re-check
              </Button>
            </div>
            {status && status.behind > 0 && !inSession && (
              <p className="text-[11px] text-txt-muted">Catching up downloads from a host: connect to someone in the crew first (below).</p>
            )}
          </>
        ) : (
          playsActive && <p className="text-xs text-txt-dim">No crew set for {gameLabel(activeGame)} yet.</p>
        )}

        {/* Publish */}
        <div className="pt-3 border-t border-line space-y-2">
          {(def?.content_types.length ?? 0) > 1 && (
            <div className="flex flex-wrap gap-1.5">
              {def!.content_types.map((ct) => (
                <button
                  key={ct.id}
                  onClick={() => setTypes((prev) => { const n = new Set(prev); n.has(ct.id) ? n.delete(ct.id) : n.add(ct.id); return n; })}
                  className={cx(
                    "h-7 px-3 font-mono text-[10.5px] uppercase tracking-[0.08em] border transition-colors",
                    types.has(ct.id) ? "bg-neon/10 border-neon text-neon" : "bg-bg border-line-hi text-txt-dim hover:text-txt",
                  )}
                >
                  {ct.label}
                </button>
              ))}
            </div>
          )}
          {confirmPublish ? (
            <Banner
              tone="warn"
              icon={<AlertTriangle size={14} />}
              title={`Make your ${gameLabel(activeGame)} files the crew set?`}
              actions={
                <div className="flex gap-2">
                  <Button size="sm" variant="primary" onClick={handlePublish}>Publish</Button>
                  <Button size="sm" variant="ghost" onClick={() => setConfirmPublish(false)}>Cancel</Button>
                </div>
              }
            >
              Members will see how far they are from exactly this list. Only the list is shared, never files; they still pull from a host and choose what to apply.
            </Banner>
          ) : (
            <Button onClick={() => setConfirmPublish(true)} disabled={publishing} icon={publishing ? <Loader2 size={14} className="animate-spin" /> : <Share2 size={14} />}>
              {publishing ? "Publishing..." : set ? "Publish My Setup as the New Crew Set" : "Publish My Setup as the Crew Set"}
            </Button>
          )}
        </div>
      </div>

      {/* Members + connect */}
      <div className="mt-5 pt-4 border-t border-line">
        <div className="flex items-center justify-between gap-2 mb-2">
          <p className="hud-label">// Members</p>
          <div className="flex gap-2">
            {last && last.node_id !== myNode && !inSession && (
              <Button size="sm" variant="primary" onClick={() => handleConnect(last.node_id, last.name)} disabled={isLoading} icon={<Plug size={12} />}>
                Reconnect to {last.name}
              </Button>
            )}
            <Button size="sm" variant="ghost" onClick={onScan} disabled={scanning} icon={scanning ? <Loader2 size={12} className="animate-spin" /> : <Radar size={12} />}>
              {scanning ? "Looking..." : "Who's Hosting?"}
            </Button>
          </div>
        </div>
        <div className="border border-border divide-y divide-border">
          {members.map((m) => {
            const me = m.node_id === myNode;
            const live = hostingNow.has(m.node_id);
            return (
              <div key={m.node_id} className={cx("row-y flex items-center gap-3 px-3 py-2", m.removed && "opacity-50")}>
                {live ? <LiveDot /> : <LiveDot tone="idle" />}
                <div className="flex-1 min-w-0">
                  <p className="text-[13px] text-txt truncate">
                    {m.name}
                    {me && <span className="text-txt-muted"> (you)</span>}
                    {m.removed && <span className="text-txt-muted"> · removed</span>}
                  </p>
                  <p className="font-mono text-[10px] text-txt-muted truncate">
                    {live ? "hosting now on your network" : m.last_seen ? `last seen ${formatDate(m.last_seen)}` : "not seen yet"}
                    {last?.node_id === m.node_id && " · last host"}
                  </p>
                </div>
                {!me && !m.removed && !inSession && (
                  <Button size="sm" variant={live ? "primary" : "secondary"} onClick={() => handleConnect(m.node_id, m.name)} disabled={isLoading} icon={<Plug size={12} />}>
                    Join
                  </Button>
                )}
                {!me && (
                  <Button size="sm" variant="ghost" onClick={() => handleRemove(m.node_id, !m.removed)} icon={m.removed ? <UserPlus size={12} /> : <UserMinus size={12} />}>
                    {m.removed ? "Restore" : "Remove"}
                  </Button>
                )}
              </div>
            );
          })}
        </div>
        <p className="text-[11px] text-txt-muted mt-2 leading-relaxed">
          Join works over the internet with no code; it's faster on the same network. The host's PIN and game checks still
          apply. Removing someone stops sharing crew info with them and spreads to members as they connect, but they keep
          their copy of the crew: to lock someone out, change your PIN.
        </p>
      </div>
    </Panel>
  );
}
