import { useCallback, useEffect, useState } from "react";
import { Shield, ShieldCheck, ShieldAlert, Loader2, Network, PlugZap } from "lucide-react";
import type { ConnectionTestResult, FirewallStatus, NetworkDiagnostics } from "../lib/types";
import { toastError, toastSuccess } from "../lib/toast";
import * as cmd from "../lib/commands";

/** True when the firewall is known to be blocking (or not allowing) inbound connections. */
export function firewallNeedsFix(fw: FirewallStatus | null): boolean {
  return !!fw && fw.supported && fw.checked && (fw.has_block_rule || !fw.has_allow_rule);
}

/**
 * Windows Firewall status with a one-click fix. `compact` renders nothing when
 * everything is fine (used on the hosting panel).
 */
export function FirewallCheck({ compact = false }: { compact?: boolean }) {
  const [status, setStatus] = useState<FirewallStatus | null>(null);
  const [fixing, setFixing] = useState(false);

  useEffect(() => {
    let cancelled = false;
    cmd.getFirewallStatus().then((s) => { if (!cancelled) setStatus(s); }).catch(() => {});
    return () => { cancelled = true; };
  }, []);

  const fix = async () => {
    setFixing(true);
    try {
      const s = await cmd.fixFirewall();
      setStatus(s);
      toastSuccess("Windows Firewall now allows SyncCrate");
    } catch (e) {
      toastError(String(e));
    } finally {
      setFixing(false);
    }
  };

  if (!status || !status.supported) return null;
  const needsFix = firewallNeedsFix(status);
  if (compact && !needsFix) return null;

  if (!needsFix) {
    return (
      <div className="flex items-center gap-2 text-xs text-status-green">
        <ShieldCheck size={14} />
        {status.checked ? "Windows Firewall allows SyncCrate" : "Firewall status unknown"}
      </div>
    );
  }

  return (
    <div className="bg-status-red/5 rounded-xl border border-status-red/20 p-3 flex items-start gap-3">
      <ShieldAlert size={16} className="text-status-red shrink-0 mt-0.5" />
      <div className="flex-1 min-w-0">
        <p className="text-xs font-semibold text-status-red">
          {status.has_block_rule ? "Windows Firewall is blocking SyncCrate" : "SyncCrate isn't allowed through Windows Firewall"}
        </p>
        <p className="text-[11px] text-txt-dim leading-relaxed mt-0.5">
          Friends won't be able to connect to you until this is fixed. You don't need to run SyncCrate as administrator —
          click Fix and approve the one-time Windows prompt.
        </p>
      </div>
      <button
        onClick={fix}
        disabled={fixing}
        className="shrink-0 flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-status-red/15 hover:bg-status-red/25 text-status-red text-xs font-medium transition-colors disabled:opacity-50"
      >
        {fixing ? <Loader2 size={12} className="animate-spin" /> : <Shield size={12} />}
        {fixing ? "Fixing..." : "Fix Windows Firewall"}
      </button>
    </div>
  );
}

/** Full network diagnostics panel shown in the connection help section. */
export default function NetworkHealth() {
  const [diag, setDiag] = useState<NetworkDiagnostics | null>(null);
  const [testIp, setTestIp] = useState("");
  const [testPort, setTestPort] = useState("9847");
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<ConnectionTestResult | null>(null);

  const load = useCallback(() => {
    cmd.getNetworkDiagnostics().then((d) => {
      setDiag(d);
      setTestPort((p) => (p === "9847" ? String(d.session_port) : p));
    }).catch(() => {});
  }, []);

  useEffect(load, [load]);

  const runTest = async () => {
    const port = parseInt(testPort, 10);
    if (!testIp.trim() || !port) return;
    setTesting(true);
    setTestResult(null);
    try {
      setTestResult(await cmd.testConnection(testIp.trim(), port));
    } catch (e) {
      setTestResult({ reachable: false, message: String(e), latency_ms: null });
    } finally {
      setTesting(false);
    }
  };

  const realInterfaces = diag?.interfaces.filter((i) => !i.is_virtual) ?? [];

  return (
    <div className="bg-bg-card rounded-xl border border-border p-4 space-y-3">
      <div className="flex items-center gap-2">
        <Network size={16} className="text-accent-light" />
        <h4 className="text-xs font-semibold">Network Check</h4>
      </div>

      <FirewallCheck />

      {diag && (
        <div className="text-[11px] text-txt-dim">
          <span className="font-medium text-txt">This PC: </span>
          {realInterfaces.length === 0
            ? "no network connection found"
            : realInterfaces.map((i) => i.ip).join(", ")}
          <span className="text-txt-muted"> · port {diag.session_port}</span>
        </div>
      )}

      <div>
        <p className="text-[11px] text-txt-dim mb-1.5">Test if a host is reachable (without joining):</p>
        <div className="flex gap-2">
          <input
            value={testIp}
            onChange={(e) => setTestIp(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && runTest()}
            placeholder="Host IP, e.g. 192.168.1.20"
            className="flex-1 min-w-0 bg-bg-elevated border border-border rounded-lg px-2.5 py-1.5 text-xs font-mono focus:outline-none focus:border-accent/50"
          />
          <input
            value={testPort}
            onChange={(e) => setTestPort(e.target.value.replace(/\D/g, ""))}
            className="w-20 bg-bg-elevated border border-border rounded-lg px-2.5 py-1.5 text-xs font-mono focus:outline-none focus:border-accent/50"
          />
          <button
            onClick={runTest}
            disabled={testing || !testIp.trim()}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent/15 hover:bg-accent/25 text-accent-light text-xs font-medium transition-colors disabled:opacity-50"
          >
            {testing ? <Loader2 size={12} className="animate-spin" /> : <PlugZap size={12} />}
            Test
          </button>
        </div>
        {testResult && (
          <p className={`text-[11px] mt-1.5 leading-relaxed ${testResult.reachable ? "text-status-green" : "text-status-yellow"}`}>
            {testResult.message}
            {testResult.latency_ms != null && ` (${testResult.latency_ms} ms)`}
          </p>
        )}
      </div>
    </div>
  );
}
