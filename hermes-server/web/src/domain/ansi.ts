// Many programs colour their own log with ANSI escape codes (`ESC[32m`). Shown as they are, those are noise (`[2m`, `[0m`); read, they are the colours
// the program wanted. Only the colours and the weight are kept; any other escape is dropped.
import type { Piece } from './logSyntax';

const ESC = '\u001b';
// a control sequence: ESC [ parameters, intermediate bytes, a final byte
const CSI = new RegExp(`${ESC}\\[([0-9;:?]*)([ -/]*)([@-~])`, 'g');

export const hasAnsi = (line: string): boolean => line.includes(`${ESC}[`);

/** The line as plain text: no escapes. */
export function stripAnsi(line: string): string {
  return hasAnsi(line) ? line.replace(CSI, '') : line;
}

const COLOURS = ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white'];

interface Style {
  fg: string;
  bold: boolean;
  dim: boolean;
  italic: boolean;
  underline: boolean;
}

const PLAIN: Style = { fg: '', bold: false, dim: false, italic: false, underline: false };

function kindOf(s: Style): string {
  const kinds = [s.fg && `ansi-${s.fg}`, s.bold && 'ansi-bold', s.dim && 'ansi-dim', s.italic && 'ansi-italic', s.underline && 'ansi-underline'].filter(Boolean);
  return kinds.join(' ');
}

/** Applies the parameters of one `m` sequence to a style. */
function apply(style: Style, params: string): Style {
  const codes = params === '' ? [0] : params.split(/[;:]/).map((p) => Number(p) || 0);
  let s = { ...style };
  for (let i = 0; i < codes.length; i++) {
    const c = codes[i]!;
    if (c === 0) s = { ...PLAIN };
    else if (c === 1) s.bold = true;
    else if (c === 2) s.dim = true;
    else if (c === 3) s.italic = true;
    else if (c === 4) s.underline = true;
    else if (c === 22) { s.bold = false; s.dim = false; }
    else if (c === 23) s.italic = false;
    else if (c === 24) s.underline = false;
    else if (c >= 30 && c <= 37) s.fg = COLOURS[c - 30]!;
    else if (c >= 90 && c <= 97) s.fg = `bright-${COLOURS[c - 90]!}`;
    else if (c === 39) s.fg = '';
    else if (c === 38 || c === 48) i += codes[i + 1] === 5 ? 2 : codes[i + 1] === 2 ? 4 : 0; // 256-colour and true colour: skipped, with their parameters
  }
  return s;
}

/** The line as pieces: each stretch of text with the colours that were on when it was written. */
export function ansiPieces(line: string): Piece[] {
  const pieces: Piece[] = [];
  let style = PLAIN;
  let at = 0;
  CSI.lastIndex = 0;
  for (let m = CSI.exec(line); m; m = CSI.exec(line)) {
    if (m.index > at) pieces.push({ text: line.slice(at, m.index), kind: kindOf(style) });
    if (m[3] === 'm') style = apply(style, m[1] ?? '');
    at = m.index + m[0].length;
  }
  if (at < line.length) pieces.push({ text: line.slice(at), kind: kindOf(style) });
  return pieces;
}
