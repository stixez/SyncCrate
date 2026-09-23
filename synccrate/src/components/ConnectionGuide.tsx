import { Wifi, Globe, AlertTriangle } from "lucide-react";
import { open } from "@tauri-apps/plugin-shell";
import NetworkHealth from "./NetworkHealth";

export default function ConnectionGuide() {
  return (
    <div className="space-y-4">
      <NetworkHealth />
      <div className="grid grid-cols-2 gap-4">
        <div className="bg-bg-card rounded-xl border border-border p-5">
          <div className="flex items-center gap-2 mb-3">
            <Wifi size={18} className="text-status-green" />
            <h3 className="font-semibold text-sm">Same Network</h3>
          </div>
          <p className="text-xs text-txt-dim leading-relaxed">
            If you and your friends are on the same Wi-Fi or LAN, SyncCrate finds hosts automatically.
            Have one person host and the others click Scan. If a host doesn't show up, paste the
            <span className="font-medium text-txt"> join code</span> from the host's screen instead.
          </p>
        </div>
        <div className="bg-bg-card rounded-xl border border-border p-5">
          <div className="flex items-center gap-2 mb-3">
            <Globe size={18} className="text-accent-light" />
            <h3 className="font-semibold text-sm">Different Location</h3>
          </div>
          <p className="text-xs text-txt-dim leading-relaxed">
            Friends somewhere else can join too, with no VPN or port forwarding. The host shares their
            <span className="font-medium text-txt"> join code</span> and friends paste it. SyncCrate connects directly
            when it can, and otherwise through an encrypted relay that can't read your files.
          </p>
          <p className="text-[11px] text-txt-muted leading-relaxed mt-2">
            Already use{" "}
            <button onClick={() => open("https://tailscale.com").catch(() => {})} className="text-accent-light hover:underline inline">Tailscale</button>{" "}
            or{" "}
            <button onClick={() => open("https://www.zerotier.com").catch(() => {})} className="text-accent-light hover:underline inline">ZeroTier</button>?
            Connect by IP with the host's VPN address also works.
          </p>
        </div>
      </div>
      <div className="bg-bg-card rounded-xl border border-border p-4 flex items-start gap-3">
        <AlertTriangle size={16} className="text-status-yellow shrink-0 mt-0.5" />
        <div>
          <h4 className="text-xs font-semibold mb-1">Still not connecting?</h4>
          <ul className="text-[11px] text-txt-dim leading-relaxed space-y-1 list-disc list-inside">
            <li>On the <span className="font-medium text-txt">host</span> PC, use the Network Check above — "Fix Windows Firewall" solves most failed connections</li>
            <li>Use <span className="font-medium text-txt">Test</span> above with the host's IP to see whether it's reachable</li>
            <li>Make sure both players are running the <span className="font-medium text-txt">same version</span> of SyncCrate</li>
            <li>Verify the host is actively hosting (green "Hosting" status)</li>
            <li>Guest Wi-Fi and some mesh routers isolate devices from each other — use the main network or a VPN</li>
            <li>If using a custom port, make sure both sides use the same port number</li>
            <li>Tailscale/ZeroTier: check that both devices show as "Connected" in the VPN app</li>
          </ul>
        </div>
      </div>
    </div>
  );
}
