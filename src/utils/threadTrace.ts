import type { TerminalEntry } from '../types/hermes';
import { dedupeActivityEntries } from './taskInsights';

export type TraceStepIcon = 'reasoning' | 'commentary' | 'spawn' | 'tool' | 'link' | 'status';

export type TraceStepStatus = 'running' | 'completed' | 'failed';

export type TraceToolKind =
  | 'terminal'
  | 'file'
  | 'search'
  | 'skill'
  | 'web'
  | 'agent'
  | 'reasoning'
  | 'commentary'
  | 'tool'
  | 'status';

export interface ClassifiedTraceStep {
  id: string;
  label: string;
  detail: string;
  timestamp: string;
  icon: TraceStepIcon;
  isReasoning?: boolean;
  status?: TraceStepStatus;
  toolKind?: TraceToolKind;
}

export type ThreadTimelineBlock =
  | { kind: 'entry'; id: string; entry: TerminalEntry }
  | {
      kind: 'trace';
      id: string;
      items: ClassifiedTraceStep[];
      isLive: boolean;
    };

export function classifyTraceStep(entry: TerminalEntry): ClassifiedTraceStep {
  const raw = entry.text.trim();
  const lower = raw.toLowerCase();

  if (entry.activityKind === 'reasoning') {
    const detail = (entry.reasoningDetail ?? raw.replace(/^reasoning\s*[:—-]\s*/i, '')).trim();
    return {
      id: entry.id,
      label: 'Reasoning',
      detail,
      timestamp: entry.timestamp,
      icon: 'reasoning',
      isReasoning: true,
      status: entry.activityStatus === 'running' ? 'running' : 'completed',
      toolKind: 'reasoning',
    };
  }

  if (entry.activityKind === 'commentary') {
    const detail = raw.replace(/^commentary\s*[:—-]\s*/i, '').trim();
    return {
      id: entry.id,
      label: 'Commentary',
      detail,
      timestamp: entry.timestamp,
      icon: 'commentary',
      status: 'completed',
      toolKind: 'commentary',
    };
  }

  let icon: TraceStepIcon = 'status';
  if (entry.activityKind === 'subagent') {
    icon = 'spawn';
  } else if (entry.activityKind === 'tool') {
    icon = 'tool';
  } else if (lower.startsWith('reasoning:') || lower.startsWith('reasoning —')) {
    icon = 'reasoning';
  } else if (lower.includes('spawn') || lower.includes('agent') || lower.includes('session')) {
    icon = 'spawn';
  } else if (
    lower.includes('link') ||
    lower.includes('http') ||
    lower.includes('tab') ||
    lower.includes('mcp') ||
    lower.includes('gateway')
  ) {
    icon = 'link';
  } else if (
    lower.includes('run') ||
    lower.includes('exec') ||
    lower.includes('tool') ||
    lower.includes('read') ||
    lower.includes('write') ||
    lower.includes('search') ||
    lower.includes('brief')
  ) {
    icon = 'tool';
  }

  // Split concise label vs trailing detail around colon or em-dash when present
  const colonIdx = raw.indexOf(':');
  const dashIdx = raw.indexOf(' — ');
  let label = raw;
  let detail = '';

  if (colonIdx > 2 && colonIdx < 28) {
    label = raw.slice(0, colonIdx).trim();
    detail = raw.slice(colonIdx + 1).trim();
  } else if (dashIdx > 2 && dashIdx < 28) {
    label = raw.slice(0, dashIdx).trim();
    detail = raw.slice(dashIdx + 3).trim();
  } else {
    const statusMatch = raw.match(
      /^(.+?)\s+(completed|complete|done|finished|running|started|failed|error|cancelled|interrupted)$/i
    );
    if (statusMatch) {
      label = statusMatch[1].trim();
      detail = statusMatch[2].toLowerCase();
    }
  }

  const rawStatus = (entry.activityStatus || '').toLowerCase();
  const detailLower = detail.toLowerCase();
  let status: TraceStepStatus = 'completed';
  if (
    rawStatus === 'running' ||
    rawStatus === 'started' ||
    rawStatus === 'in_progress' ||
    /^(running|started|pending)$/.test(detailLower)
  ) {
    status = 'running';
  } else if (
    rawStatus === 'error' ||
    rawStatus === 'failed' ||
    entry.errorCode ||
    entry.terminalOutcome === 'failed' ||
    /^(failed|error|cancelled|interrupted)$/.test(detailLower)
  ) {
    status = 'failed';
  }

  const labelNorm = (label || raw).toLowerCase().replace(/[_-]+/g, ' ');
  let toolKind: TraceToolKind = icon === 'tool' ? 'tool' : 'status';
  if (icon === 'reasoning') {
    toolKind = 'reasoning';
  } else if (icon === 'spawn') {
    toolKind = 'agent';
  } else if (
    /\b(terminal|bash|shell|sh|exec|command|cmd|process)\b/.test(labelNorm)
  ) {
    toolKind = 'terminal';
  } else if (/\b(skill|skills|memory|recall)\b/.test(labelNorm)) {
    toolKind = 'skill';
  } else if (/\b(search|grep|find|glob|query|lookup)\b/.test(labelNorm)) {
    toolKind = 'search';
  } else if (
    /\b(read|write|edit|patch|file|view|replace|delete|list|dir|fs)\b/.test(labelNorm)
  ) {
    toolKind = 'file';
  } else if (
    icon === 'link' ||
    /\b(web|fetch|http|curl|browser|url|mcp|gateway)\b/.test(labelNorm)
  ) {
    toolKind = 'web';
  }

  return {
    id: entry.id,
    label: label || 'Activity',
    detail,
    timestamp: entry.timestamp,
    icon,
    isReasoning: icon === 'reasoning',
    status,
    toolKind,
  };
}

