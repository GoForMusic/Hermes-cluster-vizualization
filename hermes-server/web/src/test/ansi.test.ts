import { describe, expect, it } from 'vitest';
import { ansiPieces, hasAnsi, stripAnsi } from '../domain/ansi';

const E = '\u001b';
const show = (line: string) => ansiPieces(line).map((p) => `${p.kind || '-'}:${p.text}`);

describe('the colours a program put in its own log', () => {
  it('knows a line that has them', () => {
    expect(hasAnsi(`${E}[32mok${E}[0m`)).toBe(true);
    expect(hasAnsi('plain [32m not an escape')).toBe(false);
  });

  it('takes them out of the text', () => {
    expect(stripAnsi(`${E}[2m2026-11-25${E}[0m ${E}[32m INFO${E}[0m up`)).toBe('2026-11-25  INFO up');
    expect(stripAnsi('plain')).toBe('plain');
    expect(stripAnsi(`a${E}[2Kb${E}[1;1Hc`)).toBe('abc');
  });

  it('keeps what each stretch of text was written with', () => {
    expect(show(`${E}[2mtime${E}[0m ${E}[32m INFO${E}[0m message`)).toEqual(['ansi-dim:time', '-: ', 'ansi-green: INFO', '-: message']);
    expect(show(`${E}[1;31mfail${E}[0m`)).toEqual(['ansi-red ansi-bold:fail']);
    expect(show(`${E}[91mbright${E}[39m plain`)).toEqual(['ansi-bright-red:bright', '-: plain']);
    expect(show(`${E}[3;4mx${E}[23mu${E}[24mp`)).toEqual(['ansi-italic ansi-underline:x', 'ansi-underline:u', '-:p']);
  });

  it('skips the 256-colour and true-colour codes with their parameters, and keeps going', () => {
    expect(show(`${E}[38;5;208mx${E}[0m ${E}[38;2;1;2;3;1my${E}[0m`)).toEqual(['-:x', '-: ', 'ansi-bold:y']);
  });

  it('gives back every character that was text', () => {
    for (const line of [`${E}[32m`, `a${E}[0m`, `${E}[0m${E}[0m`, 'x', '']) {
      expect(ansiPieces(line).map((p) => p.text).join('')).toBe(stripAnsi(line));
    }
  });
});
