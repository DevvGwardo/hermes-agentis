import test from 'node:test';
import assert from 'node:assert/strict';
import type { TerminalEntry } from '../types/hermes';
import {
  classifyTraceStep,
  coalesceConsecutiveSteps,
  groupThreadTimeline,
} from './threadTrace';

function makeEntry(
  id: string,
  role: TerminalEntry['role'],
  text: string,
  timestamp = '10:47'
): TerminalEntry {
  return {
    id,
    channelId: 'general',
    role,
    source: 'ui',
    text,
    timestamp,
  };
}

test('classifyTraceStep categorizes spawn, tool, link, and status steps cleanly', () => {
  const spawn = classifyTraceStep(makeEntry('1', 'activity', 'Agent spawned: researcher'));
  assert.equal(spawn.icon, 'spawn');
  assert.equal(spawn.label, 'Agent spawned');
  assert.equal(spawn.detail, 'researcher');

  const link = classifyTraceStep(makeEntry('2', 'activity', 'MCP link verified'));
  assert.equal(link.icon, 'link');

  const tool = classifyTraceStep(makeEntry('3', 'activity', 'Tool run: brief summary'));
  assert.equal(tool.icon, 'tool');
  assert.equal(tool.label, 'Tool run');
  assert.equal(tool.detail, 'brief summary');

  const toolDone = classifyTraceStep(makeEntry('4', 'activity', 'tool_describe completed'));
  assert.equal(toolDone.icon, 'tool');
  assert.equal(toolDone.label, 'tool_describe');
  assert.equal(toolDone.detail, 'completed');
});

test('groupThreadTimeline collapses adjacent activity entries into a single trace tree', () => {
  const entries: TerminalEntry[] = [
    makeEntry('u1', 'user', 'Check repo status'),
    makeEntry('a1', 'activity', 'Agent spawned: inspector'),
    makeEntry('a2', 'activity', 'Tool run: git status'),
    makeEntry('a3', 'activity', 'Link x2: tabs'),
    makeEntry('h1', 'hermes', 'Everything is clean.'),
  ];

  const blocks = groupThreadTimeline(entries, false);
  assert.equal(blocks.length, 3);
  assert.equal(blocks[0].kind, 'entry');
  assert.equal(blocks[1].kind, 'trace');
  if (blocks[1].kind === 'trace') {
    assert.equal(blocks[1].items.length, 3);
    assert.equal(blocks[1].isLive, false);
  }
  assert.equal(blocks[2].kind, 'entry');
});

test('groupThreadTimeline attaches live thinking state to trailing trace or creates one', () => {
  const withoutTrailingActivity = groupThreadTimeline(
    [makeEntry('u1', 'user', 'Run tests')],
    true
  );
  assert.equal(withoutTrailingActivity.length, 2);
  assert.equal(withoutTrailingActivity[1].kind, 'trace');
  if (withoutTrailingActivity[1].kind === 'trace') {
    assert.equal(withoutTrailingActivity[1].isLive, true);
    assert.equal(withoutTrailingActivity[1].items.length, 0);
  }

  const withTrailingActivity = groupThreadTimeline(
    [makeEntry('u1', 'user', 'Run tests'), makeEntry('a1', 'activity', 'Tool run: npm test')],
    true
  );
  assert.equal(withTrailingActivity.length, 2);
  if (withTrailingActivity[1].kind === 'trace') {
    assert.equal(withTrailingActivity[1].isLive, true);
    assert.equal(withTrailingActivity[1].items.length, 1);
  }
});

