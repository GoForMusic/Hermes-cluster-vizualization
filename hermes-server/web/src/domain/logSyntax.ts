// Colours for a line of a log: what is the time, the level, the method, the status, a key and its value. Pure: a line in, pieces out, so it can be
// tested without a page. It does not understand every log format; it knows the shapes that most of them are made of (a timestamp, a level in capitals
// or in `level=info`, `key=value`, a JSON object, an access log line, an address, a duration), and leaves the rest as it is.

export interface Piece {
  text: string;
  /** What it is, as a class name (`tk-time`, `tk-lvl-error`, ...); empty for text that is not anything in particular. */
  kind: string;
}

/** A line longer than this is shown as it is: colouring text of that size costs more than it tells. */
export const MAX_COLOURED = 2000;

// One regular expression, one group per kind. The leftmost match wins and, at the same place, the first group: so the order is the priority.
const KINDS: readonly [name: string, source: string][] = [
  // times
  ['time', String.raw`\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:[.,]\d+)?(?:Z|[+-]\d{2}:?\d{2})?`],
  ['time', String.raw`\[\d{2}/[A-Za-z]{3}/\d{4}:\d{2}:\d{2}:\d{2} [+-]\d{4}\]`],
  ['time', String.raw`\d{4}/\d{2}/\d{2} \d{2}:\d{2}:\d{2}`],
  ['time', String.raw`\b[A-Z][a-z]{2} {1,2}\d{1,2} \d{2}:\d{2}:\d{2}\b`],
  ['time', String.raw`\b\d{2}:\d{2}:\d{2}(?:[.,]\d+)?\b`],
  // a JSON key, with its colon
  ['key', String.raw`"[A-Za-z_@][\w.@-]*"(?=\s*:)`],
  // `level=info`, `lvl=warn`, `"level":"error"`: the value is the level
  ['levelvalue', String.raw`(?<=\b(?:level|lvl|severity)["']?\s*[=:]\s*["']?)(?:trace|debug|info|notice|warn|warning|error|err|fatal|critical|crit|panic)\b`],
  // a level in capitals, or in brackets
  ['level', String.raw`\b(?:TRACE|DEBUG|INFO|NOTICE|WARN|WARNING|ERROR|ERR|FATAL|CRITICAL|CRIT|PANIC)\b`],
  ['level', String.raw`\[(?:trace|debug|info|notice|warn|warning|error|err|fatal|critical|crit|emerg|alert)\]`],
  // an access log: the method, and the status that follows the quoted request
  ['method', String.raw`\b(?:GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS|CONNECT)\b`],
  ['status', String.raw`(?<=" )[1-5]\d{2}(?= )`],
  ['status', String.raw`(?<=\b(?:status|code|status_code)["']?\s*[=:]\s*["']?)[1-5]\d{2}\b`],
  // key=value: the key
  ['key', String.raw`(?<![\w.-])[A-Za-z_][\w.-]*(?==)`],
  // text in quotes, an address, a URL, a path
  ['str', String.raw`"(?:[^"\\]|\\.)*"`],
  ['url', String.raw`\bhttps?://[^\s"')\]]+`],
  ['ip', String.raw`\b\d{1,3}(?:\.\d{1,3}){3}(?::\d{1,5})?\b`],
  ['path', String.raw`(?<=[\s="'(])/[\w.~%@:+-]+(?:/[\w.~%@:+-]*)*(?:\?[^\s"')]*)?`],
  // the `1#1:` of nginx, before the numbers that it is made of
  ['tag', String.raw`\b\d+#\d+:`],
  // a number with its unit: a duration or a size
  ['num', String.raw`\b\d+(?:\.\d+)?(?:ms|µs|us|ns|s|m|h|d|[KMGT]i?B|B)\b`],
  ['num', String.raw`\b\d+(?:\.\d+)?\b`],
  ['bool', String.raw`\b(?:true|false|null|nil|None)\b`],
  // a tag in brackets (`[notice]`, `[main]`) and the thread-ish `1#1:` of nginx
  ['tag', String.raw`\[[^\]\s]{1,40}\]`],
];

/** The same group name can be wanted twice, with different shapes: each pattern gets its own group, and its kind is looked up by number. */
const PATTERN = new RegExp(KINDS.map(([, source]) => `(${source})`).join('|'), 'g');

const LEVELS: Record<string, string> = {
  trace: 'trace', debug: 'debug', info: 'info', notice: 'info', warn: 'warn', warning: 'warn', error: 'error', err: 'error', fatal: 'error', critical: 'error', crit: 'error', panic: 'error', emerg: 'error', alert: 'error',
};

function kindOf(name: string, text: string): string {
  if (name === 'level' || name === 'levelvalue') return `tk-lvl-${LEVELS[text.replace(/^\[|\]$/g, '').toLowerCase()] ?? 'info'}`;
  if (name === 'status') return `tk-s${text[0]}`;
  return `tk-${name}`;
}

/** The line as pieces, in order, that put together are the line. */
export function highlight(line: string): Piece[] {
  if (!line || line.length > MAX_COLOURED) return [{ text: line, kind: '' }];
  const pieces: Piece[] = [];
  let at = 0;
  let lastKey = ''; // the last key seen, to tell what its value is
  PATTERN.lastIndex = 0;
  for (let m = PATTERN.exec(line); m; m = PATTERN.exec(line)) {
    if (m[0] === '') { PATTERN.lastIndex += 1; continue; }
    const group = m.findIndex((g, i) => i > 0 && g !== undefined) - 1;
    if (m.index > at) pieces.push({ text: line.slice(at, m.index), kind: '' });
    const kind = KINDS[group]![0];
    if (kind === 'str') {
      // the value of a JSON key that says the level is a level, not just text
      const inner = m[0].slice(1, -1).toLowerCase();
      if (/^(level|lvl|severity)$/.test(lastKey) && inner in LEVELS) {
        pieces.push({ text: '"', kind: 'tk-str' }, { text: m[0].slice(1, -1), kind: `tk-lvl-${LEVELS[inner]}` }, { text: '"', kind: 'tk-str' });
      } else {
        pieces.push(...request(m[0]));
      }
    } else {
      pieces.push({ text: m[0], kind: kindOf(kind, m[0]) });
    }
    lastKey = kind === 'key' ? m[0].replace(/^"|"$/g, '').toLowerCase() : kind === 'str' ? '' : lastKey;
    at = m.index + m[0].length;
  }
  if (at < line.length) pieces.push({ text: line.slice(at), kind: '' });
  return pieces;
}

const REQUEST = /^"((?:GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS|CONNECT)) (\S+)(?: (HTTP\/\d(?:\.\d)?))?"$/;

/** `"GET /big HTTP/1.1"` of an access log is a request: the method, the path and the protocol each have their colour. Any other quoted text is a string. */
function request(quoted: string): Piece[] {
  const m = REQUEST.exec(quoted);
  if (!m) return [{ text: quoted, kind: 'tk-str' }];
  const [, method, path, protocol] = m as unknown as [string, string, string, string | undefined];
  return [
    { text: '"', kind: 'tk-str' },
    { text: method, kind: 'tk-method' },
    { text: ' ', kind: '' },
    { text: path, kind: 'tk-path' },
    ...(protocol ? [{ text: ' ', kind: '' }, { text: protocol, kind: 'tk-tag' }] : []),
    { text: '"', kind: 'tk-str' },
  ];
}
