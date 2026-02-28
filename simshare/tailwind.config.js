/** @type {import('tailwindcss').Config} */
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
          DEFAULT: "rgb(var(--color-bg) / <alpha-value>)",
          card: "rgb(var(--color-bg-card) / <alpha-value>)",
          "card-hover": "rgb(var(--color-bg-card-hover) / <alpha-value>)",
          "card-active": "rgb(var(--color-bg-card-active) / <alpha-value>)",
        },
        accent: {
          DEFAULT: "rgb(var(--color-accent) / <alpha-value>)",
          light: "rgb(var(--color-accent-light) / <alpha-value>)",
        },
        border: {
          DEFAULT: "rgb(var(--color-border) / <alpha-value>)",
        },
        txt: {
          DEFAULT: "rgb(var(--color-txt) / <alpha-value>)",
          dim: "rgb(var(--color-txt-dim) / <alpha-value>)",
        },
        status: {
          green: "rgb(var(--color-status-green) / <alpha-value>)",
          yellow: "rgb(var(--color-status-yellow) / <alpha-value>)",
          red: "rgb(var(--color-status-red) / <alpha-value>)",
        },
      },
    },
  },
  plugins: [],
};