test('groupThreadTimeline deduplicates tool pairs and respects includeLivePlaceholder=false', () => {
  const hiddenLive = groupThreadTimeline([makeEntry('u1', 'user', 'Audit repo')], true, false);
  assert.equal(hiddenLive.length, 1);
  assert.equal(hiddenLive[0].kind, 'entry');

  const dedupedTrace = groupThreadTimeline(
    [
      makeEntry('u1', 'user', 'Inspect CI'),
      { ...makeEntry('a1', 'activity', 'Running read_file'), activityStatus: 'running' },
      { ...makeEntry('a2', 'activity', 'read_file: ci-workflows.test.ts'), activityStatus: 'done' },
      { ...makeEntry('a3', 'activity', 'Running terminal'), activityStatus: 'running' },
    ],
    false
  );
  assert.equal(dedupedTrace.length, 2);
  assert.equal(dedupedTrace[1].kind, 'trace');
  if (dedupedTrace[1].kind === 'trace') {
    assert.equal(dedupedTrace[1].items.length, 2);
    assert.equal(dedupedTrace[1].items[0].label, 'read_file');
  }
});

test('classifyTraceStep preserves multiline reasoning_content and commentary steps', () => {
  const reasoning = classifyTraceStep({
    ...makeEntry('r1', 'activity', 'Reasoning: Step 1\nStep 2'),
    activityKind: 'reasoning',
    reasoningDetail: 'Step 1\nStep 2',
  });
  assert.equal(reasoning.icon, 'reasoning');
  assert.equal(reasoning.isReasoning, true);
  assert.equal(reasoning.label, 'Reasoning');
  assert.equal(reasoning.detail, 'Step 1\nStep 2');

  const commentary = classifyTraceStep({
    ...makeEntry('c1', 'activity', 'Checking git diff first'),
    activityKind: 'commentary',
  });
  assert.equal(commentary.icon, 'commentary');
  assert.equal(commentary.label, 'Commentary');
  assert.equal(commentary.detail, 'Checking git diff first');
});

test('groupThreadTimeline renders terminal diagnostic failures as standalone entry blocks', () => {
  const blocks = groupThreadTimeline(
    [
      makeEntry('u1', 'user', 'Check build status'),
      {
        ...makeEntry('e1', 'activity', 'Hermes Gateway offline at http://127.0.0.1:8642'),
        activityKind: 'status',
        activityStatus: 'error',
        errorCode: 'gateway_offline',
        terminalOutcome: 'failed',
      },
    ],
    false,
    false
  );
  assert.equal(blocks.length, 2);
  assert.equal(blocks[1].kind, 'entry');
  if (blocks[1].kind === 'entry') {
    assert.equal(blocks[1].entry.errorCode, 'gateway_offline');
  }
});

test('groupThreadTimeline keeps activity runs in chronological order around assistant replies', () => {
  const chronological = groupThreadTimeline(
    [
      makeEntry('u1', 'user', 'what should we work on', '08:21'),
      makeEntry('h1', 'hermes', 'Scanning ledgers...', '08:22'),
      { ...makeEntry('a1', 'activity', 'terminal completed', '08:23'), activityKind: 'tool' },
      { ...makeEntry('a2', 'activity', 'terminal completed', '08:24'), activityKind: 'tool' },
    ],
    false
  );
  assert.equal(chronological.length, 3);
  assert.equal(chronological[0].kind, 'entry');
  assert.equal(chronological[1].kind, 'entry');
  if (chronological[1].kind === 'entry') {
    assert.equal(chronological[1].entry.id, 'h1');
  }
  assert.equal(chronological[2].kind, 'trace');
  if (chronological[2].kind === 'trace') {
    assert.equal(chronological[2].items.length, 2);
    assert.equal(chronological[2].isLive, false);
  }

  const activeStreaming = groupThreadTimeline(
    [
      makeEntry('u2', 'user', 'simplify this list', '08:25'),
      makeEntry('h2', 'hermes', 'Here is the simplified list:', '08:25'),
    ],
    true,
    true
  );
  assert.equal(activeStreaming.length, 2);
  assert.equal(activeStreaming.every((b) => b.kind === 'entry'), true);
});

