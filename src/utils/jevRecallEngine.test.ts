import test from 'node:test';
import assert from 'node:assert/strict';
import type { TerminalEntry } from '../types/hermes';
import {
  JEV_CONTEXT_LIMITS,
  UNIICHAT_RECALL_BENCHMARKS,
  analyzeSessionEntries,
  buildJevRecallProjection,
  buildTypeSafeJevRequestPayload,
  computeRecallAccuracyAtPoint,
  parseTypeSafeJevResponse,
  redactCredentialTokens,
} from './jevRecallEngine';

function makeEntries(turnPairs: number, charsPerMessage = 420): TerminalEntry[] {
  const entries: TerminalEntry[] = [];
  for (let i = 0; i < turnPairs; i++) {
    const topicWord = `module${i}_component${i}_spec${i}`;
    entries.push({
      id: `u-${i}`,
      role: 'user',
      text: `Please inspect ${topicWord} and verify the recall constraints. `.repeat(
        Math.max(1, Math.round(charsPerMessage / 65))
      ),
      timestamp: '10:00',
      channelId: 'ch-1',
    });
    entries.push({
      id: `a-${i}`,
      role: 'hermes',
      text: `Checked ${topicWord}; coverage + BM25-lite matched evidence cleanly. `.repeat(
        Math.max(1, Math.round(charsPerMessage / 70))
      ),
      timestamp: '10:01',
      channelId: 'ch-1',
    });
  }
  return entries;
}

test('UNIICHAT_RECALL_BENCHMARKS preserves published DevvGwardo/uniichat-recall hit@1 hierarchy', () => {
  assert.equal(UNIICHAT_RECALL_BENCHMARKS['coverage-rerank'].hitAt1, 0.933);
  assert.equal(UNIICHAT_RECALL_BENCHMARKS['keyword-first-pass'].hitAt1, 0.858);
  assert.equal(UNIICHAT_RECALL_BENCHMARKS['bm25-only'].hitAt1, 0.742);
  assert.ok(
    UNIICHAT_RECALL_BENCHMARKS['coverage-rerank'].hitAt1 >
      UNIICHAT_RECALL_BENCHMARKS['keyword-first-pass'].hitAt1
  );
  assert.ok(
    UNIICHAT_RECALL_BENCHMARKS['keyword-first-pass'].hitAt1 >
      UNIICHAT_RECALL_BENCHMARKS['bm25-only'].hitAt1
  );
});

test('redactCredentialTokens masks sk-, ghp_, vck_, apikey_, and Bearer secrets', () => {
  const raw =
    'Keys: sk-live1234567890, ghp_AbCdEfGhIjKl1234, vck_9988776655, apikey_secret1234, Bearer eyJhbGciOiJIUzI1NiJ9';
  const clean = redactCredentialTokens(raw);
  assert.doesNotMatch(clean, /sk-live/);
  assert.doesNotMatch(clean, /ghp_AbCd/);
  assert.doesNotMatch(clean, /vck_9988/);
  assert.doesNotMatch(clean, /apikey_secret/);
  assert.doesNotMatch(clean, /eyJhbGci/);
  assert.match(clean, /\[REDACTED_SK\]/);
  assert.match(clean, /\[REDACTED_GH\]/);
  assert.match(clean, /\[REDACTED_VCK\]/);
  assert.match(clean, /\[REDACTED_APIKEY\]/);
  assert.match(clean, /\[REDACTED_TOKEN\]/);
});

