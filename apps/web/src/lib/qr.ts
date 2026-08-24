/**
 * Minimal QR encoder for pairing a phone with a private tailnet address.
 *
 * Leave renders the away URL as a scannable code so nobody has to retype a
 * MagicDNS name on a phone keyboard. The encoder stays in-repo and
 * dependency-free: pairing must keep working in the offline installable PWA,
 * and the setup page is served from loopback with a strict origin policy.
 *
 * Byte mode, error correction level M, versions 1 through 10.
 */

const MODE_BYTE = 0b0100;
const PAD_BYTES = [0xec, 0x11];

interface VersionSpec {
  /** Error correction codewords per block. */
  ecPerBlock: number;
  /** [blockCount, dataCodewordsPerBlock] for each block group. */
  groups: Array<[number, number]>;
  /** Row and column centres of the alignment patterns. */
  alignment: number[];
}

/** Level M block structure for the versions Leave can produce. */
const VERSIONS: Record<number, VersionSpec> = {
  1: { ecPerBlock: 10, groups: [[1, 16]], alignment: [] },
  2: { ecPerBlock: 16, groups: [[1, 28]], alignment: [6, 18] },
  3: { ecPerBlock: 26, groups: [[1, 44]], alignment: [6, 22] },
  4: { ecPerBlock: 18, groups: [[2, 32]], alignment: [6, 26] },
  5: { ecPerBlock: 24, groups: [[2, 43]], alignment: [6, 30] },
  6: { ecPerBlock: 16, groups: [[4, 27]], alignment: [6, 34] },
  7: { ecPerBlock: 18, groups: [[4, 31]], alignment: [6, 22, 38] },
  8: { ecPerBlock: 22, groups: [[2, 38], [2, 39]], alignment: [6, 24, 42] },
  9: { ecPerBlock: 22, groups: [[3, 36], [2, 37]], alignment: [6, 26, 46] },
  10: { ecPerBlock: 26, groups: [[4, 43], [1, 44]], alignment: [6, 28, 50] }
};

const MIN_VERSION = 1;
const MAX_VERSION = 10;

/** A square grid of light and dark modules, without a quiet zone. */
export interface QrMatrix {
  /** Module count along one side. */
  size: number;
  /** Row-major dark-module flags. */
  modules: boolean[][];
  /** Symbol version, useful for tests and diagnostics. */
  version: number;
}

/** Raised when the text cannot fit the supported symbol versions. */
export class QrCapacityError extends Error {
  constructor(length: number) {
    super(`${length} bytes exceed the supported QR capacity`);
    this.name = "QrCapacityError";
  }
}

/**
 * Encode UTF-8 text as a level M QR symbol.
 *
 * @throws {QrCapacityError} when the text needs a version above 10.
 */
export function encodeQr(text: string): QrMatrix {
  const data = new TextEncoder().encode(text);
  const version = chooseVersion(data.length);
  const codewords = interleave(version, buildCodewords(version, data));
  const size = 17 + version * 4;
  const reserved = reservedMatrix(version, size);
  const base = functionPatterns(version, size);
  let best: { matrix: boolean[][]; penalty: number } | undefined;
  for (let mask = 0; mask < 8; mask += 1) {
    const matrix = base.map((row) => [...row]);
    placeData(matrix, reserved, codewords, mask);
    placeFormat(matrix, mask);
    const penalty = score(matrix);
    if (!best || penalty < best.penalty) best = { matrix, penalty };
  }
  if (!best) throw new Error("QR mask selection produced no candidate");
  return { size, modules: best.matrix, version };
}

/**
 * Render a matrix as an SVG path covering every dark module.
 *
 * @param quietZone Modules of margin required by the specification.
 */
export function qrToSvgPath(matrix: QrMatrix, quietZone = 4): string {
  const parts: string[] = [];
  for (let row = 0; row < matrix.size; row += 1) {
    for (let column = 0; column < matrix.size; column += 1) {
      if (matrix.modules[row][column]) {
        parts.push(`M${column + quietZone} ${row + quietZone}h1v1h-1z`);
      }
    }
  }
  return parts.join("");
}

