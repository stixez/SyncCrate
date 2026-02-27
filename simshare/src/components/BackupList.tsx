import { useState, useEffect } from "react";
import { Archive, Plus, RotateCcw, Trash2 } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { useLogStore } from "../stores/useLogStore";
import { formatBytes, formatDate } from "../lib/utils";
import * as cmd from "../lib/commands";

const GAME_LABELS: Record<string, string> = {
  Sims2: "Sims 2",
  Sims3: "Sims 3",
  Sims4: "Sims 4",
};

const GAME_COLORS: Record<string, string> = {
  Sims2: "bg-status-yellow/20 text-status-yellow",
  Sims3: "bg-status-green/20 text-status-green",
  Sims4: "bg-accent/20 text-accent-light",
};

export default function BackupList() {
  const backups = useAppStore((s) => s.backups);
  const setBackups = useAppStore((s) => s.setBackups);
  const addLog = useLogStore((s) => s.addLog);

  const [label, setLabel] = useState("");
  const [showCreate, setShowCreate] = useState(false);
  const [creating, setCreating] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [restoreConfirm, setRestoreConfirm] = useState<string | null>(null);

  useEffect(() => {
    cmd.listBackups().then(setBackups).catch(console.error);
  }, [setBackups]);

  const handleCreate = async () => {
    if (!label.trim()) return;
    setCreating(true);
    try {
      await cmd.createBackup(label.trim());
      const updated = await cmd.listBackups();
      setBackups(updated);
      setLabel("");
      setShowCreate(false);
      addLog(`Backup "${label.trim()}" created`, "success");
    } catch (e) {
      addLog(`Backup failed: ${e}`, "error");
    } finally {
      setCreating(false);
    }
  };

  const handleRestore = async (id: string) => {
    if (restoreConfirm !== id) {
      setRestoreConfirm(id);
      return;
    }
    setRestoreConfirm(null);
    setRestoring(true);
    try {
      await cmd.restoreBackup(id);
      const updated = await cmd.listBackups();
      setBackups(updated);
      addLog("Backup restored (safety backup created)", "success");
    } catch (e) {
      addLog(`Restore failed: ${e}`, "error");
    } finally {
      setRestoring(false);
    }
  };

  const handleDelete = async (id: string) => {
    if (deleteConfirm !== id) {
      setDeleteConfirm(id);
      return;
    }
    setDeleteConfirm(null);
    try {
      await cmd.deleteBackup(id);
      const updated = await cmd.listBackups();
      setBackups(updated);
      addLog("Backup deleted", "info");
    } catch (e) {
      addLog(`Delete failed: ${e}`, "error");
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-xl font-bold">Backups</h2>
        {!showCreate && (
          <button
            onClick={() => setShowCreate(true)}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-light text-white text-sm font-medium transition-colors"
          >
            <Plus size={14} />
            Create Backup
          </button>
        )}
      </div>

      {showCreate && (
        <div className="bg-bg-card rounded-xl border border-accent/50 p-4 space-y-3">
          <h3 className="font-semibold text-sm">New Backup</h3>
          <input
            type="text"
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            maxLength={128}
            placeholder="Backup label (e.g. Before installing new mods)..."
            className="w-full bg-bg border border-border rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-accent"
          />
          <p className="text-xs text-txt-dim">
            This will back up your Mods, Saves, Tray, and Screenshots folders.
          </p>
          <div className="flex gap-2">
            <button
              onClick={handleCreate}
              disabled={creating || !label.trim()}
              className="bg-accent hover:bg-accent-light text-white rounded-lg px-4 py-2 text-sm font-medium transition-colors disabled:opacity-50"
            >
              {creating ? "Creating..." : "Create Backup"}
            </button>
            <button
              onClick={() => setShowCreate(false)}
              className="px-3 py-2 rounded-lg bg-bg-card-hover text-txt-dim text-sm transition-colors"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {restoring && (
        <div className="bg-accent/10 border border-accent/30 rounded-xl p-4 text-center">
          <p className="text-sm font-medium text-accent-light">Restoring backup...</p>
          <p className="text-xs text-txt-dim mt-1">A safety backup is being created first.</p>
        </div>
      )}

      <div className="space-y-3">
        {backups.length === 0 ? (
          <div className="text-center py-12 text-txt-dim">
            <Archive size={40} className="mx-auto mb-3 opacity-40" />
            <p>No backups yet</p>
            <p className="text-xs mt-1">Create a backup to save the current state of your mods and saves</p>
          </div>
        ) : (
          backups.map((backup) => (
            <div
              key={backup.id}
              className="bg-bg-card rounded-xl border border-border p-4"
            >
              <div className="flex items-start justify-between">
                <div>
                  <div className="flex items-center gap-2">
                    <h3 className="font-semibold text-sm">{backup.label}</h3>
                    <span className={`text-[10px] px-1.5 py-0.5 rounded-full font-medium ${GAME_COLORS[backup.game] || GAME_COLORS.Sims4}`}>
                      {GAME_LABELS[backup.game] || "Sims 4"}
                    </span>
                  </div>
                  <p className="text-xs text-txt-dim mt-1">
                    {formatDate(backup.created_at)}
                  </p>
                </div>
                <div className="flex gap-2">
                  <button
                    onClick={() => handleRestore(backup.id)}
                    disabled={restoring}
                    className={`flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-medium transition-colors ${
                      restoreConfirm === backup.id
                        ? "bg-status-yellow/20 text-status-yellow"
                        : "bg-bg border border-border text-txt-dim hover:bg-bg-card-hover"
                    } disabled:opacity-50`}
                  >
                    <RotateCcw size={12} />
                    {restoreConfirm === backup.id ? "Confirm Restore?" : "Restore"}
                  </button>
                  <button
                    onClick={() => handleDelete(backup.id)}
                    className={`flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-medium transition-colors ${
                      deleteConfirm === backup.id
                        ? "bg-status-red/20 text-status-red"
                        : "bg-bg border border-border text-txt-dim hover:bg-bg-card-hover"
                    }`}
                  >
                    <Trash2 size={12} />
                    {deleteConfirm === backup.id ? "Confirm?" : "Delete"}
                  </button>
                </div>
              </div>
              <div className="flex gap-4 mt-2 text-xs text-txt-dim">
                <span>{backup.file_count} files</span>
                <span>{formatBytes(backup.total_size)}</span>
                <span>{backup.mods_count} mods</span>
                <span>{backup.saves_count} saves</span>
                {!!backup.tray_count && <span>{backup.tray_count} tray</span>}
                {!!backup.screenshots_count && <span>{backup.screenshots_count} screenshots</span>}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