test('computeRecallAccuracyAtPoint decays monotonically with turn depth and context pressure', () => {
  const fresh = computeRecallAccuracyAtPoint({
    mode: 'coverage-rerank',
    turn: 2,
    tokens: 1200,
    topicDriftRatio: 0.15,
    activityNoiseRatio: 0.05,
    queryExpansion: true,
  });
  const mid = computeRecallAccuracyAtPoint({
    mode: 'coverage-rerank',
    turn: 16,
    tokens: 12_000,
    topicDriftRatio: 0.45,
    activityNoiseRatio: 0.18,
    queryExpansion: true,
  });
  const heavy = computeRecallAccuracyAtPoint({
    mode: 'coverage-rerank',
    turn: 34,
    tokens: 34_000,
    topicDriftRatio: 0.72,
    activityNoiseRatio: 0.30,
    queryExpansion: true,
  });

  assert.ok(fresh > mid, `Expected fresh (${fresh}) > mid (${mid})`);
  assert.ok(mid > heavy, `Expected mid (${mid}) > heavy (${heavy})`);

  // Query expansion helps under high paraphrase/topic drift
  const withExpansion = computeRecallAccuracyAtPoint({
    mode: 'coverage-rerank',
    turn: 12,
    tokens: 9000,
    topicDriftRatio: 0.8,
    activityNoiseRatio: 0.1,
    queryExpansion: true,
  });
  const withoutExpansion = computeRecallAccuracyAtPoint({
    mode: 'coverage-rerank',
    turn: 12,
    tokens: 9000,
    topicDriftRatio: 0.8,
    activityNoiseRatio: 0.1,
    queryExpansion: false,
  });
  assert.ok(withExpansion > withoutExpansion);
});

test('buildJevRecallProjection predicts a clear reset turn and flags reset-now on deep sessions', () => {
  const freshProj = buildJevRecallProjection({
    entries: makeEntries(2, 260),
    mode: 'coverage-rerank',
    resetThreshold: 0.78,
  });
  assert.equal(freshProj.healthStatus, 'fresh');
  assert.ok(freshProj.turnsRemaining > 10);
  assert.ok(freshProj.recommendedResetTurn > freshProj.snapshot.turnCount);
  assert.ok(freshProj.points.length >= 25);

  // Original Hermes snapshot comparison should be lower than uniichat-recall at deep turns
  const tail = freshProj.points[freshProj.points.length - 1];
  assert.ok(tail.accuracy > tail.originalHermesAccuracy);

  // Very deep / dense session should recommend starting a new session
  const heavyProj = buildJevRecallProjection({
    entries: makeEntries(36, 1600),
    mode: 'coverage-rerank',
    resetThreshold: 0.78,
  });
  assert.equal(heavyProj.healthStatus, 'reset-now');
  assert.equal(heavyProj.turnsRemaining, 0);
  assert.ok(heavyProj.jevQuestionBudgetPct >= 80);
});

test('buildTypeSafeJevRequestPayload and parseTypeSafeJevResponse conform to TypeSafe /v1/systemone schema', () => {
  const snap = analyzeSessionEntries(makeEntries(5, 320));
  const req = buildTypeSafeJevRequestPayload({
    sessionId: 'sess-sk-secret99999999',
    mode: 'coverage-rerank',
    queryExpansion: true,
    resetThreshold: 0.78,
    snapshot: snap,
  });

  assert.equal(req.model, JEV_CONTEXT_LIMITS.defaultModel);
  assert.doesNotMatch(req.state.session_id, /sk-secret/);
  assert.equal(req.questions.recall_grounded.type, 'noul');
  assert.equal(req.questions.abstention_safe.type, 'noul');
  assert.equal(req.questions.context_coherence.type, 'score');
  assert.equal(req.questions.session_action.type, 'choice');

  const parsed = parseTypeSafeJevResponse(
    {
      model: 'jev-1.14.0',
      answers: {
        recall_grounded: { type: 'noul', noul: 0.91 },
        abstention_safe: { type: 'noul', noul: 0.98 },
        context_coherence: { type: 'score', score: 'sharp' },
        session_action: {
          type: 'choice',
          choice: 'continue',
          probabilities: { continue: 0.89, compact: 0.08, start_new_session: 0.03 },
        },
      },
      usage: { input_tokens: 1840 },
    },
    112
  );

  assert.ok(parsed);
  assert.equal(parsed.source, 'typesafe-jev-live');
  assert.equal(parsed.model, 'jev-1.14.0');
  assert.equal(parsed.recallGroundedNoul, 0.91);
  assert.equal(parsed.abstentionSafeNoul, 0.98);
  assert.equal(parsed.contextCoherence, 'sharp');
  assert.equal(parsed.sessionAction, 'continue');
  assert.equal(parsed.inputTokens, 1840);
});
