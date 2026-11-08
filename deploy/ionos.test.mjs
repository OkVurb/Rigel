import test from 'node:test';
import assert from 'node:assert/strict';
import { matchesRecord, planRecord, recordName, stripDot } from './ionos.mjs';

test('stripDot normalizes case and the root dot', () => {
  assert.equal(stripDot(' API.AskRigel.com. '), 'api.askrigel.com');
  assert.equal(stripDot('@'), '@');
});

test('recordName accepts short, fqdn and apex labels', () => {
  assert.equal(recordName('api', 'askrigel.com'), 'api.askrigel.com');
  assert.equal(recordName('api.askrigel.com', 'askrigel.com'), 'api.askrigel.com');
  assert.equal(recordName('@', 'askrigel.com'), 'askrigel.com');
  assert.equal(recordName('', 'askrigel.com'), 'askrigel.com');
  assert.equal(recordName('API.ASKRIGEL.COM.', 'askrigel.com.'), 'api.askrigel.com');
  assert.equal(recordName('api.example.net', 'askrigel.com'), 'api.example.net.askrigel.com');
});

test('matchesRecord compares the normalized name and case-insensitive type', () => {
  assert.equal(
    matchesRecord({ name: 'API.askrigel.com.', type: 'a' }, 'api.askrigel.com', 'A'),
    true,
  );
  assert.equal(
    matchesRecord({ name: 'api.askrigel.com', type: 'AAAA' }, 'api.askrigel.com', 'A'),
    false,
  );
});

test('planRecord creates when no record exists', () => {
  const desired = { name: 'api.askrigel.com', type: 'A', content: '203.0.113.10', ttl: 300 };
  assert.deepEqual(planRecord([], desired), { action: 'create', desired });
});

test('planRecord updates only changed fields', () => {
  const plan = planRecord(
    [
      {
        id: 'record-1',
        name: 'api.askrigel.com',
        type: 'A',
        content: '198.51.100.1',
        ttl: 3600,
        prio: 0,
        disabled: false,
      },
    ],
    { name: 'api.askrigel.com', type: 'A', content: '203.0.113.10', ttl: 300 },
  );
  assert.equal(plan.action, 'update');
  assert.equal(plan.id, 'record-1');
  assert.deepEqual(plan.changes, { content: '203.0.113.10', ttl: 300 });
});

test('planRecord is unchanged when the record already matches', () => {
  const plan = planRecord(
    [
      {
        id: 'record-1',
        name: 'api.askrigel.com',
        type: 'A',
        content: '203.0.113.10',
        ttl: 300,
      },
    ],
    { name: 'api.askrigel.com', type: 'A', content: '203.0.113.10', ttl: 300 },
  );
  assert.equal(plan.action, 'unchanged');
});

test('planRecord refuses to choose between round-robin records', () => {
  const plan = planRecord(
    [
      { id: 'one', name: 'api.askrigel.com', type: 'A', content: '198.51.100.1' },
      { id: 'two', name: 'api.askrigel.com', type: 'A', content: '198.51.100.2' },
    ],
    { name: 'api.askrigel.com', type: 'A', content: '203.0.113.10', ttl: 300 },
  );
  assert.equal(plan.action, 'ambiguous');
  assert.equal(plan.matches.length, 2);
});
