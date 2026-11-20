import assert from 'node:assert/strict';
import test from 'node:test';
import { httpHref, splitInline } from '../../work/verify/web-tests/articleInline.js';

test('a markdown link shows its label and a citation stays a citation', () => {
  const pieces = splitInline('[Rust](https://example.com) is fast [1].');
  assert.deepEqual(pieces, [
    { kind: 'link', label: 'Rust', href: 'https://example.com' },
    { kind: 'text', text: ' is fast ' },
    { kind: 'cite', marker: '[1]' },
    { kind: 'text', text: '.' },
  ]);
  assert.equal(httpHref('https://example.com'), 'https://example.com/');
  assert.equal(httpHref('javascript:alert(1)'), null);
});

test('single emphasis renders and an underscore inside a word stays', () => {
  assert.deepEqual(splitInline('The result is *significant*.'), [
    { kind: 'text', text: 'The result is ' },
    { kind: 'em', text: 'significant' },
    { kind: 'text', text: '.' },
  ]);
  assert.deepEqual(splitInline('The result is _significant_.'), [
    { kind: 'text', text: 'The result is ' },
    { kind: 'em', text: 'significant' },
    { kind: 'text', text: '.' },
  ]);
  assert.deepEqual(splitInline('The field is foo_bar_baz.'), [
    { kind: 'text', text: 'The field is foo_bar_baz.' },
  ]);
  assert.deepEqual(splitInline('The result is 2 * 3.'), [
    { kind: 'text', text: 'The result is 2 * 3.' },
  ]);
});

test('bold markers render and a digit citation is not a link', () => {
  assert.deepEqual(splitInline('A saved **official** statement [1].'), [
    { kind: 'text', text: 'A saved ' },
    { kind: 'strong', text: 'official' },
    { kind: 'text', text: ' statement ' },
    { kind: 'cite', marker: '[1]' },
    { kind: 'text', text: '.' },
  ]);
  assert.deepEqual(splitInline('A saved __official__ statement.'), [
    { kind: 'text', text: 'A saved ' },
    { kind: 'strong', text: 'official' },
    { kind: 'text', text: ' statement.' },
  ]);
});

test('emphasis inside a code span stays literal', () => {
  assert.deepEqual(splitInline('Use `*x*` now [1].'), [
    { kind: 'text', text: 'Use ' },
    { kind: 'code', text: '*x*' },
    { kind: 'text', text: ' now ' },
    { kind: 'cite', marker: '[1]' },
    { kind: 'text', text: '.' },
  ]);
});
