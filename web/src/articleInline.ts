export type InlinePiece =
  | { kind: 'text'; text: string }
  | { kind: 'strong'; text: string }
  | { kind: 'em'; text: string }
  | { kind: 'code'; text: string }
  | { kind: 'link'; label: string; href: string }
  | { kind: 'cite'; marker: string };

/** Visible article syntax. Code spans stay literal. A digit marker such as [1] stays a citation. */
export function splitInline(text: string): InlinePiece[] {
  const pieces: InlinePiece[] = [];
  let index = 0;
  let plain = '';
  const flush = () => {
    if (plain) {
      pieces.push({ kind: 'text', text: plain });
      plain = '';
    }
  };
  while (index < text.length) {
    const token = takeToken(text, index);
    if (!token) {
      plain += text[index];
      index += 1;
      continue;
    }
    flush();
    pieces.push(token.piece);
    index = token.end;
  }
  flush();
  return pieces;
}

export function httpHref(url: string): string | null {
  try {
    const parsed = new URL(url);
    if (parsed.protocol === 'http:' || parsed.protocol === 'https:') return parsed.href;
  } catch {
    return null;
  }
  return null;
}

function takeToken(text: string, index: number): { piece: InlinePiece; end: number } | null {
  const code = codeSpan(text, index);
  if (code) return { piece: { kind: 'code', text: code.body }, end: code.end };
  const strong = paired(text, index, '**', false);
  if (strong) return { piece: { kind: 'strong', text: strong.inside }, end: strong.end };
  const underline = paired(text, index, '__', true);
  if (underline) return { piece: { kind: 'strong', text: underline.inside }, end: underline.end };
  const link = markdownLink(text, index);
  if (link) return { piece: { kind: 'link', label: link.label, href: link.href }, end: link.end };
  const em = paired(text, index, '*', false);
  if (em) return { piece: { kind: 'em', text: em.inside }, end: em.end };
  const emUnder = paired(text, index, '_', true);
  if (emUnder) return { piece: { kind: 'em', text: emUnder.inside }, end: emUnder.end };
  const cite = text.slice(index).match(/^\[[A-Za-z0-9_-]+\]/);
  if (cite) return { piece: { kind: 'cite', marker: cite[0] }, end: index + cite[0].length };
  return null;
}

function codeSpan(text: string, index: number): { body: string; end: number } | null {
  if (text[index] !== '`') return null;
  let marks = 0;
  while (text[index + marks] === '`') marks += 1;
  const fence = '`'.repeat(marks);
  const bodyStart = index + marks;
  const close = text.indexOf(fence, bodyStart);
  if (close < 0) return null;
  return { body: text.slice(bodyStart, close), end: close + marks };
}

function paired(
  text: string,
  index: number,
  marker: string,
  rejectIntraWord: boolean,
): { inside: string; end: number } | null {
  if (!text.startsWith(marker, index)) return null;
  const after = index + marker.length;
  if (after >= text.length || /\s/u.test(text[after])) return null;
  const close = text.indexOf(marker, after);
  if (close < 0) return null;
  const inside = text.slice(after, close);
  if (!inside || /\s/u.test(inside[inside.length - 1]) || inside.includes('\n')) return null;
  const end = close + marker.length;
  if (
    rejectIntraWord &&
    (isAlnum(text[index - 1] ?? '') || isAlnum(text[end] ?? ''))
  ) {
    return null;
  }
  return { inside, end };
}

function markdownLink(
  text: string,
  index: number,
): { label: string; href: string; end: number } | null {
  if (text[index] !== '[') return null;
  const close = text.indexOf(']', index + 1);
  if (close < 0 || text[close + 1] !== '(') return null;
  const label = text.slice(index + 1, close);
  if (!label || /^[0-9]+$/u.test(label)) return null;
  let depth = 0;
  for (let cursor = close + 1; cursor < text.length; cursor += 1) {
    const ch = text[cursor];
    if (ch === '(') depth += 1;
    else if (ch === ')') {
      depth -= 1;
      if (depth === 0) {
        return { label, href: text.slice(close + 2, cursor), end: cursor + 1 };
      }
    }
  }
  return null;
}

function isAlnum(ch: string): boolean {
  return ch !== '' && /[\p{L}\p{N}]/u.test(ch);
}
