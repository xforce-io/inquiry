// Citation compatibility must follow identity, not the spelling of an evidence ID.
import assert from 'node:assert/strict';
import test from 'node:test';
import { buildCitationAliases, resolveCitationIndex } from '../../work/verify/web-tests/citationRefs.js';

const a = { artifact_id: 'saved-a', sha256: 'a'.repeat(64), source_id: 'r1s1', paragraph: 4, quote: 'exact first quote' };
const b = { ...a, source_id: 'r1s2', quote: 'exact second quote' };

test('numeric references remain canonical and within the report array', () => {
  assert.equal(resolveCitationIndex('1', 2), 0);
  assert.equal(resolveCitationIndex('2', 2), 1);
  for (const marker of ['0', '01', '3', '999999999999999999999', 'E01']) {
    assert.equal(resolveCitationIndex(marker, 2), undefined);
  }
});
test('evidence labels map to matching tuples even when citation order differs', () => {
  const aliases = buildCitationAliases([b, a], [{ ...a, id: 'E01' }, { ...b, id: 'E07' }]);
  assert.equal(resolveCitationIndex('E01', 2, aliases), 1);
  assert.equal(resolveCitationIndex('E07', 2, aliases), 0);
  assert.equal(resolveCitationIndex('E02', 2, aliases), undefined);
});
test('every tuple field must match; matching an ID or source number is insufficient', () => {
  for (const key of ['artifact_id', 'sha256', 'source_id', 'paragraph', 'quote']) {
    const wrong = { ...a, [key]: key === 'paragraph' ? 99 : 'different', id: 'E01' };
    assert.equal(buildCitationAliases([a], [wrong]).has('E01'), false);
  }
});
test('ambiguous evidence IDs and duplicated citation tuples have no alias', () => {
  assert.equal(buildCitationAliases([a, b], [{ ...a, id: 'E01' }, { ...b, id: 'E01' }]).has('E01'), false);
  assert.equal(buildCitationAliases([a, a], [{ ...a, id: 'E01' }]).has('E01'), false);
});
test('numeric references never fall back to an opaque numeric evidence alias', () => {
  const aliases = new Map([['0', 0], ['01', 0], ['999', 0]]);
  for (const marker of ['0', '01', '999']) {
    assert.equal(resolveCitationIndex(marker, 1, aliases), undefined);
  }
});
