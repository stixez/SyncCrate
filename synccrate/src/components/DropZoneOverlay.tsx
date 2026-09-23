import { Download } from "lucide-react";

export default function DropZoneOverlay() {
  return (
    <div className="fixed inset-0 z-[100] bg-bg/85 backdrop-blur-sm flex items-center justify-center pointer-events-none p-10">
      <div className="corner-brackets w-full max-w-xl">
        <div className="border border-dashed border-neon/60 bg-neon/[0.04] px-10 py-12 flex flex-col items-center text-center">
          <div className="w-16 h-16 grid place-items-center border border-neon/50 bg-bg mb-5">
            <Download size={30} className="text-neon animate-bounce" />
          </div>
          <p className="hud-label mb-2"><b>//</b> Drop zone armed</p>
          <p className="display text-[2rem] text-txt">
            Drop to <span className="text-neon">install</span>
          </p>
          <p className="font-mono text-[11px] tracking-[0.08em] text-txt-dim mt-4">
            .package · .ts4script · .zip · .sims3pack
          </p>
        </div>
      </div>
    </div>
  );
}
