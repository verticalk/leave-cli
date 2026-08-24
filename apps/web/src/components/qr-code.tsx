import { useMemo } from "react";
import { encodeQr, qrToSvgPath, qrViewBoxSize } from "../lib/qr";

/**
 * Render a scannable code for a private address.
 *
 * Pairing a phone should not mean retyping a MagicDNS name, so the setup and
 * settings screens show the away URL as a code the phone camera can read.
 */
export function QrCode({ value, title }: { value: string; title: string }) {
  const symbol = useMemo(() => {
    try {
      const matrix = encodeQr(value);
      return { path: qrToSvgPath(matrix), extent: qrViewBoxSize(matrix) };
    } catch {
      return undefined;
    }
  }, [value]);

  if (!symbol) {
    return (
      <p className="qr-unavailable">
        This address is too long to show as a code. Copy it to your phone instead.
      </p>
    );
  }

  return (
    <svg
      className="qr-code"
      viewBox={`0 0 ${symbol.extent} ${symbol.extent}`}
      role="img"
      aria-label={title}
      shapeRendering="crispEdges"
    >
      <rect width={symbol.extent} height={symbol.extent} fill="#ffffff" />
      <path d={symbol.path} fill="#000000" />
    </svg>
  );
}
