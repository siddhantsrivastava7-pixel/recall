import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        ink: {
          950: "#070b13",
          900: "#0b111d",
          850: "#101827",
          800: "#131d2f",
          750: "#172337",
          700: "#20324a",
        },
        line: {
          DEFAULT: "rgba(255, 255, 255, 0.08)",
          soft: "rgba(255, 255, 255, 0.04)",
        },
        accent: {
          DEFAULT: "#8fb5ff",
          strong: "#bfd3ff",
          muted: "#50688d",
        },
        success: "#7fd2ad",
        warning: "#ffcf7e",
        danger: "#ff8a8a",
      },
      boxShadow: {
        glow: "0 24px 80px rgba(4, 7, 16, 0.45)",
        panel: "0 16px 48px rgba(5, 10, 18, 0.34)",
      },
      borderRadius: {
        "4xl": "2rem",
      },
      fontFamily: {
        sans: ["Aptos", "\"Segoe UI Variable Display\"", "\"Segoe UI\"", "sans-serif"],
      },
      animation: {
        rise: "rise 240ms ease-out",
        breathe: "breathe 2.8s ease-in-out infinite",
      },
      keyframes: {
        rise: {
          "0%": { opacity: "0", transform: "translateY(8px)" },
          "100%": { opacity: "1", transform: "translateY(0)" },
        },
        breathe: {
          "0%, 100%": { transform: "scale(1)" },
          "50%": { transform: "scale(1.02)" },
        },
      },
    },
  },
  plugins: [],
};

export default config;