/** Side length in modules once the quiet zone is included. */
export function qrViewBoxSize(matrix: QrMatrix, quietZone = 4): number {
  return matrix.size + quietZone * 2;
}

function chooseVersion(byteLength: number): number {
  for (let version = MIN_VERSION; version <= MAX_VERSION; version += 1) {
    const available = dataCodewords(version) * 8;
    const needed = 4 + countBits(version) + byteLength * 8;
    if (needed <= available) return version;
  }
  throw new QrCapacityError(byteLength);
}

function countBits(version: number): number {
  return version < 10 ? 8 : 16;
}

function dataCodewords(version: number): number {
  return VERSIONS[version].groups.reduce((total, [blocks, perBlock]) => total + blocks * perBlock, 0);
}

function buildCodewords(version: number, data: Uint8Array): number[][] {
  const bits: number[] = [];
  pushBits(bits, MODE_BYTE, 4);
  pushBits(bits, data.length, countBits(version));
  for (const byte of data) pushBits(bits, byte, 8);
  const capacity = dataCodewords(version) * 8;
  for (let index = 0; index < 4 && bits.length < capacity; index += 1) bits.push(0);
  while (bits.length % 8 !== 0) bits.push(0);
  const codewords: number[] = [];
  for (let index = 0; index < bits.length; index += 8) {
    codewords.push(bits.slice(index, index + 8).reduce((value, bit) => (value << 1) | bit, 0));
  }
  for (let index = 0; codewords.length < dataCodewords(version); index += 1) {
    codewords.push(PAD_BYTES[index % PAD_BYTES.length]);
  }

  const blocks: number[][] = [];
  let offset = 0;
  for (const [blockCount, perBlock] of VERSIONS[version].groups) {
    for (let block = 0; block < blockCount; block += 1) {
      blocks.push(codewords.slice(offset, offset + perBlock));
      offset += perBlock;
    }
  }
  return blocks;
}

function interleave(version: number, blocks: number[][]): number[] {
  const ecPerBlock = VERSIONS[version].ecPerBlock;
  const ecBlocks = blocks.map((block) => reedSolomon(block, ecPerBlock));
  const output: number[] = [];
  const longestData = Math.max(...blocks.map((block) => block.length));
  for (let index = 0; index < longestData; index += 1) {
    for (const block of blocks) {
      if (index < block.length) output.push(block[index]);
    }
  }
  for (let index = 0; index < ecPerBlock; index += 1) {
    for (const block of ecBlocks) output.push(block[index]);
  }
  return output;
}

const EXP = new Uint8Array(512);
const LOG = new Uint8Array(256);
{
  let value = 1;
  for (let index = 0; index < 255; index += 1) {
    EXP[index] = value;
    LOG[value] = index;
    value <<= 1;
    if (value & 0x100) value ^= 0x11d;
  }
  for (let index = 255; index < 512; index += 1) EXP[index] = EXP[index - 255];
}

function multiply(left: number, right: number): number {
  if (left === 0 || right === 0) return 0;
  return EXP[LOG[left] + LOG[right]];
}

function reedSolomon(data: number[], ecLength: number): number[] {
  let generator = [1];
  for (let index = 0; index < ecLength; index += 1) {
    const next = new Array<number>(generator.length + 1).fill(0);
    for (let position = 0; position < generator.length; position += 1) {
      next[position] ^= generator[position];
      next[position + 1] ^= multiply(generator[position], EXP[index]);
    }
    generator = next;
  }
  const remainder = new Array<number>(ecLength).fill(0);
  for (const byte of data) {
    const factor = byte ^ remainder[0];
    remainder.shift();
    remainder.push(0);
    for (let index = 0; index < ecLength; index += 1) {
      remainder[index] ^= multiply(generator[index + 1], factor);
    }
  }
  return remainder;
}

function pushBits(bits: number[], value: number, length: number): void {
  for (let index = length - 1; index >= 0; index -= 1) bits.push((value >> index) & 1);
}

function blankMatrix(size: number): boolean[][] {
  return Array.from({ length: size }, () => new Array<boolean>(size).fill(false));
}

