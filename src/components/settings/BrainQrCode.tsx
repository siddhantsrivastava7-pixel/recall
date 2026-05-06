/**
 * v0.5.36 — Brain-themed scannable QR code for the Mobile Pairing tab.
 *
 * Replaces the v0.5.35 plain-square QRCodeSVG with a synapse-feeling
 * variant: round dots in a blue→purple gradient, extra-rounded
 * corner squares, transparent background. Encodes the same JSON
 * payload as before — phone scanners read it identically; this is
 * purely a style upgrade.
 *
 * Visual decisions:
 *   * Dots (type: "dots") — round modules read as synaptic nodes
 *     instead of pixelated squares. QR scanners tolerate non-square
 *     modules fine at error-correction level M.
 *   * Linear gradient #6699ff → #b388ff at 45° — the "Recall blue"
 *     bleeding into a brain-purple. Single direction; no harsh
 *     transitions that would confuse the camera contrast detector.
 *   * Corners "extra-rounded" — softens the otherwise-stark finder
 *     patterns.
 *   * Transparent background — sits inside Settings' panel-glass
 *     surface without a hard white edge.
 *
 * Implementation note: qr-code-styling is imperative (it appends
 * an SVG element to a target div). React's useEffect mounts the
 * QR on the ref, and re-renders on `value` change; the cleanup
 * clears the container so we don't accumulate orphan SVGs when the
 * payload regenerates after a "Reset pairing" click.
 */

import { useEffect, useRef } from "react";
import QRCodeStyling from "qr-code-styling";

interface BrainQrCodeProps {
  value: string;
  /// Square pixel size. The encoded modules scale to fit; M-level
  /// error correction with ~30 modules per side renders cleanly
  /// at >=180px. Below that the camera struggles.
  size?: number;
}

export function BrainQrCode({ value, size = 200 }: BrainQrCodeProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!containerRef.current) return;
    const qr = new QRCodeStyling({
      width: size,
      height: size,
      type: "svg",
      data: value,
      margin: 4,
      qrOptions: {
        errorCorrectionLevel: "M",
      },
      dotsOptions: {
        type: "dots",
        gradient: {
          type: "linear",
          rotation: Math.PI / 4,
          colorStops: [
            { offset: 0, color: "#6699ff" },
            { offset: 1, color: "#b388ff" },
          ],
        },
      },
      cornersSquareOptions: {
        type: "extra-rounded",
        color: "#7a8cff",
      },
      cornersDotOptions: {
        type: "dot",
        color: "#9b7bff",
      },
      backgroundOptions: {
        color: "transparent",
      },
    });

    // Clear any prior SVG before mounting a fresh one. Without
    // this, payload regen on "Reset pairing" stacks the old + new
    // QR images inside the container.
    containerRef.current.innerHTML = "";
    qr.append(containerRef.current);

    return () => {
      // Same cleanup on unmount so we don't leave a stale SVG
      // in the DOM when the user navigates away from Settings.
      if (containerRef.current) {
        containerRef.current.innerHTML = "";
      }
    };
  }, [value, size]);

  return (
    <div
      ref={containerRef}
      style={{
        width: size,
        height: size,
        // The styled QR draws on a transparent background; this
        // wrapper centers it consistently regardless of margin
        // configuration on the QR itself.
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    />
  );
}
