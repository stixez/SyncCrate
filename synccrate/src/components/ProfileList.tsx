import { useState, useEffect, useMemo } from "react";
import { Plus, Upload, X } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import ProfileCard from "./ProfileCard";
import { Button, Input, Panel, SectionHeader, StatTile } from "./ui";
import * as cmd from "../lib/commands";
import type { ProfileComparison } from "../lib/types";
import { open, save } from "@tauri-apps/plugin-dialog";

interface Props {
  gameId: string;
}

export default function ProfileList({ gameId }: Props) {
  const profiles = useAppStore((s) => s.profiles);
  const setProfiles = useAppStore((s) => s.setProfiles);
  const addLog = useLogStore((s) => s.addLog);
  const [showCreate, setShowCreate] = useState(false);
  const [name, setName] = useState("");
  const [desc, setDesc] = useState("");
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [comparison, setComparison] = useState<ProfileComparison | null>(null);

  useEffect(() => {
    cmd.listProfiles().then(setProfiles).catch(console.error);
  }, [setProfiles]);

  // Filter profiles to current game
  const filteredProfiles = useMemo(() => {
    return profiles.filter((p) => p.game === gameId);
  }, [profiles, gameId]);

  const handleCreate = async () => {
    if (!name.trim()) return;
    try {
      await cmd.saveProfile(name, desc, "\uD83D\uDCE6", gameId);
      const updated = await cmd.listProfiles();
      setProfiles(updated);
      setName("");
      setDesc("");
      setShowCreate(false);
      addLog(`Profile "${name}" created`, "success");
    } catch (e) {
      addLog(`Failed to create profile: ${e}`, "error");
    }
  };

  const handleDelete = async (id: string) => {
    if (deleteConfirm !== id) {
      setDeleteConfirm(id);
      return;
    }
    setDeleteConfirm(null);
    try {
      await cmd.deleteProfile(id);
      const updated = await cmd.listProfiles();
      setProfiles(updated);
      addLog("Profile deleted", "info");
    } catch (e) {
      addLog(`Failed to delete profile: ${e}`, "error");
    }
  };

  const handleLoad = async (id: string) => {
    try {
      const result = await cmd.loadProfile(id);
      setComparison(result);
      addLog(`Profile "${result.profile_name}" compared: ${result.matched} matched, ${result.missing.length} missing, ${result.modified.length} modified`, "success");
    } catch (e) {
      addLog(`Failed to load profile: ${e}`, "error");
    }
  };

  const handleExport = async (id: string, profileName: string) => {
    try {
      const dest = await save({
        defaultPath: `${profileName}.synccrate-profile`,
        filters: [{ name: "SyncCrate Profile", extensions: ["synccrate-profile", "simshare-profile"] }],
      });
      if (dest) {
        await cmd.exportProfile(id, dest);
        const filename = dest.split(/[/\\]/).pop() || dest;
        addLog(`Profile exported as ${filename}`, "success");
      }
    } catch (e) {
      addLog(`Failed to export profile: ${e}`, "error");
    }
  };

  const handleImport = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "SyncCrate Profile", extensions: ["synccrate-profile", "simshare-profile"] }],
      });
      if (selected) {
        const path = typeof selected === "string" ? selected : selected;
        await cmd.importProfile(path);
        const updated = await cmd.listProfiles();
        setProfiles(updated);
        addLog("Profile imported", "success");
      }
    } catch (e) {
      addLog(`Failed to import profile: ${e}`, "error");
    }
  };

  return (
    <div className="space-y-5">
      <SectionHeader
        label={<><b>// Loadouts</b> &nbsp;{filteredProfiles.length} saved</>}
        title={<>Mod <span className="text-neon">Profiles</span></>}
        description="Snapshot your current mod setup, share it as a file, and compare it against what's installed."
        actions={
          <Button size="sm" onClick={handleImport} icon={<Upload size={13} />}>
            Import
          </Button>
        }
      />

      {comparison && (
        <Panel
          tone="accent"
          brackets
          label={<><b>// Compare</b> &nbsp;Profile vs installed</>}
          title={comparison.profile_name}
          actions={
            <Button size="sm" variant="ghost" onClick={() => setComparison(null)} icon={<X size={13} />}>
              Close
            </Button>
          }
        >
          <div className="grid grid-cols-4 gap-3">
            <StatTile value={comparison.matched} label="Matched" highlight />
            <StatTile value={comparison.missing.length} label="Missing" className={comparison.missing.length ? "[&_p]:text-status-red" : undefined} />
            <StatTile value={comparison.modified.length} label="Modified" className={comparison.modified.length ? "[&_p]:text-amber" : undefined} />
            <StatTile value={comparison.extra.length} label="Extra" />
          </div>
          {(comparison.missing.length > 0 || comparison.modified.length > 0) && (
            <div className="grid grid-cols-2 gap-3 mt-4">
              {comparison.missing.length > 0 && (
                <div className="bg-bg border border-border">
                  <p className="hud-label px-3 py-2 border-b border-border !text-status-red">Missing mods</p>
                  <div className="max-h-32 overflow-y-auto px-3 py-2 space-y-0.5">
                    {comparison.missing.map((p) => (
                      <p key={p} className="text-[11px] text-txt-dim font-mono truncate">{p.split("/").pop()}</p>
                    ))}
                  </div>
                </div>
              )}
              {comparison.modified.length > 0 && (
                <div className="bg-bg border border-border">
                  <p className="hud-label px-3 py-2 border-b border-border !text-amber">Modified mods</p>
                  <div className="max-h-32 overflow-y-auto px-3 py-2 space-y-0.5">
                    {comparison.modified.map((p) => (
                      <p key={p} className="text-[11px] text-txt-dim font-mono truncate">{p.split("/").pop()}</p>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </Panel>
      )}

      <div className="grid grid-cols-2 xl:grid-cols-3 gap-4">
        {filteredProfiles.map((profile) => (
          <ProfileCard
            key={profile.id}
            profile={profile}
            onDelete={() => handleDelete(profile.id)}
            onLoad={() => handleLoad(profile.id)}
            onExport={() => handleExport(profile.id, profile.name)}
            isDeletePending={deleteConfirm === profile.id}
            onCancelDelete={() => setDeleteConfirm(null)}
          />
        ))}

        {showCreate ? (
          <Panel tone="accent" label={<b>// New loadout</b>} title="Snapshot current mods">
            <div className="space-y-3">
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                maxLength={64}
                placeholder="Profile name..."
                aria-label="Profile name"
                autoFocus
              />
              <textarea
                value={desc}
                onChange={(e) => setDesc(e.target.value)}
                maxLength={256}
                placeholder="Description..."
                aria-label="Profile description"
                rows={2}
                className="input !h-auto py-2 resize-none"
              />
              <div className="flex gap-2">
                <Button variant="primary" block onClick={handleCreate} disabled={!name.trim()}>
                  Save Profile
                </Button>
                <Button variant="ghost" onClick={() => setShowCreate(false)}>
                  Cancel
                </Button>
              </div>
            </div>
          </Panel>
        ) : (
          <button
            onClick={() => setShowCreate(true)}
            className="group border border-dashed border-line-hi hover:border-neon p-6 flex flex-col items-start justify-end gap-3 text-left transition-colors min-h-[196px]"
          >
            <span className="w-10 h-10 grid place-items-center border border-line-hi text-txt-muted group-hover:border-neon group-hover:text-neon transition-colors">
              <Plus size={18} />
            </span>
            <span>
              <span className="hud-label block mb-1">// Slot empty</span>
              <span className="font-display font-semibold uppercase tracking-[0.05em] text-[15px] text-txt group-hover:text-neon transition-colors">
                Create New Profile
              </span>
            </span>
          </button>
        )}
      </div>

      {filteredProfiles.length === 0 && !showCreate && (
        <p className="font-mono text-[11px] uppercase tracking-[0.08em] text-txt-muted">
          No profiles for this game yet. Create one to snapshot your current mod setup.
        </p>
      )}
    </div>
  );
}
