import { Wifi, Globe, AlertTriangle } from "lucide-react";
import { open } from "@tauri-apps/plugin-shell";
import NetworkHealth from "./NetworkHealth";
import { Panel } from "./ui";

const TROUBLESHOOTING = [
  <>On the <b className="font-medium text-txt">host</b> PC, use the Network Check above — "Fix Windows Firewall" solves most failed connections</>,
  <>Use <b className="font-medium text-txt">Test</b> above with the host's IP to see whether it's reachable</>,
  <>Make sure both players are running the <b className="font-medium text-txt">same version</b> of SyncCrate</>,
  <>Verify the host is actively hosting (green "Hosting" status)</>,
  <>Guest Wi-Fi and some mesh routers isolate devices from each other — use the main network or a VPN</>,
  <>If using a custom port, make sure both sides use the same port number</>,
  <>Tailscale/ZeroTier: check that both devices show as "Connected" in the VPN app</>,
];

export default function ConnectionGuide() {
  return (
    <div className="space-y-4">
      <NetworkHealth />
      <div className="grid grid-cols-2 gap-4">
        <Panel label={<><b>A</b> &nbsp;LAN</>} title="Same network" icon={<Wifi size={15} className="text-neon" />}>
          <p className="text-xs text-txt-dim leading-relaxed">
            If you and your friends are on the same Wi-Fi or LAN, SyncCrate finds hosts automatically.
            Have one person host and the others click Scan for Hosts. If a host doesn't show up, paste the
            <span className="font-medium text-txt"> join code</span> from the host's screen instead.
          </p>
        </Panel>
        <Panel label={<><b>B</b> &nbsp;Internet</>} title="Different location" icon={<Globe size={15} className="text-neon" />}>
          <p className="text-xs text-txt-dim leading-relaxed">
            Friends somewhere else can join too, with no VPN or port forwarding. The host shares their
            <span className="font-medium text-txt"> join code</span> and friends paste it. SyncCrate connects directly
            when it can, and otherwise through an encrypted relay that can't read your files.
          </p>
          <p className="text-[11px] text-txt-muted leading-relaxed mt-3 pt-3 border-t border-border">
            Already use{" "}
            <button onClick={() => open("https://tailscale.com").catch(() => {})} className="text-neon hover:underline inline">Tailscale</button>{" "}
            or{" "}
            <button onClick={() => open("https://www.zerotier.com").catch(() => {})} className="text-neon hover:underline inline">ZeroTier</button>?
            Connect by IP with the host's VPN address also works.
          </p>
        </Panel>
      </div>
      <div className="border border-border border-l-2 border-l-amber bg-amber/[0.05] p-4 flex items-start gap-3">
        <AlertTriangle size={16} className="text-amber shrink-0 mt-0.5" />
        <div className="min-w-0">
          <p className="hud-label !text-amber mb-2">// Still not connecting?</p>
          <ol className="text-[11.5px] text-txt-dim leading-relaxed space-y-1.5">
            {TROUBLESHOOTING.map((tip, i) => (
              <li key={i} className="flex gap-2.5">
                <span className="font-mono text-[10.5px] text-txt-muted tabular shrink-0 pt-px">{String(i + 1).padStart(2, "0")}</span>
                <span>{tip}</span>
              </li>
            ))}
          </ol>
        </div>
      </div>
    </div>
  );
}
