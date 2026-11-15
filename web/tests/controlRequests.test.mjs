import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
import ts from 'typescript';

// Exercise the actual UI handlers without starting polling, a browser or a model.
const source = ts.createSourceFile('main.ts', fs.readFileSync(new URL('../src/main.ts', import.meta.url), 'utf8'), ts.ScriptTarget.Latest, true);
const handlers = source.statements.filter(node => ts.isFunctionDeclaration(node) && ['requestResume', 'requestCancel'].includes(node.name?.text));
assert.equal(handlers.length, 2);
const code = ts.transpile(handlers.map(node => node.getText(source)).join('\n'), { target: ts.ScriptTarget.ES2022 });

function fixture() {
  const calls = [], receipts = new Set();
  let serial = 0, effects = 0, response = 'success';
  const context = {
    Error, TypeError, errorText: error => error.message,
    generation: 1, current: { snapshot: { revision: 1 } }, cancelKeys: new Map(), resumeKeys: new Map(),
    crypto: { randomUUID: () => `request-${++serial}` },
    document: { querySelector: () => ({}) }, notice() {}, async load() {},
    async api(path, body) {
      calls.push({ path, ...body });
      if (response === 'revision_conflict') throw Object.assign(new Error('conflict'), { code: 'revision_conflict' });
      if (!receipts.has(body.request_key)) { receipts.add(body.request_key); effects++; }
      if (response === 'lost') throw new TypeError('response lost after server accepted');
      if (response === 'navigate') context.generation++;
      return {};
    },
  };
  vm.createContext(context); vm.runInContext(code, context);
  return { context, calls, get effects() { return effects; }, set response(value) { response = value; } };
}

for (const [handler, cache] of [['requestResume', 'resumeKeys'], ['requestCancel', 'cancelKeys']]) {
  test(`${handler}: a later control cycle uses a new key and revision`, async () => {
    const f = fixture();
    await f.context[handler]('goal');
    f.context.current.snapshot.revision = 4;
    await f.context[handler]('goal');
    assert.equal(f.effects, 2);
    assert.notEqual(f.calls[0].request_key, f.calls[1].request_key);
    assert.equal(f.calls[1].expected_revision, 4);
  });
  test(`${handler}: an uncertain response retries the same accepted request`, async () => {
    const f = fixture(); f.response = 'lost';
    await f.context[handler]('goal');
    f.context.current.snapshot.revision = 2; f.response = 'success';
    await f.context[handler]('goal');
    assert.equal(f.effects, 1);
    assert.equal(f.calls[0].request_key, f.calls[1].request_key);
    assert.equal(f.calls[1].expected_revision, 1);
    assert.equal(f.context[cache].size, 0);
  });
  test(`${handler}: revision conflict permits a fresh request`, async () => {
    const f = fixture(); f.response = 'revision_conflict';
    await f.context[handler]('goal');
    f.response = 'success'; f.context.current.snapshot.revision = 3;
    await f.context[handler]('goal');
    assert.notEqual(f.calls[0].request_key, f.calls[1].request_key);
    assert.equal(f.calls[1].expected_revision, 3);
  });
  test(`${handler}: confirmed success clears the key after navigation`, async () => {
    const f = fixture(); f.response = 'navigate';
    await f.context[handler]('goal');
    assert.equal(f.context[cache].size, 0);
  });
}
