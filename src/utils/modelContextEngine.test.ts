import test from 'node:test';
import assert from 'node:assert/strict';
import {
  computeContextWindowUsage,
  estimateAccurateTokens,
  extractProviderTokenUsage,
  formatCompactTokens,
  parseExternalModelCatalog,
  resolveModelInfoSpec,
  type ContextInputEntry,
} from './modelContextEngine';

test('estimateAccurateTokens weights code/JSON punctuation higher than plain English prose', () => {
  const prose = 'The quick brown fox jumps over the lazy dog in a calm meadow every morning.';
  const code = '{"id":"run_01","args":{"path":"src/utils/modelContextEngine.ts","flags":[true,false,128000]}}';
  const proseTokens = estimateAccurateTokens(prose);
  const codeTokens = estimateAccurateTokens(code);
  assert.ok(proseTokens >= 14 && proseTokens <= 24, `expected prose tokens near 18, got ${proseTokens}`);
  assert.ok(codeTokens > proseTokens, `expected JSON/code (${codeTokens}) to use more tokens than prose (${proseTokens})`);
  assert.equal(estimateAccurateTokens('   '), 0);
});

test('parseExternalModelCatalog parses OpenRouter-shaped model specs with context_length and pricing', () => {
  const parsed = parseExternalModelCatalog(
    {
      data: [
        {
          id: 'anthropic/claude-sonnet-4',
          name: 'Anthropic: Claude Sonnet 4',
          context_length: 200000,
          top_provider: { max_completion_tokens: 64000 },
          architecture: { modality: 'text+image->text' },
          pricing: { prompt: '0.000003', completion: '0.000015' },
        },
        {
          id: 'invalid-no-context',
          name: 'Missing Context',
        },
      ],
    },
    'openrouter-live',
    12345
  );

  assert.equal(parsed.length, 1);
  assert.equal(parsed[0].id, 'anthropic/claude-sonnet-4');
  assert.equal(parsed[0].provider, 'Anthropic');
  assert.equal(parsed[0].displayName, 'Claude Sonnet 4');
  assert.equal(parsed[0].contextWindow, 200000);
  assert.equal(parsed[0].maxOutputTokens, 64000);
  assert.deepEqual(parsed[0].pricing, { inputPerMillion: 3, outputPerMillion: 15 });
  assert.equal(parsed[0].metadataSource, 'openrouter-live');
});

test('resolveModelInfoSpec prioritizes gateway specs over live catalog and handles prefix matching', () => {
  const liveSpecs = parseExternalModelCatalog(
    {
      data: [
        {
          id: 'anthropic/claude-sonnet-4',
          name: 'Anthropic: Claude Sonnet 4',
          context_length: 200000,
          top_provider: { max_completion_tokens: 64000 },
        },
      ],
    },
    'openrouter-live'
  );

  const matched = resolveModelInfoSpec({
    modelId: 'claude-sonnet-4-20250514',
    liveCatalogSpecs: liveSpecs,
  });
  assert.equal(matched.contextWindow, 200000);
  assert.equal(matched.metadataSource, 'openrouter-live');

  const gatewayOverride = resolveModelInfoSpec({
    modelId: 'claude-sonnet-4',
    gatewaySpecs: [
      {
        id: 'claude-sonnet-4',
        displayName: 'Claude Sonnet 4 (1M Beta)',
        provider: 'Anthropic',
        contextWindow: 1_000_000,
        maxOutputTokens: 64_000,
        modality: 'text+image->text',
        metadataSource: 'gateway',
      },
    ],
    liveCatalogSpecs: liveSpecs,
  });
  assert.equal(gatewayOverride.contextWindow, 1_000_000);
  assert.equal(gatewayOverride.metadataSource, 'gateway');
});

test('computeContextWindowUsage calculates segmented breakdown and respects provider usage overrides', () => {
  const model = resolveModelInfoSpec({ modelId: 'openai/gpt-4o' });
  const entries: ContextInputEntry[] = [
    { id: 'init-1', role: 'hermes', text: 'Ignored init greeting' },
    { id: 'u-1', role: 'user', text: 'Summarize the distributed cache invalidation strategy across nodes.' },
    { id: 'a-1', role: 'activity', text: 'grep_search("cache_invalidation")', reasoningDetail: 'Checking Redis TTL hooks' },
    { id: 'h-1', role: 'hermes', text: 'The cache uses write-through invalidation with a 60s TTL fallback and pub/sub fanout.' },
  ];

  const estReport = computeContextWindowUsage({
    model,
    entries,
    skillsCount: 4,
    toolsetsCount: 2,
  });

  assert.equal(estReport.contextWindow, 128_000);
  assert.equal(estReport.measurementSource, 'tokenized-estimate');
  assert.equal(estReport.status, 'nominal');
  const segmentSum = estReport.segments.reduce((acc, s) => acc + s.tokens, 0);
  assert.equal(segmentSum, estReport.usedTokens);
  assert.equal(estReport.usedTokens + estReport.remainingTokens, estReport.contextWindow);

  // Now supply a live provider usage report near the 80% compaction floor
  const providerUsage = extractProviderTokenUsage({
    usage: { prompt_tokens: 105_000, completion_tokens: 2_500, total_tokens: 107_500 },
  });
  assert.ok(providerUsage);

  const liveReport = computeContextWindowUsage({
    model,
    entries,
    lastReportedUsage: providerUsage,
  });
  assert.equal(liveReport.measurementSource, 'provider-usage');
  assert.equal(liveReport.usedTokens, 107_500);
  assert.equal(liveReport.status, 'warning');
  assert.equal(liveReport.segments.reduce((acc, s) => acc + s.tokens, 0), liveReport.usedTokens);
});

test('formatCompactTokens formats small, k, and M token sizes cleanly', () => {
  assert.equal(formatCompactTokens(842), '842');
  assert.equal(formatCompactTokens(4_250), '4.3k');
  assert.equal(formatCompactTokens(131_072), '131k');
  assert.equal(formatCompactTokens(999_680), '999k');
  assert.equal(formatCompactTokens(1_000_000), '1M');
  assert.equal(formatCompactTokens(1_048_576), '1.05M');
});
