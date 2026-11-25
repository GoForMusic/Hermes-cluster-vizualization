import { describe, expect, it } from 'vitest';
import { MAX_COLOURED, highlight } from '../domain/logSyntax';

/** `kind:text` for each piece that is something, and the text alone for what is not: easy to read in a failure. */
const show = (line: string): string[] => highlight(line).map((p) => (p.kind ? `${p.kind.replace('tk-', '')}:${p.text}` : p.text));

describe('colouring a line of a log', () => {
  it('always gives back the line, whatever it is made of', () => {
    for (const line of ['', 'plain text', '2026-11-25T10:09:40.460934681Z INFO "x" 200 /a/b?c=d 10.0.0.1:80 12ms', '{"a":{"b":[1,2,"3"]}}', 'ü ß 日本語 = = ==', '"unclosed', '[', '/', '1#1:']) {
      expect(highlight(line).map((p) => p.text).join('')).toBe(line);
    }
  });

  it('knows the times of the usual formats', () => {
    expect(show('2026-11-25T10:09:40.460934681Z hello')[0]).toBe('time:2026-11-25T10:09:40.460934681Z');
    expect(show('2026-11-25 10:09:40,123 hello')[0]).toBe('time:2026-11-25 10:09:40,123');
    expect(show('2026/11/25 10:01:02 [notice] up')[0]).toBe('time:2026/11/25 10:01:02');
    expect(show('Nov 25 10:01:02 host sshd: x')[0]).toBe('time:Nov 25 10:01:02');
    expect(show('10:01:02.5 start')[0]).toBe('time:10:01:02.5');
    expect(show('1.2.3.4 - - [25/Nov/2026:10:09:40 +0000] "GET / HTTP/1.1"')).toContain('time:[25/Nov/2026:10:09:40 +0000]');
  });

  it('knows a level in capitals, in brackets and in level=info, and tells how bad it is', () => {
    expect(show('x ERROR y')).toEqual(['x ', 'lvl-error:ERROR', ' y']);
    expect(show('WARNING careful')[0]).toBe('lvl-warn:WARNING');
    expect(show('FATAL')[0]).toBe('lvl-error:FATAL');
    expect(show('DEBUG a')[0]).toBe('lvl-debug:DEBUG');
    expect(show('2026/11/25 10:01:02 [notice] up')).toContain('lvl-info:[notice]');
    expect(show('2026/11/25 10:01:02 [emerg] down')).toContain('lvl-error:[emerg]');
    expect(show('level=warn msg="x"')).toContain('lvl-warn:warn');
    expect(show('{"level":"error","msg":"x"}')).toContain('lvl-error:error');
    // the word in a sentence is a word, not a level
    expect(show('this is info about errors')).toEqual(['this is info about errors']);
  });

  it('colours an access log line: the address, the method, the path, the status, the size', () => {
    const line = '10.0.2.17 - - [25/Nov/2026:10:09:49 +0000] "GET /big HTTP/1.1" 200 5242880 "-" "Wget" "-"';
    const got = show(line);
    expect(got[0]).toBe('ip:10.0.2.17');
    expect(got).toContain('method:GET');
    expect(got).toContain('path:/big');
    expect(got).toContain('s2:200');
    expect(got).toContain('num:5242880');
    expect(show('x" 404 12')).toContain('s4:404');
    expect(show('x" 502 12')).toContain('s5:502');
    expect(show('x" 301 12')).toContain('s3:301');
    expect(show('status=503 ok')).toContain('s5:503');
  });

  it('colours key=value and the JSON of structured logs', () => {
    expect(show('level=info msg="listening" port=8080 took=12ms ok=true')).toEqual([
      'key:level', '=', 'lvl-info:info', ' ', 'key:msg', '=', 'str:"listening"', ' ', 'key:port', '=', 'num:8080', ' ', 'key:took', '=', 'num:12ms', ' ', 'key:ok', '=', 'bool:true',
    ]);
    const json = show('{"ts":"x","n":42,"on":false,"user":null}');
    expect(json).toContain('key:"ts"');
    expect(json).toContain('str:"x"');
    expect(json).toContain('key:"n"');
    expect(json).toContain('num:42');
    expect(json).toContain('bool:false');
    expect(json).toContain('bool:null');
  });

  it('colours addresses, URLs, durations, sizes and tags', () => {
    expect(show('connect 10.0.0.5:5432 failed')).toContain('ip:10.0.0.5:5432');
    expect(show('GET https://example.com/a?b=c now')).toContain('url:https://example.com/a?b=c');
    expect(show('took 1.5s')).toContain('num:1.5s');
    expect(show('used 256MiB')).toContain('num:256MiB');
    expect(show('2026/11/25 10:01:02 [notice] 1#1: signal 3 received')).toEqual(expect.arrayContaining(['tag:1#1:']));
    expect(show('[main] started')[0]).toBe('tag:[main]');
  });

  it('leaves a very long line alone', () => {
    const long = `ERROR ${'x'.repeat(MAX_COLOURED)}`;
    expect(highlight(long)).toEqual([{ text: long, kind: '' }]);
  });

  it('is fast enough for a screenful of lines', () => {
    const line = '2026-11-25T10:09:40.460934681Z 10.0.2.30 - - [25/Nov/2026:10:09:40 +0000] "GET /big?x=1 HTTP/1.1" 200 5242880 "-" "Wget" "-" level=info took=12ms';
    const start = performance.now();
    for (let i = 0; i < 5000; i++) highlight(line);
    expect(performance.now() - start).toBeLessThan(2000);
  });
});
