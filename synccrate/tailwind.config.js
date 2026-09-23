/** @type {import('tailwindcss').Config} */
const c = (v) => `rgb(var(--color-${v}) / <alpha-value>)`;

export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  safelist: [
    "bg-accent-light",
    "bg-status-green",
    "bg-status-yellow",
    "bg-status-red",
  ],
  theme: {
    extend: {
      colors: {
        bg: {
          DEFAULT: c("bg"),
          2: c("bg-2"),
          card: c("bg-card"),
          "card-hover": c("bg-card-hover"),
          "card-active": c("bg-card-active"),
          elevated: c("bg-elevated"),
        },
        accent: {
          DEFAULT: c("accent"),
          light: c("accent-light"),
        },
        // Bright HUD accent: active states, live indicators, key numbers. Use sparingly.
        neon: {
          DEFAULT: c("neon"),
          ink: c("on-neon"),
        },
        amber: {
          DEFAULT: c("amber"),
        },
        border: {
          DEFAULT: c("border"),
          hi: c("line-hi"),
        },
        line: {
          DEFAULT: c("border"),
          hi: c("line-hi"),
        },
        txt: {
          DEFAULT: c("txt"),
          dim: c("txt-dim"),
          muted: c("txt-muted"),
        },
        status: {
          green: c("status-green"),
          yellow: c("status-yellow"),
          red: c("status-red"),
        },
      },
      fontFamily: {
        sans: ["Inter", "Segoe UI", "system-ui", "sans-serif"],
        display: ["'Chakra Petch'", "Segoe UI", "system-ui", "sans-serif"],
        mono: ["'JetBrains Mono'", "ui-monospace", "Consolas", "monospace"],
      },
      letterSpacing: {
        hud: "0.12em",
      },
    },
  },
  plugins: [],
};
