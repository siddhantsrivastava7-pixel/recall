interface RecallMarkProps {
  size?: number;
  className?: string;
}

/**
 * Recall brand mark — the dotted-graph "R" used as the desktop app icon.
 * Source: [public/recall-mark.png](public/recall-mark.png) (the same PNG that
 * ships in the desktop bundle). Kept as an `<img>` rather than inline SVG
 * because the icon's gradient/glow renders much better as a baked PNG than
 * as scaled vector primitives.
 */
export const RecallMark = ({ size = 18, className }: RecallMarkProps) => (
  <img
    src="/recall-mark.png"
    alt="Recall"
    width={size}
    height={size}
    className={className}
    style={{
      display: "block",
      flexShrink: 0,
      borderRadius: Math.round(size * 0.22),
    }}
    draggable={false}
  />
);
