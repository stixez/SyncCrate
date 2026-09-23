import { useCallback, useEffect, useState } from "react";
import { Shield, ShieldCheck, ShieldAlert, Loader2, Network, PlugZap } from "lucide-react";
import type { ConnectionTestResult, FirewallStatus, NetworkDiagnostics } from "../lib/types";
import { toastError, toastSuccess } from "../lib/toast";
import * as cmd from "../lib/commands";
import { Button, Input, Panel } from "./ui";

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
      <div className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-[0.08em] text-status-green">
        <ShieldCheck size={14} />
        {status.checked ? "Windows Firewall allows SyncCrate" : "Firewall status unknown"}
      </div>
    );
  }

  return (
    <div className="border border-status-red/40 border-l-2 border-l-status-red bg-status-red/[0.07] p-3.5 flex items-start gap-3" role="alert">
      <ShieldAlert size={16} className="text-status-red shrink-0 mt-0.5" />
      <div className="flex-1 min-w-0">
        <p className="font-display font-semibold uppercase tracking-[0.05em] text-[13px] text-status-red leading-snug">
          {status.has_block_rule ? "Windows Firewall is blocking SyncCrate" : "SyncCrate isn't allowed through Windows Firewall"}
        </p>
        <p className="text-[11.5px] text-txt-dim leading-relaxed mt-1">
          Friends won't be able to connect to you until this is fixed. You don't need to run SyncCrate as administrator —
          click Fix and approve the one-time Windows prompt.
        </p>
      </div>
      <Button
        variant="danger"
        size="sm"
        className="shrink-0"
        onClick={fix}
        disabled={fixing}
        icon={fixing ? <Loader2 size={12} className="animate-spin" /> : <Shield size={12} />}
      >
        {fixing ? "Fixing..." : "Fix Windows Firewall"}
      </Button>
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
    <Panel label="// Diagnostics" title="Network check" icon={<Network size={15} className="text-neon" />} bodyClassName="space-y-4">
      <FirewallCheck />

      {diag && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
          <span className="hud-label">This PC</span>
          <span className="font-mono text-xs text-txt">
            {realInterfaces.length === 0
              ? "no network connection found"
              : realInterfaces.map((i) => i.ip).join(", ")}
          </span>
          <span className="font-mono text-[11px] text-txt-muted">port {diag.session_port}</span>
        </div>
      )}

      <div>
        <p className="hud-label mb-1.5">Test if a host is reachable (without joining)</p>
        <div className="flex gap-2">
          <Input
            mono
            size="sm"
            value={testIp}
            onChange={(e) => setTestIp(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && runTest()}
            placeholder="Host IP, e.g. 192.168.1.20"
            aria-label="Host IP to test"
            wrapperClassName="flex-1 min-w-0"
          />
          <Input
            mono
            size="sm"
            value={testPort}
            onChange={(e) => setTestPort(e.target.value.replace(/\D/g, ""))}
            aria-label="Port to test"
            wrapperClassName="w-20"
            className="text-center"
          />
          <Button
            size="sm"
            className="!h-[30px]"
            onClick={runTest}
            disabled={testing || !testIp.trim()}
            icon={testing ? <Loader2 size={12} className="animate-spin" /> : <PlugZap size={12} />}
          >
            Test
          </Button>
        </div>
        {testResult && (
          <p
            className={`font-mono text-[11px] mt-2 leading-relaxed border-l-2 pl-2.5 ${
              testResult.reachable ? "text-status-green border-l-status-green" : "text-amber border-l-amber"
            }`}
          >
            {testResult.message}
            {testResult.latency_ms != null && ` (${testResult.latency_ms} ms)`}
          </p>
        )}
      </div>
    </Panel>
  );
}
