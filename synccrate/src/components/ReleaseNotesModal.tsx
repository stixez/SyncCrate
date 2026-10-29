import { ArrowRight, Sparkles, X } from "lucide-react";
import { useAppStore } from "../stores/useAppStore";
import { useDialog } from "../hooks/useDialog";
import { digest, type Release } from "../lib/releaseNotes";
import { inline } from "./Markdown";
import { Button } from "./ui";

/** Shown once after SyncCrate updates: the new version's summary and first
 *  highlights, with the full notes a click away. */
export default function ReleaseNotesModal({ release, onClose }: { release: Release; onClose: () => void }) {
  const dialogRef = useDialog(onClose);
  const { summary, items } = digest(release.body);

  const seeAll = () => {
    onClose();
    useAppStore.getState().navigateToGlobal("release-notes");
  };

  return (
    <div
      className="fixed inset-0 bg-black/65 backdrop-blur-[2px] flex items-center justify-center z-50 p-4"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-label={`What changed in SyncCrate ${release.version}`}
    >
      <div ref={dialogRef} className="corner-brackets max-w-lg w-full" onClick={(e) => e.stopPropagation()}>
        <div className="panel shadow-2xl">
          <div className="p-6">
            <div className="flex items-start justify-between gap-3 mb-4">
              <div>
                <p className="hud-label mb-1.5 flex items-center gap-1.5">
                  <Sparkles size={11} className="text-neon" />
                  <span><b>//</b> Just updated</span>
                </p>
                <h3 className="display text-[1.6rem]">
                  SyncCrate <span className="text-neon">{release.version}</span>
                </h3>
              </div>
              <button
                onClick={onClose}
                className="w-8 h-8 grid place-items-center text-txt-dim hover:text-txt hover:bg-bg-card-hover transition-colors"
                aria-label="Close"
              >
                <X size={18} />
              </button>
            </div>

            {summary && <p className="text-sm text-txt-dim mb-4">{inline(summary)}</p>}

            {items.length > 0 && (
              <ul className="space-y-2.5 mb-5 max-h-[50vh] overflow-y-auto pr-1">
                {items.map((item, i) => {
                  const bold = /^\*\*(.+?)\*\*\s*(.*)$/.exec(item);
                  return (
                    <li key={i} className="flex gap-2.5 text-[12.5px] leading-relaxed">
                      <span className="text-neon shrink-0 select-none">▸</span>
                      <div className="min-w-0">
                        {bold ? (
                          <>
                            <p className="font-medium text-txt">{inline(bold[1])}</p>
                            {bold[2] && <p className="text-txt-dim line-clamp-2">{inline(bold[2])}</p>}
                          </>
                        ) : (
                          <p className="text-txt-dim line-clamp-3">{inline(item)}</p>
                        )}
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}

            <div className="flex items-center justify-end gap-2">
              <Button variant="ghost" onClick={seeAll} icon={<ArrowRight size={14} />}>
                All release notes
              </Button>
              <Button variant="primary" onClick={onClose}>
                Got it
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
