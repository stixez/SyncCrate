import { Download } from "lucide-react";

export default function DropZoneOverlay() {
  return (
    <div className="fixed inset-0 z-[100] bg-bg/80 backdrop-blur-sm flex items-center justify-center pointer-events-none">
      <div className="border-2 border-dashed border-accent rounded-2xl p-12 flex flex-col items-center gap-4">
        <Download size={48} className="text-accent-light animate-bounce" />
        <p className="text-lg font-semibold text-accent-light">Drop files to install</p>
        <p className="text-sm text-txt-dim">
          Supported: .package, .ts4script, .zip, .sims3pack
        </p>
      </div>
    </div>
  );
}