function functionPatterns(version: number, size: number): boolean[][] {
  const matrix = blankMatrix(size);
  for (const [row, column] of [[0, 0], [0, size - 7], [size - 7, 0]] as const) {
    drawFinder(matrix, row, column);
  }
  for (let index = 8; index < size - 8; index += 1) {
    const dark = index % 2 === 0;
    matrix[6][index] = dark;
    matrix[index][6] = dark;
  }
  for (const row of VERSIONS[version].alignment) {
    for (const column of VERSIONS[version].alignment) {
      if (!skipAlignment(row, column, size)) drawAlignment(matrix, row, column);
    }
  }
  matrix[size - 8][8] = true;
  if (version >= 7) drawVersionInfo(matrix, version, size);
  return matrix;
}

function drawFinder(matrix: boolean[][], top: number, left: number): void {
  for (let row = 0; row < 7; row += 1) {
    for (let column = 0; column < 7; column += 1) {
      const edge = row === 0 || row === 6 || column === 0 || column === 6;
      const core = row >= 2 && row <= 4 && column >= 2 && column <= 4;
      matrix[top + row][left + column] = edge || core;
    }
  }
}

function drawAlignment(matrix: boolean[][], centreRow: number, centreColumn: number): void {
  for (let row = -2; row <= 2; row += 1) {
    for (let column = -2; column <= 2; column += 1) {
      const ring = Math.max(Math.abs(row), Math.abs(column));
      matrix[centreRow + row][centreColumn + column] = ring !== 1;
    }
  }
}

function reservedMatrix(version: number, size: number): boolean[][] {
  const reserved = blankMatrix(size);
  const reserveBlock = (top: number, left: number, rows: number, columns: number) => {
    for (let row = top; row < top + rows; row += 1) {
      for (let column = left; column < left + columns; column += 1) {
        if (row >= 0 && row < size && column >= 0 && column < size) reserved[row][column] = true;
      }
    }
  };
  reserveBlock(0, 0, 9, 9);
  reserveBlock(0, size - 8, 9, 8);
  reserveBlock(size - 8, 0, 8, 9);
  for (let index = 0; index < size; index += 1) {
    reserved[6][index] = true;
    reserved[index][6] = true;
  }
  for (const row of VERSIONS[version].alignment) {
    for (const column of VERSIONS[version].alignment) {
      if (skipAlignment(row, column, size)) continue;
      reserveBlock(row - 2, column - 2, 5, 5);
    }
  }
  if (version >= 7) {
    reserveBlock(0, size - 11, 6, 3);
    reserveBlock(size - 11, 0, 3, 6);
  }
  return reserved;
}

/** Alignment centres overlapping a finder pattern are not drawn. */
function skipAlignment(row: number, column: number, size: number): boolean {
  return (
    (row <= 8 && column <= 8) ||
    (row <= 8 && column >= size - 9) ||
    (row >= size - 9 && column <= 8)
  );
}

function placeData(
  matrix: boolean[][],
  reserved: boolean[][],
  codewords: number[],
  mask: number
): void {
  const size = matrix.length;
  let bitIndex = 0;
  let upward = true;
  for (let right = size - 1; right >= 1; right -= 2) {
    // Column 6 carries the vertical timing pattern, so the pair shifts left.
    if (right === 6) right = 5;
    const column = right;
    for (let step = 0; step < size; step += 1) {
      const row = upward ? size - 1 - step : step;
      for (const offset of [0, 1]) {
        const target = column - offset;
        if (reserved[row][target]) continue;
        const byte = codewords[bitIndex >> 3];
        const bit = byte === undefined ? 0 : (byte >> (7 - (bitIndex & 7))) & 1;
        bitIndex += 1;
        matrix[row][target] = (bit === 1) !== maskAt(mask, row, target);
      }
    }
    upward = !upward;
  }
}

function maskAt(mask: number, row: number, column: number): boolean {
  switch (mask) {
    case 0:
      return (row + column) % 2 === 0;
    case 1:
      return row % 2 === 0;
    case 2:
      return column % 3 === 0;
    case 3:
      return (row + column) % 3 === 0;
    case 4:
      return (Math.floor(row / 2) + Math.floor(column / 3)) % 2 === 0;
    case 5:
      return ((row * column) % 2) + ((row * column) % 3) === 0;
    case 6:
      return (((row * column) % 2) + ((row * column) % 3)) % 2 === 0;
    default:
      return (((row + column) % 2) + ((row * column) % 3)) % 2 === 0;
  }
}

