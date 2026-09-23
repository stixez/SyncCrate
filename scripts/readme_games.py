"""Regenerate the Supported Games section of README.md from the game registry.

Usage (from the repo root):  python scripts/readme_games.py

Rewrites everything between the <!-- GAMES:START --> and <!-- GAMES:END -->
markers. Games are grouped by their first genre so the list stays scannable;
each genre is a collapsible <details> block. Line endings are preserved.
"""

import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
REGISTRY = ROOT / "synccrate" / "src-tauri" / "src" / "game_registry.json"
README = ROOT / "README.md"
START, END = "<!-- GAMES:START -->", "<!-- GAMES:END -->"

# Order and labels for the README groups (matches GENRE_LABELS in src/lib/games.ts).
GROUPS = [
    ("life-sim", "Life sim"),
    ("survival", "Survival & co-op"),
    ("sandbox", "Sandbox"),
    ("automation", "Automation"),
    ("rpg", "RPG"),
    ("mmo", "MMO"),
    ("strategy", "Strategy"),
    ("city-builder", "City builders"),
    ("simulation", "Simulation"),
    ("shooter", "Shooters"),
    ("action", "Action"),
    ("horror", "Horror"),
    ("roguelike", "Roguelikes"),
    ("racing", "Racing"),
    ("platformer", "Platformers"),
    ("party", "Party"),
    ("rhythm", "Rhythm"),
]


def main() -> None:
    games = json.loads(REGISTRY.read_text(encoding="utf-8"))["games"]
    order = [key for key, _ in GROUPS]
    buckets: dict[str, list[dict]] = {key: [] for key in order}
    for g in games:
        first = (g.get("genres") or ["party"])[0]
        buckets.setdefault(first, []).append(g)

    families = len({g["family"] for g in games})
    lines = [
        f"**{len(games)} games across {families} families**, defined in a "
        "[JSON registry](synccrate/src-tauri/src/game_registry.json). "
        "Missing one? [Add it](#adding-a-game), no code needed.",
        "",
    ]
    labels = dict(GROUPS)
    for key in order + [k for k in buckets if k not in order]:
        group = sorted(buckets.get(key, []), key=lambda g: g["label"].lower())
        if not group:
            continue
        lines += [
            "<details>",
            f"<summary><strong>{labels.get(key, key)}</strong> — {len(group)} game{'s' if len(group) != 1 else ''}</summary>",
            "",
            "| Game | What syncs |",
            "|------|------------|",
        ]
        for g in group:
            types = ", ".join(c["label"] for c in g["content_types"])
            lines.append(f"| **{g['label']}** | {types} |")
        lines += ["", "</details>", ""]

    raw = README.read_text(encoding="utf-8")
    nl = "\r\n" if "\r\n" in raw else "\n"
    head, rest = raw.split(START, 1)
    _, tail = rest.split(END, 1)
    body = nl.join(lines).rstrip() + nl
    README.write_text(head + START + nl + body + END + tail, encoding="utf-8", newline="")
    print(f"README games section updated: {len(games)} games")


if __name__ == "__main__":
    main()
