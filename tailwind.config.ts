import type { Config } from "tailwindcss";

/**
 * Tailwind is the spacing/flex/grid helper. The visual system lives in
 * [src/styles/globals.css](src/styles/globals.css) via CSS classes
 * (`.window`, `.sidebar`, `.mem-item`, `.qs-card`, `.btn`, …). This config
 * just exposes the design tokens to Tailwind utilities for one-off layout
 * tweaks. Legacy `ink-*` palette aliased to new tokens until production
 * components migrate off it.
 */
const config: Config = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        bg: {
          0: "var(--bg-0)",
          1: "var(--bg-1)",
          2: "var(--bg-2)",
          app: "var(--bg-app)",
        },
        t: {
          1: "var(--t-1)",
          2: "var(--t-2)",
          3: "var(--t-3)",
          4: "var(--t-4)",
        },
        tint: {
          1: "var(--tint-1)",
          2: "var(--tint-2)",
          3: "var(--tint-3)",
          4: "var(--tint-4)",
        },
        line: {
          DEFAULT: "var(--line)",
          strong: "var(--line-strong)",
        },
        accent: {
          DEFAULT: "var(--accent)",
          soft: "var(--accent-soft)",
          glow: "var(--accent-glow)",
          text: "var(--accent-text)",
        },
        success: "var(--success)",
        warning: "var(--warning)",
        danger: "var(--danger)",
        // Legacy palette — aliased to new tokens during migration.
        ink: {
          950: "var(--bg-app)",
          900: "var(--bg-0)",
          850: "var(--bg-1)",
          800: "var(--bg-2)",
          750: "var(--tint-2)",
          700: "var(--tint-3)",
        },
      },
      fontFamily: {
        sans: ["var(--font)"],
        display: ["var(--font-display)"],
        mono: ["var(--font-mono)"],
      },
      transitionTimingFunction: {
        ease: "cubic-bezier(0.32, 0.72, 0, 1)",
        "ease-out": "cubic-bezier(0.16, 1, 0.3, 1)",
      },
      boxShadow: {
        glow: "0 24px 80px rgba(4, 7, 16, 0.45)",
        panel: "0 16px 48px rgba(5, 10, 18, 0.34)",
      },
    },
  },
  plugins: [],
};

export default config;
