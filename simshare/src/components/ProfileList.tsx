import { useState, useEffect, useMemo } from "react";
import { Plus, Upload } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import ProfileCard from "./ProfileCard";
import * as cmd from "../lib/commands";
import type { ProfileComparison, SimsGame } from "../lib/types";
import { open, save } from "@tauri-apps/plugin-dialog";

export default function ProfileList() {
  const profiles = useAppStore((s) => s.profiles);
  const setProfiles = useAppStore((s) => s.setProfiles);
  const addLog = useLogStore((s) => s.addLog);
  const [showCreate, setShowCreate] = useState(false);
  const [name, setName] = useState("");
  const [desc, setDesc] = useState("");
  const [game, setGame] = useState<SimsGame>("Sims4");
  const [gameFilter, setGameFilter] = useState<SimsGame | "all">("all");
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [comparison, setComparison] = useState<ProfileComparison | null>(null);

  useEffect(() => {
    cmd.listProfiles().then(setProfiles).catch(console.error);
  }, [setProfiles]);

  const filteredProfiles = useMemo(() => {
    if (gameFilter === "all") return profiles;
    return profiles.filter((p) => p.game === gameFilter);
  }, [profiles, gameFilter]);

  const handleCreate = async () => {
    if (!name.trim()) return;
    try {
      await cmd.saveProfileWithGame(name, desc, "\uD83D\uDCE6", game);
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
        defaultPath: `${profileName}.simshare-profile`,
        filters: [{ name: "SimShare Profile", extensions: ["simshare-profile"] }],
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
        filters: [{ name: "SimShare Profile", extensions: ["simshare-profile"] }],
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
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-xl font-bold">Mod Profiles</h2>
        <div className="flex gap-2">
          <button
            onClick={handleImport}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-card border border-border hover:bg-bg-card-hover text-sm transition-colors"
          >
            <Upload size={14} />
            Import
          </button>
        </div>
      </div>

      <div className="flex rounded-lg border border-border overflow-hidden w-fit">
        {(["all", "Sims2", "Sims3", "Sims4"] as const).map((g) => (
          <button
            key={g}
            onClick={() => setGameFilter(g)}
            className={`px-3 py-1.5 text-xs font-medium transition-colors ${
              gameFilter === g ? "bg-accent text-white" : "bg-bg-card text-txt-dim hover:bg-bg-card-hover"
            }`}
          >
            {g === "all" ? "All" : g === "Sims2" ? "Sims 2" : g === "Sims3" ? "Sims 3" : "Sims 4"}
          </button>
        ))}
      </div>

      {comparison && (
        <div className="bg-bg-card rounded-xl border border-accent/50 p-4 space-y-3">
          <div className="flex items-center justify-between">
            <h3 className="font-semibold text-sm">Profile Comparison: {comparison.profile_name}</h3>
            <button
              onClick={() => setComparison(null)}
              className="text-xs text-txt-dim hover:text-txt"
            >
              Close
            </button>
          </div>
          <div className="grid grid-cols-4 gap-3">
            <div className="text-center">
              <p className="text-lg font-bold text-status-green">{comparison.matched}</p>
              <p className="text-xs text-txt-dim">Matched</p>
            </div>
            <div className="text-center">
              <p className="text-lg font-bold text-status-red">{comparison.missing.length}</p>
              <p className="text-xs text-txt-dim">Missing</p>
            </div>
            <div className="text-center">
              <p className="text-lg font-bold text-status-yellow">{comparison.modified.length}</p>
              <p className="text-xs text-txt-dim">Modified</p>
            </div>
            <div className="text-center">
              <p className="text-lg font-bold text-txt-dim">{comparison.extra.length}</p>
              <p className="text-xs text-txt-dim">Extra</p>
            </div>
          </div>
          {comparison.missing.length > 0 && (
            <div>
              <p className="text-xs font-medium text-status-red mb-1">Missing mods:</p>
              <div className="max-h-32 overflow-y-auto space-y-0.5">
                {comparison.missing.map((p) => (
                  <p key={p} className="text-xs text-txt-dim font-mono truncate">{p.split("/").pop()}</p>
                ))}
              </div>
            </div>
          )}
          {comparison.modified.length > 0 && (
            <div>
              <p className="text-xs font-medium text-status-yellow mb-1">Modified mods:</p>
              <div className="max-h-32 overflow-y-auto space-y-0.5">
                {comparison.modified.map((p) => (
                  <p key={p} className="text-xs text-txt-dim font-mono truncate">{p.split("/").pop()}</p>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      <div className="grid grid-cols-2 gap-4">
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
          <div className="bg-bg-card rounded-xl border border-accent/50 p-4 space-y-3">
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              maxLength={64}
              placeholder="Profile name..."
              className="w-full bg-bg border border-border rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent"
            />
            <textarea
              value={desc}
              onChange={(e) => setDesc(e.target.value)}
              maxLength={256}
              placeholder="Description..."
              rows={2}
              className="w-full bg-bg border border-border rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent resize-none"
            />
            <div>
              <label className="text-xs text-txt-dim mb-1 block">Game</label>
              <select
                value={game}
                onChange={(e) => setGame(e.target.value as SimsGame)}
                className="w-full bg-bg border border-border rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent"
              >
                <option value="Sims4">Sims 4</option>
                <option value="Sims3">Sims 3</option>
                <option value="Sims2">Sims 2</option>
              </select>
            </div>
            <div className="flex gap-2">
              <button
                onClick={handleCreate}
                className="flex-1 bg-accent hover:bg-accent-light text-white rounded-lg px-3 py-2 text-sm font-medium transition-colors"
              >
                Save Profile
              </button>
              <button
                onClick={() => setShowCreate(false)}
                className="px-3 py-2 rounded-lg bg-bg-card-hover text-txt-dim text-sm transition-colors"
              >
                Cancel
              </button>
            </div>
          </div>
        ) : (
          <button
            onClick={() => setShowCreate(true)}
            className="bg-bg-card rounded-xl border border-dashed border-border hover:border-accent/50 p-6 flex flex-col items-center justify-center gap-2 text-txt-dim hover:text-accent-light transition-colors min-h-[140px]"
          >
            <Plus size={24} />
            <span className="text-sm">Create New Profile</span>
          </button>
        )}
      </div>
    </div>
  );
}
