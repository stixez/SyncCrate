/**
 * Turn raw backend / OS error text into something a non-technical player can
 * act on. Applied to every error toast (`toastError`), so call sites can keep
 * passing `${e}`. Unknown messages pass through unchanged (minus an "Error: "
 * prefix), since most backend errors are already written for users.
 */

/** Windows (and a few POSIX) error codes that show up in io errors as "(os error N)". */
const OS_ERRORS: Record<number, string> = {
  5: "SyncCrate isn't allowed to change this file or folder. It may be read-only or in a protected folder; try Settings → Restart as admin.",
  13: "SyncCrate isn't allowed to change this file or folder.",
  32: "A file is in use by another program, usually the game. Close it and try again.",
  33: "A file is in use by another program, usually the game. Close it and try again.",
  1224: "A file is in use by another program, usually the game. Close it and try again.",
  112: "Your drive is full. Free up some space and try again.",
  39: "Your drive is full. Free up some space and try again.",
  28: "Your drive is full. Free up some space and try again.",
  206: "A path is too long for Windows. Move the mod into a shorter folder and try again.",
  225: "Windows Security blocked a file as a possible threat.",
  10053: "The connection to the other PC dropped. Reconnect and try again.",
  10054: "The connection to the other PC dropped. Reconnect and try again.",
};

/** Backend wording that's accurate but technical, by substring. */
const PHRASES: [RegExp, string][] = [
  [/^(Peer not found|Peer disconnected|No active connections)$/i, "You're no longer connected to the host. Reconnect and try again."],
  [/No remote manifest available/i, "Still waiting for the host's file list. Try again in a moment."],
  [/No sync plan (computed|available)/i, "This sync list is out of date. Click Compare & Sync again."],
  [/Multiple peers connected/i, "Choose which friend to sync with."],
  [/Connection timed out (reading|writing) message/i, "The other PC stopped responding. Check both are still online and try again."],
  [/Connection closed by peer/i, "The other PC closed the connection."],
];

export function friendlyError(e: unknown): string {
  let msg = (e instanceof Error ? e.message : String(e ?? "")).trim().replace(/^Error:\s*/, "");
  const os = msg.match(/\(os error (\d+)\)/);
  if (os) {
    const plain = OS_ERRORS[Number(os[1])];
    if (plain) {
      // Keep the file name / action the backend put in front of the OS text.
      const context = msg.slice(0, os.index).replace(/[:\s-]+$/, "").trim();
      return context && context.length < 120 ? `${plain} (${context})` : plain;
    }
  }
  for (const [re, plain] of PHRASES) {
    if (re.test(msg)) return plain;
  }
  if (msg.length > 400) msg = `${msg.slice(0, 400)}…`;
  return msg || "Something went wrong.";
}

/** "Couldn't save: The drive is full." from a prefix and a raw error. */
export function withContext(prefix: string, e: unknown): string {
  return `${prefix}: ${friendlyError(e)}`;
}