export function isTerminalDiagnosticActivity(entry: TerminalEntry): boolean {
  if (entry.role !== 'activity') return false;
  if (
    entry.activityKind === 'tool' ||
    entry.activityKind === 'reasoning' ||
    entry.activityKind === 'subagent' ||
    entry.activityKind === 'commentary'
  ) {
    return false;
  }
  return Boolean(
    entry.errorCode ||
      entry.terminalOutcome === 'failed' ||
      entry.terminalOutcome === 'cancelled' ||
      entry.terminalOutcome === 'admission_conflict' ||
      entry.activityStatus === 'error'
  );
}

export interface CoalescedTraceStep extends ClassifiedTraceStep {
  repeatCount: number;
}

/**
 * Collapses consecutive identical completed tool/link/status steps (such as
 * repeated "terminal completed" entries) into one row with a repeat count
 * and latest timestamp while leaving reasoning/commentary steps unmerged.
 */
export function coalesceConsecutiveSteps(
  steps: readonly ClassifiedTraceStep[]
): CoalescedTraceStep[] {
  const result: CoalescedTraceStep[] = [];
  for (const step of steps) {
    const prev = result[result.length - 1];
    const canMerge =
      prev &&
      !step.isReasoning &&
      !prev.isReasoning &&
      step.icon !== 'commentary' &&
      prev.icon !== 'commentary' &&
      prev.icon === step.icon &&
      prev.label.toLowerCase() === step.label.toLowerCase() &&
      prev.detail.toLowerCase() === step.detail.toLowerCase();

    if (canMerge) {
      prev.repeatCount += 1;
      if (step.timestamp) {
        prev.timestamp = step.timestamp;
      }
    } else {
      result.push({ ...step, repeatCount: 1 });
    }
  }
  return result;
}

function isTurnBoundaryEntry(entry: TerminalEntry): boolean {
  return (
    entry.role === 'user' ||
    entry.role === 'command' ||
    entry.role === 'system' ||
    isTerminalDiagnosticActivity(entry)
  );
}

