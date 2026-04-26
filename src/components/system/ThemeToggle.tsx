import { Moon, Sun, SunMoon } from "lucide-react";
import { useThemeMode, type ThemeMode } from "@/hooks/useSystemTheme";

const OPTIONS: { mode: ThemeMode; label: string; Icon: typeof Sun }[] = [
  { mode: "light", label: "Light", Icon: Sun },
  { mode: "auto", label: "Auto", Icon: SunMoon },
  { mode: "dark", label: "Dark", Icon: Moon },
];

export const ThemeToggle = () => {
  const { mode, setMode } = useThemeMode();

  return (
    <div className="theme-toggle no-drag">
      {OPTIONS.map(({ mode: m, label, Icon }) => (
        <button
          key={m}
          type="button"
          aria-label={`${label} theme`}
          aria-pressed={mode === m}
          className={mode === m ? "active" : ""}
          onClick={() => setMode(m)}
        >
          <Icon size={11} strokeWidth={2} />
        </button>
      ))}
    </div>
  );
};
