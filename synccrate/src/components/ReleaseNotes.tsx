import { useEffect, useState } from "react";
import { ChevronDown, ExternalLink } from "lucide-react";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import pkg from "../../package.json";
import * as cmd from "../lib/commands";
import { RELEASES } from "../lib/changelog";
import Markdown from "./Markdown";
import { Badge, Button, Panel, SectionHeader } from "./ui";

/** Versions shown in full before "Show older versions". */
const OPEN = 3;

function formatDate(date: string): string {
  const d = new Date(`${date}T12:00:00`);
  return Number.isNaN(d.getTime()) ? date : d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

export default function ReleaseNotes() {
  const [version, setVersion] = useState(pkg.version);
  const [showAll, setShowAll] = useState(false);
  useEffect(() => {
    cmd.getAppVersion().then(setVersion).catch(() => {});
  }, []);

  const shown = showAll ? RELEASES : RELEASES.slice(0, OPEN);

  return (
    <div className="max-w-[860px] mx-auto space-y-6">
      <SectionHeader
        label={<><b>// Release notes</b> &nbsp;Changelog</>}
        title={<>Release <span className="text-neon">notes</span></>}
        description={<>What changed in each version, newest first. You're on <span className="text-neon font-mono">v{version}</span>; the app updates itself.</>}
        actions={
          <Button
            size="sm"
            variant="ghost"
            onClick={() => openUrl("https://github.com/stixez/SyncCrate/releases").catch(() => {})}
            icon={<ExternalLink size={12} />}
          >
            Releases on GitHub
          </Button>
        }
      />

      {shown.map((r) => (
        <Panel
          key={r.version}
          title={`v${r.version}`}
          label={r.date ? `// ${formatDate(r.date)}` : undefined}
          actions={r.version === version ? <Badge tone="neon">Installed</Badge> : undefined}
        >
          <Markdown source={r.body} />
        </Panel>
      ))}

      {!showAll && RELEASES.length > OPEN && (
        <div className="flex justify-center">
          <Button variant="ghost" onClick={() => setShowAll(true)} icon={<ChevronDown size={14} />}>
            Show {RELEASES.length - OPEN} older versions
          </Button>
        </div>
      )}
    </div>
  );
}
