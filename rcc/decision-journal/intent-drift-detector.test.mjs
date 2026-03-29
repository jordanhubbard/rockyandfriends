/**
 * IntentDriftDetector tests
 * Run: node --test rcc/decision-journal/intent-drift-detector.test.mjs
 */
import { test, describe } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'fs';
import { join } from 'path';
import { tmpdir } from 'os';
import { DecisionJournal } from './index.mjs';
import { buildBaseline, detectDrift, driftReport, IntentDriftDetector } from './intent-drift-detector.mjs';

function tmpJournal() {
  const dir = mkdtempSync(join(tmpdir(), 'drift-test-'));
  const logPath = join(dir, 'decision-journal.jsonl');
  const j = new DecisionJournal({ agent: 'test-agent', logPath, silent: true });
  return { dir, logPath, j };
}

function populateJournal(j, entries) {
  for (const e of entries) j.log(e);
}

const STABLE_ENTRY = { principle_used: 'fail-safe', confidence: 0.9, was_conflict: false, outcome: 'proceed' };
const LOW_CONF     = { principle_used: 'fail-safe', confidence: 0.3, was_conflict: true,  outcome: 'blocked' };
const NEW_PRIN     = { principle_used: 'speed',     confidence: 0.7, was_conflict: false, outcome: 'proceed' };

describe('buildBaseline', () => {
  test('returns insufficient_data when fewer than 5 entries', () => {
    const { j, dir } = tmpJournal();
    populateJournal(j, [STABLE_ENTRY, STABLE_ENTRY, STABLE_ENTRY]);
    const b = buildBaseline({ journal: j, agent: 'test-agent' });
    assert.ok(b.insufficient_data);
    rmSync(dir, { recursive: true });
  });

  test('returns baseline stats with enough entries', () => {
    const { j, dir } = tmpJournal();
    for (let i = 0; i < 20; i++) populateJournal(j, [STABLE_ENTRY]);
    const b = buildBaseline({ journal: j, agent: 'test-agent', windowSize: 20 });
    assert.ok(!b.insufficient_data);
    assert.equal(b.avg_confidence, 0.9);
    assert.equal(b.conflict_rate, 0);
    assert.ok(b.principle_dist['fail-safe'] > 0.99);
    rmSync(dir, { recursive: true });
  });
});

describe('detectDrift', () => {
  test('no alert when recent matches baseline', () => {
    const { j, dir } = tmpJournal();
    // populate baseline + recent with same pattern
    for (let i = 0; i < 70; i++) populateJournal(j, [STABLE_ENTRY]);
    const result = detectDrift({ journal: j, agent: 'test-agent', windowSize: 20, baselineWindow: 50 });
    assert.ok(!result.alert, `should not alert, got score=${result.drift_score}`);
    assert.ok(result.drift_score < 0.25);
    rmSync(dir, { recursive: true });
  });

  test('alerts when confidence drops significantly', () => {
    const { j, dir } = tmpJournal();
    // stable baseline
    for (let i = 0; i < 50; i++) populateJournal(j, [STABLE_ENTRY]);
    // drifted recent window
    for (let i = 0; i < 20; i++) populateJournal(j, [LOW_CONF]);
    const result = detectDrift({ journal: j, agent: 'test-agent', windowSize: 20, baselineWindow: 50 });
    assert.ok(result.alert, `should alert on confidence drop, got score=${result.drift_score}`);
    assert.ok(result.drift_score > 0.25);
    rmSync(dir, { recursive: true });
  });

  test('alerts when principle distribution shifts', () => {
    const { j, dir } = tmpJournal();
    for (let i = 0; i < 50; i++) populateJournal(j, [STABLE_ENTRY]);
    for (let i = 0; i < 20; i++) populateJournal(j, [NEW_PRIN]);
    const result = detectDrift({ journal: j, agent: 'test-agent', windowSize: 20, baselineWindow: 50 });
    assert.ok(result.alert || result.drift_score > 0.1, `principle shift should register drift`);
    rmSync(dir, { recursive: true });
  });

  test('returns insufficient_data reason with tiny journal', () => {
    const { j, dir } = tmpJournal();
    populateJournal(j, [STABLE_ENTRY]);
    const result = detectDrift({ journal: j, agent: 'test-agent' });
    assert.equal(result.reason, 'insufficient_data');
    assert.ok(!result.alert);
    rmSync(dir, { recursive: true });
  });

  test('drift_score is between 0 and 1 in normal conditions', () => {
    const { j, dir } = tmpJournal();
    for (let i = 0; i < 70; i++) populateJournal(j, [STABLE_ENTRY]);
    const result = detectDrift({ journal: j, agent: 'test-agent', windowSize: 20, baselineWindow: 50 });
    assert.ok(result.drift_score >= 0 && result.drift_score <= 1);
    rmSync(dir, { recursive: true });
  });

  test('respects custom driftThreshold', () => {
    const { j, dir } = tmpJournal();
    for (let i = 0; i < 70; i++) populateJournal(j, [STABLE_ENTRY]);
    // With threshold=0 everything is an alert
    const result = detectDrift({ journal: j, agent: 'test-agent', windowSize: 20, baselineWindow: 50, driftThreshold: 0 });
    assert.ok(result.alert);
    rmSync(dir, { recursive: true });
  });
});

describe('driftReport', () => {
  test('returns a string with status line', () => {
    const { j, dir } = tmpJournal();
    for (let i = 0; i < 70; i++) populateJournal(j, [STABLE_ENTRY]);
    const result = detectDrift({ journal: j, agent: 'test-agent', windowSize: 20, baselineWindow: 50 });
    const report = driftReport(result);
    assert.ok(typeof report === 'string');
    assert.ok(report.includes('score='));
    rmSync(dir, { recursive: true });
  });

  test('includes DRIFT ALERT when alert=true', () => {
    const { j, dir } = tmpJournal();
    for (let i = 0; i < 50; i++) populateJournal(j, [STABLE_ENTRY]);
    for (let i = 0; i < 20; i++) populateJournal(j, [LOW_CONF]);
    const result = detectDrift({ journal: j, agent: 'test-agent', windowSize: 20, baselineWindow: 50 });
    const report = driftReport(result);
    if (result.alert) assert.ok(report.includes('DRIFT ALERT'));
    rmSync(dir, { recursive: true });
  });
});

describe('IntentDriftDetector class', () => {
  test('throws without journal', () => {
    assert.throws(() => new IntentDriftDetector({}), /journal is required/);
  });

  test('captureBaseline and check workflow', () => {
    const { j, dir } = tmpJournal();
    for (let i = 0; i < 60; i++) populateJournal(j, [STABLE_ENTRY]);
    const detector = new IntentDriftDetector({ journal: j, agent: 'test-agent', windowSize: 20, baselineWindow: 50 });
    const base = detector.captureBaseline();
    assert.ok(!base.insufficient_data, 'baseline should have enough data');
    const result = detector.check();
    assert.ok(!result.alert, 'stable pattern should not alert');
    rmSync(dir, { recursive: true });
  });

  test('report() returns a string', () => {
    const { j, dir } = tmpJournal();
    for (let i = 0; i < 70; i++) populateJournal(j, [STABLE_ENTRY]);
    const detector = new IntentDriftDetector({ journal: j, agent: 'test-agent' });
    detector.captureBaseline();
    const report = detector.report();
    assert.ok(typeof report === 'string' && report.length > 10);
    rmSync(dir, { recursive: true });
  });
});
