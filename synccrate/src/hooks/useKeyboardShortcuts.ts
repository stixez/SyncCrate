import { useEffect } from "react";
import { useAppStore } from "../stores/useAppStore";

export function useKeyboardShortcuts() {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const isInput = target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable;
      const mod = e.metaKey || e.ctrlKey;

      // Ctrl/Cmd + Shift + S → Settings
      if (mod && e.shiftKey && e.key === "S") {
        e.preventDefault();
        useAppStore.getState().navigateToGlobal("settings");
        return;
      }

      // Ctrl/Cmd + Shift + F → Content page + focus search
      if (mod && e.shiftKey && e.key === "F") {
        e.preventDefault();
        const state = useAppStore.getState();
        if (state.selectedGame) {
          state.navigateToGame(state.selectedGame, "content");
        }
        requestAnimationFrame(() => {
          const searchInput = document.querySelector<HTMLInputElement>(
            'input[type="text"][placeholder*="Search"]'
          );
          searchInput?.focus();
        });
        return;
      }

      // "/" → focus search on current page (only if not in an input)
      if (e.key === "/" && !isInput) {
        const searchInput = document.querySelector<HTMLInputElement>(
          'input[type="search"], input[aria-label^="Search" i], input[placeholder*="Search"], input[placeholder*="Find"]'
        );
        // Otherwise leave the key alone (it was swallowed on pages without one).
        if (searchInput) {
          e.preventDefault();
          searchInput.focus();
        }
        return;
      }

      // Escape → close modals
      if (e.key === "Escape" && !isInput) {
        useAppStore.getState().setShowDonate(false);
        return;
      }
    };

    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);
}