test('groupThreadTimeline renders post-reply activity as a NEW trace block below streamed text', () => {
  const blocks = groupThreadTimeline(
    [
      makeEntry('u1', 'user', 'fix the failing test', '09:00'),
      { ...makeEntry('a1', 'activity', 'search_files completed', '09:01'), activityKind: 'tool' },
      makeEntry('h1', 'hermes', 'Found it in threadTrace.ts. Patching now.', '09:02'),
      { ...makeEntry('a2', 'activity', 'Running patch', '09:03'), activityKind: 'tool', activityStatus: 'running' },
      { ...makeEntry('a3', 'activity', 'patch completed', '09:04'), activityKind: 'tool' },
    ],
    true
  );

  // Expected: user entry, pre-text trace, streamed reply, NEW live trace below the reply
  assert.equal(blocks.length, 4);
  assert.equal(blocks[0].kind, 'entry');
  assert.equal(blocks[1].kind, 'trace');
  if (blocks[1].kind === 'trace') {
    assert.equal(blocks[1].items.length, 1);
    assert.equal(blocks[1].isLive, false);
  }
  assert.equal(blocks[2].kind, 'entry');
  if (blocks[2].kind === 'entry') {
    assert.equal(blocks[2].entry.id, 'h1');
  }
  assert.equal(blocks[3].kind, 'trace');
  if (blocks[3].kind === 'trace') {
    // "Running patch" is upgraded in place by dedupe when "patch completed" lands
    assert.equal(blocks[3].items.length, 1);
    assert.equal(blocks[3].isLive, true);
  }
});

test('coalesceConsecutiveSteps merges adjacent identical steps with repeat counts and preserves reasoning', () => {
  const steps = [
    classifyTraceStep({ ...makeEntry('1', 'activity', 'terminal completed', '08:23'), activityKind: 'tool' }),
    classifyTraceStep({ ...makeEntry('2', 'activity', 'terminal completed', '08:24'), activityKind: 'tool' }),
    classifyTraceStep({ ...makeEntry('3', 'activity', 'terminal completed', '08:25'), activityKind: 'tool' }),
    classifyTraceStep({
      ...makeEntry('4', 'activity', 'Reasoning: Checking logs', '08:25'),
      activityKind: 'reasoning',
      reasoningDetail: 'Checking logs',
    }),
    classifyTraceStep({ ...makeEntry('5', 'activity', 'terminal completed', '08:26'), activityKind: 'tool' }),
  ];

  const coalesced = coalesceConsecutiveSteps(steps);
  assert.equal(coalesced.length, 3);
  assert.equal(coalesced[0].repeatCount, 3);
  assert.equal(coalesced[0].timestamp, '08:25');
  assert.equal(coalesced[1].icon, 'reasoning');
  assert.equal(coalesced[1].repeatCount, 1);
  assert.equal(coalesced[2].repeatCount, 1);
});

test('classifyTraceStep derives semantic toolKind and status for terminal, skill, file, search, and error steps', () => {
  const term = classifyTraceStep({
    ...makeEntry('t1', 'activity', 'terminal: git status + 2 commands'),
    activityKind: 'tool',
    activityStatus: 'running',
  });
  assert.equal(term.toolKind, 'terminal');
  assert.equal(term.status, 'running');

  const skill = classifyTraceStep({
    ...makeEntry('t2', 'activity', 'skill_view completed'),
    activityKind: 'tool',
  });
  assert.equal(skill.toolKind, 'skill');
  assert.equal(skill.status, 'completed');

  const fileStep = classifyTraceStep({
    ...makeEntry('t3', 'activity', 'read_file: src/utils/appUpdater.ts'),
    activityKind: 'tool',
  });
  assert.equal(fileStep.toolKind, 'file');
  assert.equal(fileStep.status, 'completed');

  const failedStep = classifyTraceStep({
    ...makeEntry('t4', 'activity', 'grep_search failed'),
    activityKind: 'tool',
    activityStatus: 'error',
  });
  assert.equal(failedStep.toolKind, 'search');
  assert.equal(failedStep.status, 'failed');
});

