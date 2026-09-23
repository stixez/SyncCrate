import type { GameDefinition } from "./types";

// Game registry loaded from the backend at startup.
let _registry: GameDefinition[] = [];
let _map: Map<string, GameDefinition> = new Map();

/** Initialize the game registry. Called once on app mount. */
export function setGameRegistry(games: GameDefinition[]): void {
  _registry = games;
  _map = new Map(games.map((g) => [g.id, g]));
}

/** Get the full list of registered games. */
export function getGameRegistry(): GameDefinition[] {
  return _registry;
}

/** Lookup a single game definition by ID. */
export function getGameDef(id: string): GameDefinition | undefined {
  return _map.get(id);
}

/** Human-readable label for a game ID. */
export function gameLabel(id: string): string {
  return _map.get(id)?.label ?? id;
}

/** Tailwind color class for a game. */
export function gameColor(id: string): string {
  return _map.get(id)?.color ?? "text-gray-400";
}

/** Hex color for dynamic accent theming. */
export function gamePrimaryColor(id: string): string {
  return _map.get(id)?.primary_color ?? "#1fb87e";
}

/** Game family (e.g., "sims", "wow", "minecraft"). */
export function gameFamily(id: string): string {
  return _map.get(id)?.family ?? "";
}

/** Check if a game belongs to a given family. */
export function isFamily(id: string, family: string): boolean {
  return _map.get(id)?.family === family;
}

/** Whether the game has pack detection support. */
export function hasPacks(id: string): boolean {
  return !!_map.get(id)?.packs;
}