/**
 * Groups consecutive `activity` entries into collapsible vertical trace trees
 * and optionally attaches the live thinking state to the trailing trace tree.
 *
 * Within each conversational turn (delimited by user/command/system/diagnostic
 * boundaries), activity runs and assistant replies keep their chronological
 * interleaving: activity that arrived before a reply renders above it, and
 * activity that arrives after streamed text renders as a NEW trace block below
 * that text — late tool steps are never merged back into the earlier activity
 * tree above the reply. The live thinking state attaches to the trailing trace
 * block of the last turn, and no dangling "Activity · Running" placeholder is
 * emitted below streamed text once the turn has reply text and no running tools.
 */
export function groupThreadTimeline(
  entries: TerminalEntry[],
  isThinking: boolean,
  includeLivePlaceholder = true
): ThreadTimelineBlock[] {
  const blocks: ThreadTimelineBlock[] = [];

  type SegmentFlowItem =
    | { type: 'trace'; items: TerminalEntry[] }
    | { type: 'reply'; entry: TerminalEntry };

  type TurnSegment = {
    boundary: TerminalEntry | null;
    flow: SegmentFlowItem[];
  };

  const segments: TurnSegment[] = [];
  let current: TurnSegment = { boundary: null, flow: [] };
  const hasContent = (seg: TurnSegment) =>
    seg.boundary !== null || seg.flow.length > 0;

  for (const entry of entries) {
    if (isTurnBoundaryEntry(entry)) {
      if (hasContent(current)) {
        segments.push(current);
      }
      current = { boundary: entry, flow: [] };
    } else if (entry.role === 'activity') {
      const last = current.flow[current.flow.length - 1];
      if (last && last.type === 'trace') {
        last.items.push(entry);
      } else {
        current.flow.push({ type: 'trace', items: [entry] });
      }
    } else {
      current.flow.push({ type: 'reply', entry });
    }
  }

  if (hasContent(current)) {
    segments.push(current);
  }

  if (segments.length === 0 && isThinking && includeLivePlaceholder) {
    blocks.push({
      kind: 'trace',
      id: 'trace-live',
      items: [],
      isLive: true,
    });
    return blocks;
  }

  for (let idx = 0; idx < segments.length; idx++) {
    const seg = segments[idx];
    const isLastSegment = idx === segments.length - 1;
    if (seg.boundary) {
      blocks.push({
        kind: 'entry',
        id: seg.boundary.id,
        entry: seg.boundary,
      });
    }

    const segActivities = seg.flow.flatMap((item) =>
      item.type === 'trace' ? item.items : []
    );
    const hasNonEmptyReply = seg.flow.some(
      (item) => item.type === 'reply' && item.entry.text.trim().length > 0
    );
    const hasRunningTool = segActivities.some(
      (a) => a.activityStatus === 'running'
    );
    const liveForSegment =
      isLastSegment && isThinking && (!hasNonEmptyReply || hasRunningTool);

    let lastTraceIdx = -1;
    seg.flow.forEach((item, i) => {
      if (item.type === 'trace') lastTraceIdx = i;
    });

    for (let i = 0; i < seg.flow.length; i++) {
      const item = seg.flow[i];
      if (item.type === 'reply') {
        blocks.push({
          kind: 'entry',
          id: item.entry.id,
          entry: item.entry,
        });
        continue;
      }
      const deduped = dedupeActivityEntries(item.items);
      if (deduped.length === 0) continue;
      blocks.push({
        kind: 'trace',
        id: `trace-${deduped[0].id}`,
        items: deduped.map(classifyTraceStep),
        isLive: liveForSegment && i === lastTraceIdx,
      });
    }

    const endsWithTrace =
      seg.flow.length > 0 && lastTraceIdx === seg.flow.length - 1;
    if (liveForSegment && includeLivePlaceholder && !endsWithTrace) {
      blocks.push({
        kind: 'trace',
        id: 'trace-live',
        items: [],
        isLive: true,
      });
    }
  }

  return blocks;
}
