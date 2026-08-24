import { describe, expect, it } from "vitest";
import { QrCapacityError, encodeQr, formatBits, qrToSvgPath, qrViewBoxSize, versionBits } from "./qr";

/** A tailnet address encoded at version 3, verified against a reference encoder. */
const AWAY_URL_SYMBOL = [
  "#######..#...###...##.#######",
  "#.....#..##..####.....#.....#",
  "#.###.#.####.#.#..###.#.###.#",
  "#.###.#.#..##.##.#.#..#.###.#",
  "#.###.#.###.#..##..#..#.###.#",
  "#.....#.#...#....##...#.....#",
  "#######.#.#.#.#.#.#.#.#######",
  "........#.....#.#...#........",
  "#.#####..####...###.#.#####..",
  "#.##.#..#.##.###.###.##.#...#",
  "#.#.#.###.########..#.##.....",
  "..####.#######.#..##.##..#.#.",
  "#..#..###..#..##.#..#....##..",
  "#.##...#.#.##..#####..###...#",
  ".##..####.#.#....#.....#.##..",
  "..#.#..##.#.#.#.#.##.#..#..#.",
  "#...#.##.#.##...##..#..#.##..",
  "##.#.#.###...###..##.####.#.#",
  "#..##.#..##..####...##.##.#..",
  "#..#.#..#....#.#.......##..#.",
  "#...###.....#.##.#..#####.###",
  "........##.....##..##...#####",
  "#######...###......##.#.###..",
  "#.....#.#.##..#.#.#.#...#..#.",
  "#.###.#.#.###...#.#.#####.#.#",
  "#.###.#.#.##..##.##......##..",
  "#.###.#.###.#..###.#########.",
  "#.....#..##..#.#.##.###.##.#.",
  "#######.#.#.#..#...#...##.#.."
];

function render(text: string): string[] {
  const matrix = encodeQr(text);
  return matrix.modules.map((row) => row.map((dark) => (dark ? "#" : ".")).join(""));
}

describe("encodeQr", () => {
  it("encodes a tailnet address exactly", () => {
    const matrix = encodeQr("https://desk.tail1234.ts.net");
    expect(matrix.version).toBe(3);
    expect(matrix.size).toBe(29);
    expect(render("https://desk.tail1234.ts.net")).toEqual(AWAY_URL_SYMBOL);
  });

  it("grows the symbol version with the address length", () => {
    expect(encodeQr("https://a").version).toBe(1);
    expect(encodeQr(`https://${"a".repeat(60)}.ts.net`).version).toBe(5);
    expect(encodeQr(`https://${"a".repeat(190)}.ts.net`).version).toBe(10);
  });

  it("draws the three finder patterns", () => {
    const { modules, size } = encodeQr("https://desk.tail1234.ts.net");
    for (const [top, left] of [
      [0, 0],
      [0, size - 7],
      [size - 7, 0]
    ]) {
      expect(modules[top][left]).toBe(true);
      expect(modules[top + 1][left + 1]).toBe(false);
      expect(modules[top + 3][left + 3]).toBe(true);
    }
  });

  it("keeps the timing patterns alternating", () => {
    const { modules, size } = encodeQr("https://desk.tail1234.ts.net");
    for (let index = 8; index < size - 8; index += 1) {
      expect(modules[6][index]).toBe(index % 2 === 0);
      expect(modules[index][6]).toBe(index % 2 === 0);
    }
  });

  it("encodes multi-byte characters as UTF-8", () => {
    expect(() => encodeQr("https://café.ts.net")).not.toThrow();
  });

  it("refuses text beyond the supported versions", () => {
    expect(() => encodeQr("x".repeat(214))).toThrow(QrCapacityError);
  });
});

describe("format and version information", () => {
  it("matches the level M format strings", () => {
    const expected = [0x5412, 0x5125, 0x5e7c, 0x5b4b, 0x45f9, 0x40ce, 0x4f97, 0x4aa0];
    expect(Array.from({ length: 8 }, (_, mask) => formatBits(mask))).toEqual(expected);
  });

  it("matches the published version strings", () => {
    expect(versionBits(7)).toBe(0x07c94);
    expect(versionBits(8)).toBe(0x085bc);
    expect(versionBits(9)).toBe(0x09a99);
    expect(versionBits(10)).toBe(0x0a4d3);
  });
});

describe("svg rendering", () => {
  it("emits one path segment per dark module inside the quiet zone", () => {
    const matrix = encodeQr("https://desk.tail1234.ts.net");
    const dark = matrix.modules.flat().filter(Boolean).length;
    const path = qrToSvgPath(matrix);
    expect(path.split("M").length - 1).toBe(dark);
    expect(qrViewBoxSize(matrix)).toBe(matrix.size + 8);
    expect(path.startsWith("M4 4")).toBe(true);
  });
});