/** Format information bits for error correction level M, including BCH and masking. */
export function formatBits(mask: number): number {
  const data = (0b00 << 3) | mask;
  let remainder = data;
  for (let index = 0; index < 10; index += 1) {
    remainder <<= 1;
    if (remainder & 0b100_0000_0000) remainder ^= 0b101_0011_0111;
  }
  return ((data << 10) | remainder) ^ 0b101_0100_0001_0010;
}

function placeFormat(matrix: boolean[][], mask: number): void {
  const size = matrix.length;
  const bits = formatBits(mask);
  const dark = (index: number) => ((bits >> index) & 1) === 1;
  for (let index = 0; index <= 5; index += 1) {
    matrix[index][8] = dark(index);
  }
  matrix[7][8] = dark(6);
  matrix[8][8] = dark(7);
  matrix[8][7] = dark(8);
  for (let index = 9; index < 15; index += 1) {
    matrix[8][14 - index] = dark(index);
  }
  for (let index = 0; index < 8; index += 1) {
    matrix[8][size - 1 - index] = dark(index);
  }
  for (let index = 8; index < 15; index += 1) {
    matrix[size - 15 + index][8] = dark(index);
  }
  matrix[size - 8][8] = true;
}

/** Version information bits for versions 7 and above. */
export function versionBits(version: number): number {
  let remainder = version;
  for (let index = 0; index < 12; index += 1) {
    remainder <<= 1;
    if (remainder & 0b1_0000_0000_0000) remainder ^= 0b1_1111_0010_0101;
  }
  return (version << 12) | remainder;
}

function drawVersionInfo(matrix: boolean[][], version: number, size: number): void {
  const bits = versionBits(version);
  for (let index = 0; index < 18; index += 1) {
    const dark = ((bits >> index) & 1) === 1;
    const row = Math.floor(index / 3);
    const column = index % 3;
    matrix[row][size - 11 + column] = dark;
    matrix[size - 11 + column][row] = dark;
  }
}

function score(matrix: boolean[][]): number {
  const size = matrix.length;
  let penalty = 0;

  for (let row = 0; row < size; row += 1) {
    let horizontal = 1;
    let vertical = 1;
    for (let index = 1; index < size; index += 1) {
      horizontal = matrix[row][index] === matrix[row][index - 1] ? horizontal + 1 : 1;
      if (horizontal === 5) penalty += 3;
      else if (horizontal > 5) penalty += 1;
      vertical = matrix[index][row] === matrix[index - 1][row] ? vertical + 1 : 1;
      if (vertical === 5) penalty += 3;
      else if (vertical > 5) penalty += 1;
    }
  }

  for (let row = 0; row < size - 1; row += 1) {
    for (let column = 0; column < size - 1; column += 1) {
      const value = matrix[row][column];
      if (
        value === matrix[row][column + 1] &&
        value === matrix[row + 1][column] &&
        value === matrix[row + 1][column + 1]
      ) {
        penalty += 3;
      }
    }
  }

  const pattern = [true, false, true, true, true, false, true];
  const light = [false, false, false, false];
  const matches = (values: boolean[], start: number, expected: boolean[]) =>
    expected.every((bit, offset) => values[start + offset] === bit);
  for (let index = 0; index < size; index += 1) {
    const rowValues = matrix[index];
    const columnValues = matrix.map((row) => row[index]);
    for (const values of [rowValues, columnValues]) {
      for (let start = 0; start + 7 <= size; start += 1) {
        if (!matches(values, start, pattern)) continue;
        const before = start >= 4 && matches(values, start - 4, light);
        const after = start + 11 <= size && matches(values, start + 7, light);
        if (before || after) penalty += 40;
      }
    }
  }

  const dark = matrix.flat().filter(Boolean).length;
  const ratio = (dark * 100) / (size * size);
  penalty += Math.floor(Math.abs(ratio - 50) / 5) * 10;
  return penalty;
}
