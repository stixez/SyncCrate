import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { isPermissionGranted, requestPermission, sendNotification as sendOsNotification } from "@tauri-apps/plugin-notification";
import { useAppStore } from "../stores/useAppStore";

/** Desktop notification, only when enabled and the window is in the background. */
export async function sendNotification(title: string, body: string) {
  try {
    if (!useAppStore.getState().notificationsEnabled) return;
    const win = getCurrentWebviewWindow();
    const [focused, visible] = await Promise.all([win.isFocused(), win.isVisible()]);
    if (focused && visible) return;
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === "granted";
    if (granted) sendOsNotification({ title, body });
  } catch {
    // Notifications not supported in this environment
  }
}
