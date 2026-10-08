import { open as openUrl } from "@tauri-apps/plugin-shell";
import { toastError } from "./toast";

/** Creator links come from another PC (a host, a pack's author) or a mod's
 * own metadata: only ever hand https to the browser. */
export function isHttps(url?: string | null): url is string {
  return !!url && /^https:\/\//i.test(url);
}

/** Opens a creator's page (or a mod's update, changelog or dependency link) in
 * the browser; anything but https is ignored, and a failure shows a toast. */
export function openCreatorLink(url?: string | null) {
  if (!isHttps(url)) return;
  openUrl(url).catch((e) => toastError(`Couldn't open the link: ${e}`));
}
