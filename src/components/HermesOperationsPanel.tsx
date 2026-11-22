import { useState } from 'react';
import {
  Check,
  CheckCircle2,
  Cpu,
  Pause,
  Play,
  Plus,
  RefreshCw,
  Send,
  Trash2,
  X,
} from 'lucide-react';
import type {
  ApprovalCard,
  BriefDecision,
  Channel,
  ChannelId,
  HermesCronJob,
  HermesGatewayStatus,
  HermesProfileMode,
  HermesProfileSummary,
  HermesSkill,
  HermesToolset,
  HermesTransportMode,
} from '../types/hermes';
import { formatDisplaySessionTitle, formatSourceLabel } from '../utils/sidebarSessions';
import { EmptyState, MorphTabs, StaggerItem, StatusPulse, type MorphTabItem } from './ui';

interface HermesOperationsPanelProps {
  channels: Channel[];
  activeChannel: ChannelId;
  onSelectChannel: (id: ChannelId) => void;
  approvals: ApprovalCard[];
  onApproveCard: (id: string, choice?: 'once' | 'always' | 'deny') => void;
  onRejectCard: (id: string) => void;
  onClarifyCard?: (id: string, answer: string) => void;
  brief: BriefDecision[];
  onRunBriefCommand: (command: string, briefId: string) => void;
  gatewayStatus?: HermesGatewayStatus | null;
  onRefreshGateway?: () => Promise<unknown> | void;
  skills?: HermesSkill[];
  toolsets?: HermesToolset[];
  cronJobs?: HermesCronJob[];
  transportMode?: HermesTransportMode;
  onChangeTransportMode?: (mode: HermesTransportMode) => void;
  onCreateJob?: (payload: { prompt: string; schedule: string; name?: string }) => Promise<unknown>;
  onPauseJob?: (jobId: string) => Promise<void>;
  onResumeJob?: (jobId: string) => Promise<void>;
  onRunJob?: (jobId: string) => Promise<void>;
  onDeleteJob?: (jobId: string) => Promise<void>;
  profiles?: HermesProfileSummary[];
  selectedProfileId?: string;
  onSelectDefaultProfile?: (profileId: string) => void;
  onSaveProfile?: (payload: {
    id: string;
    label?: string;
    mode: HermesProfileMode;
    baseUrl?: string;
    apiKey?: string;
    description?: string;
  }) => Promise<unknown>;
  onDeleteProfile?: (profileId: string) => Promise<unknown>;
  sessionCountByProfile?: Record<string, number>;
}

type OpsTab = 'runtime' | 'approvals' | 'jobs';

export function HermesOperationsPanel({
  channels,
  activeChannel,
  onSelectChannel,
  approvals,
  onApproveCard,
  onRejectCard,
  onClarifyCard,
  gatewayStatus,
  onRefreshGateway,
  skills = [],
  toolsets = [],
  cronJobs = [],
  transportMode = 'auto',
  onChangeTransportMode,
  onCreateJob,
  onPauseJob,
  onResumeJob,
  onRunJob,
  onDeleteJob,
  profiles = [],
  selectedProfileId = 'default',
  onSelectDefaultProfile,
  onSaveProfile,
  onDeleteProfile,
  sessionCountByProfile = {},
}: HermesOperationsPanelProps) {
  const pendingApprovals = approvals.filter((a) => a.status === 'pending').length;

  const [activeTab, setActiveTab] = useState<OpsTab>(
    pendingApprovals > 0 ? 'approvals' : 'runtime'
  );
  const [clarifyInputs, setClarifyInputs] = useState<Record<string, string>>({});
  const [refreshing, setRefreshing] = useState(false);
  const [expandedToolset, setExpandedToolset] = useState<string | null>(null);
  const [showNewJobForm, setShowNewJobForm] = useState(false);
  const [newJobSchedule, setNewJobSchedule] = useState('0 9 * * *');
  const [newJobPrompt, setNewJobPrompt] = useState('');
  const [newJobName, setNewJobName] = useState('');
  const [jobBusyId, setJobBusyId] = useState<string | null>(null);
  const [jobError, setJobError] = useState<string | null>(null);
  const [showAddProfileForm, setShowAddProfileForm] = useState(false);
  const [profileIdInput, setProfileIdInput] = useState('');
  const [profileModeInput, setProfileModeInput] = useState<HermesProfileMode>('multiplexed');
  const [profileBaseUrlInput, setProfileBaseUrlInput] = useState('');
  const [profileApiKeyInput, setProfileApiKeyInput] = useState('');
  const [profileBusy, setProfileBusy] = useState(false);
  const [profileError, setProfileError] = useState<string | null>(null);

  const opsTabs: MorphTabItem<OpsTab>[] = [
    { id: 'runtime', label: 'Gateway & Tools' },
    { id: 'approvals', label: `Approvals (${pendingApprovals})` },
    { id: 'jobs', label: `Jobs (${cronJobs.length})` },
  ];

  const handleRefresh = async () => {
    if (!onRefreshGateway) return;
    setRefreshing(true);
    try {
      await onRefreshGateway();
    } finally {
      window.setTimeout(() => setRefreshing(false), 260);
    }
  };

  const handleCreateJobSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!onCreateJob || !newJobPrompt.trim() || !newJobSchedule.trim()) return;
    setJobError(null);
    setJobBusyId('create');
    try {
      await onCreateJob({
        prompt: newJobPrompt.trim(),
        schedule: newJobSchedule.trim(),
        name: newJobName.trim() || undefined,
      });
      setNewJobPrompt('');
      setNewJobName('');
      setShowNewJobForm(false);
    } catch (err) {
      setJobError(err instanceof Error ? err.message : 'Failed to create job');
    } finally {
      setJobBusyId(null);
    }
  };

  const runJobAction = async (jobId: string, fn?: (id: string) => Promise<void>) => {
    if (!fn) return;
    setJobError(null);
    setJobBusyId(jobId);
    try {
      await fn(jobId);
    } catch (err) {
      setJobError(err instanceof Error ? err.message : 'Job action failed');
    } finally {
      setJobBusyId(null);
    }
  };

  const handleSaveProfileSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!onSaveProfile) return;
    const cleanId = profileIdInput.trim().toLowerCase();
    if (!cleanId) {
      setProfileError('Enter a profile slug (e.g. coder, research).');
      return;
    }
    if (profileModeInput === 'standalone' && !profileBaseUrlInput.trim()) {
      setProfileError('Standalone profiles need a gateway URL (e.g. http://127.0.0.1:8643).');
      return;
    }
    setProfileError(null);
    setProfileBusy(true);
    try {
      await onSaveProfile({
        id: cleanId,
        label: cleanId,
        mode: profileModeInput,
        baseUrl: profileModeInput === 'standalone' ? profileBaseUrlInput.trim() : undefined,
        apiKey: profileApiKeyInput.trim() || undefined,
      });
      setProfileIdInput('');
      setProfileBaseUrlInput('');
      setProfileApiKeyInput('');
      setShowAddProfileForm(false);
    } catch (err) {
      setProfileError(err instanceof Error ? err.message : 'Failed to save profile');
    } finally {
      setProfileBusy(false);
    }
  };

  const handleRemoveProfile = async (profileId: string) => {
    if (!onDeleteProfile) return;
    setProfileError(null);
    setProfileBusy(true);
    try {
      await onDeleteProfile(profileId);
    } catch (err) {
      setProfileError(err instanceof Error ? err.message : 'Failed to remove profile');
    } finally {
      setProfileBusy(false);
    }
  };

  const isConnected = Boolean(gatewayStatus?.connected);
  const capabilities = gatewayStatus?.capabilities;
  const routeDisplay = `${gatewayStatus?.gatewayUrl || 'http://127.0.0.1:8642'}${gatewayStatus?.routePrefix || ''}`;

  const activeChannelObj = channels.find((ch) => ch.id === activeChannel) ?? channels[0] ?? null;
  const activeScopeTitle = activeChannelObj
    ? formatDisplaySessionTitle({
        topic: activeChannelObj.description,
        name: String(activeChannelObj.name ?? activeChannelObj.id).replace(/^#/, ''),
        channelId: String(activeChannelObj.id ?? ''),
        sessionId: String(activeChannelObj.sessionId ?? activeChannelObj.id ?? ''),
        sourceLabel: formatSourceLabel(
          undefined,
          String(activeChannelObj.sessionId ?? activeChannelObj.id ?? ''),
          String(activeChannelObj.id ?? '')
        ),
      }).title
    : 'No active session';

  return (
    <section
      aria-label="Hermes Agent Operations"
      data-testid="hermes-operations-panel"
      className="flex flex-col"
    >
      {/* Clean Single-Line Header & Underline Navigation */}
      <div className="flex flex-wrap items-baseline justify-between gap-2 pb-2">
        <div className="flex flex-wrap items-baseline gap-x-2.5 gap-y-0.5 min-w-0">
          <h1 className="text-base font-semibold text-white tracking-tight shrink-0">
            Queue &amp; Operations
          </h1>
          <span className="text-xs text-zinc-400 truncate">
            {activeChannelObj ? activeScopeTitle : 'Gateway health, approvals & scheduled jobs'}
          </span>
        </div>

        {channels.length > 1 && (
          <label className="inline-flex items-center gap-1.5 text-[11px] text-zinc-400 shrink-0">
            <span className="sr-only">Switch active session</span>
            <select
              aria-label="Switch active session"
              value={activeChannel}
              onChange={(e) => onSelectChannel(e.target.value)}
              className="rounded-md bg-white/[0.04] border border-white/10 hover:border-white/20 px-2 py-1 font-mono text-[11px] text-zinc-200 focus:outline-none focus:border-[var(--ui-accent)] max-w-[180px] cursor-pointer"
            >
              {channels.map((ch) => {
                const safeChannelId = String(ch.id ?? '');
                const safeSessionId = String(ch.sessionId ?? safeChannelId);
                const rawName = String(ch.name ?? safeChannelId).replace(/^#/, '');
                const { title } = formatDisplaySessionTitle({
                  topic: ch.description,
                  name: rawName,
                  channelId: safeChannelId,
                  sessionId: safeSessionId,
                  sourceLabel: formatSourceLabel(undefined, safeSessionId, safeChannelId),
                });
                return (
                  <option key={ch.id} value={ch.id}>
                    {title}
                  </option>
                );
              })}
            </select>
          </label>
        )}
      </div>

      {/* Underline Tab Navigation aligned with top workspace bar */}
      <div className="border-b border-white/[0.08] overflow-x-auto cyber-scroll">
        <MorphTabs
          items={opsTabs}
          value={activeTab}
          onChange={setActiveTab}
          ariaLabel="Hermes Agent sections"
          variant="underline"
        />
      </div>

      {/* Tab Content — flat single-elevation rhythm */}
      <div
        key={activeTab}
        className="ui-card-enter pt-4 space-y-4"
      >
        {activeTab === 'approvals' &&
          (approvals.length === 0 ? (
            <EmptyState
              icon={<CheckCircle2 className="w-4 h-4" />}
              title="No pending run approvals"
              description="Pending execution gates from /v1/runs/{id}/events and pending_controls will appear here when a run requests confirmation."
            />
          ) : (
            approvals.map((card, idx) => {
              const isPending = card.status === 'pending';
              const isClarify = card.kind === 'clarify';
              const clarifyVal = clarifyInputs[card.id] ?? '';
              return (
                <StaggerItem key={card.id} index={idx}>
                  <div
                    data-testid={`approval-card-${card.id}`}
                    className="rounded-xl bg-white/[0.04] border border-white/10 hover:border-white/20 transition-colors p-3 space-y-2.5"
                  >
                    <div className="flex items-start justify-between gap-2.5">
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] font-mono text-zinc-400">
                          <span className="max-w-[160px] truncate">run:{card.runId || card.id}</span>
                          {card.requestId && (
                            <>
                              <span aria-hidden="true">·</span>
                              <span className="max-w-[140px] truncate">req:{card.requestId}</span>
                            </>
                          )}
                          {typeof card.executionGeneration === 'number' && (
                            <>
                              <span aria-hidden="true">·</span>
                              <span>gen:{card.executionGeneration}</span>
                            </>
                          )}
                          <span aria-hidden="true">·</span>
                          <span className="max-w-[140px] truncate">#{card.channelId}</span>
                          {card.mcpTool && (
                            <>
                              <span aria-hidden="true">·</span>
                              <span className="text-[var(--ui-accent-soft)]">{card.mcpTool}</span>
                            </>
                          )}
                        </div>
                        <h3 className="text-sm font-semibold text-white mt-0.5 break-words">{card.title}</h3>
                      </div>
                      <span
                        className={`inline-flex items-center gap-1.5 font-mono text-[11px] uppercase font-semibold shrink-0 ${
                          card.status === 'approved'
                            ? 'text-[var(--ui-accent-soft)]'
                            : card.status === 'rejected'
                            ? 'text-rose-400'
                            : 'text-amber-300'
                        }`}
                      >
                        {isPending && <StatusPulse state="live" size="sm" />}
                        <span>{card.status}</span>
                      </span>
                    </div>

                    {card.summary && (
                      <p className="text-xs text-zinc-200 whitespace-pre-wrap">{card.summary}</p>
                    )}

                    {(card.command || card.draft) && (
                      <pre className="rounded-lg bg-black/45 border border-white/10 p-2 font-mono text-[11px] text-[var(--ui-accent-soft)] overflow-x-auto whitespace-pre-wrap">
                        {card.command || card.draft}
                      </pre>
                    )}

                    {isPending && isClarify && onClarifyCard ? (
                      <div className="flex items-center gap-2 pt-1">
                        <input
                          type="text"
                          value={clarifyVal}
                          onChange={(e) =>
                            setClarifyInputs((prev) => ({ ...prev, [card.id]: e.target.value }))
                          }
                          placeholder="Enter clarification answer..."
                          className="flex-1 min-w-0 rounded-lg bg-black/35 border border-white/12 px-2.5 py-2 font-mono text-xs text-white focus:outline-none focus:border-[var(--ui-accent)]"
                        />
                        <button
                          type="button"
                          onClick={() => onClarifyCard(card.id, clarifyVal)}
                          disabled={!clarifyVal.trim()}
                          className="ui-accent-btn min-h-[36px] px-3 rounded-lg font-semibold text-xs inline-flex items-center gap-1.5 hover:opacity-90 disabled:opacity-45 cursor-pointer"
                        >
                          <Send className="w-3.5 h-3.5" />
                          <span>Reply</span>
                        </button>
                      </div>
                    ) : isPending ? (
                      <div className="flex flex-wrap items-center gap-2 pt-1">
                        <button
                          type="button"
                          data-testid={`approve-once-${card.id}`}
                          onClick={() => onApproveCard(card.id, 'once')}
                          className="ui-accent-btn flex-1 min-h-[38px] px-3 rounded-lg font-semibold text-xs flex items-center justify-center gap-1.5 hover:opacity-90 transition-opacity cursor-pointer"
                        >
                          <Check className="w-4 h-4" />
                          <span>Approve Once</span>
                        </button>
                        <button
                          type="button"
                          data-testid={`approve-always-${card.id}`}
                          onClick={() => onApproveCard(card.id, 'always')}
                          className="min-h-[38px] px-3 rounded-lg bg-[rgba(var(--ui-accent-rgb),0.16)] text-[var(--ui-accent-soft)] border border-[rgba(var(--ui-accent-rgb),0.36)] font-semibold text-xs flex items-center justify-center gap-1.5 hover:bg-[rgba(var(--ui-accent-rgb),0.24)] transition-colors cursor-pointer"
                        >
                          <span>Always Allow</span>
                        </button>
                        <button
                          type="button"
                          data-testid={`approve-deny-${card.id}`}
                          onClick={() => onRejectCard(card.id)}
                          className="min-h-[38px] px-3.5 rounded-lg bg-rose-500/12 text-rose-200 border border-rose-400/30 text-xs flex items-center justify-center gap-1.5 hover:bg-rose-500/22 transition-colors cursor-pointer"
                        >
                          <X className="w-4 h-4" />
                          <span>Deny</span>
                        </button>
                      </div>
                    ) : null}
                  </div>
                </StaggerItem>
              );
            })
          ))}

        {activeTab === 'jobs' && (
          <div className="space-y-2.5">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="font-mono text-xs text-white font-semibold min-w-0 truncate">
                Scheduled Jobs (GET /api/jobs)
              </div>
              <div className="flex items-center gap-2 shrink-0">
                {onRefreshGateway && (
                  <button
                    type="button"
                    onClick={() => void handleRefresh()}
                    className="ui-tactile px-2.5 py-1 rounded-md bg-white/[0.06] border border-white/10 text-[11px] font-mono text-zinc-300 hover:text-white cursor-pointer"
                  >
                    Refresh
                  </button>
                )}
                {onCreateJob && isConnected && (
                  <button
                    type="button"
                    onClick={() => setShowNewJobForm((v) => !v)}
                    className="ui-tactile px-2.5 py-1 rounded-md bg-[rgba(var(--ui-accent-rgb),0.16)] border border-[rgba(var(--ui-accent-rgb),0.36)] text-[11px] font-mono text-[var(--ui-accent-soft)] hover:bg-[rgba(var(--ui-accent-rgb),0.24)] inline-flex items-center gap-1 cursor-pointer"
                  >
                    <Plus className="w-3 h-3" />
                    <span>New Job</span>
                  </button>
                )}
              </div>
            </div>

            {jobError && (
              <div className="rounded-lg bg-rose-500/15 border border-rose-400/30 px-3 py-2 font-mono text-[11px] text-rose-200">
                {jobError}
              </div>
            )}

            {showNewJobForm && onCreateJob && (
              <form
                onSubmit={handleCreateJobSubmit}
                className="rounded-xl bg-black/35 border border-white/15 p-3 space-y-2"
              >
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                  <input
                    type="text"
                    value={newJobName}
                    onChange={(e) => setNewJobName(e.target.value)}
                    placeholder="Name (optional)"
                    className="rounded-lg bg-black/45 border border-white/12 px-2.5 py-1.5 font-mono text-xs text-white focus:outline-none focus:border-[var(--ui-accent)]"
                  />
                  <input
                    type="text"
                    value={newJobSchedule}
                    onChange={(e) => setNewJobSchedule(e.target.value)}
                    placeholder="Cron or schedule (e.g. 0 9 * * *)"
                    className="rounded-lg bg-black/45 border border-white/12 px-2.5 py-1.5 font-mono text-xs text-white focus:outline-none focus:border-[var(--ui-accent)]"
                  />
                </div>
                <textarea
                  rows={2}
                  value={newJobPrompt}
                  onChange={(e) => setNewJobPrompt(e.target.value)}
                  placeholder="Job prompt to execute on gateway..."
                  className="w-full rounded-lg bg-black/45 border border-white/12 px-2.5 py-1.5 font-mono text-xs text-white focus:outline-none focus:border-[var(--ui-accent)]"
                />
                <div className="flex items-center justify-end gap-2">
                  <button
                    type="button"
                    onClick={() => setShowNewJobForm(false)}
                    className="px-3 py-1 rounded-md text-xs text-zinc-400 hover:text-white cursor-pointer"
                  >
                    Cancel
                  </button>
                  <button
                    type="submit"
                    disabled={jobBusyId === 'create' || !newJobPrompt.trim()}
                    className="ui-accent-btn px-3 py-1 rounded-md font-semibold text-xs hover:opacity-90 disabled:opacity-45 cursor-pointer"
                  >
                    Create Job
                  </button>
                </div>
              </form>
            )}

            {cronJobs.length === 0 ? (
              <EmptyState
                icon={<Cpu className="w-4 h-4" />}
                title={isConnected ? 'No scheduled jobs on gateway' : 'Gateway offline'}
                description={
                  isConnected
                    ? 'Create a job above or via cron tools; live jobs from GET /api/jobs will appear here.'
                    : 'Connect to the Hermes Agent API Server to inspect and manage /api/jobs.'
                }
              />
            ) : (
              cronJobs.map((job, idx) => {
                const isPaused = job.paused || job.status === 'paused';
                const busy = jobBusyId === job.id;
                return (
                  <StaggerItem key={job.id} index={idx}>
                    <div
                      data-testid={`cron-job-${job.id}`}
                      className="rounded-xl bg-white/[0.04] border border-white/10 p-3 space-y-2"
                    >
                      <div className="flex flex-wrap sm:flex-nowrap items-start justify-between gap-2">
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-2 font-mono text-[11px]">
                            <span className="text-[var(--ui-accent-soft)] font-semibold">{job.schedule}</span>
                            <span className="text-zinc-600">·</span>
                            <span className={isPaused ? 'text-amber-300' : 'text-[var(--ui-accent-soft)]'}>
                              {job.status || (isPaused ? 'paused' : 'active')}
                            </span>
                            <span className="text-zinc-600">·</span>
                            <span className="text-zinc-400 truncate">{job.id}</span>
                          </div>
                          <p className="text-xs text-white mt-1 break-words">{job.summary}</p>
                        </div>

                        <div className="flex items-center gap-1 shrink-0">
                          {onRunJob && (
                            <button
                              type="button"
                              disabled={busy}
                              onClick={() => void runJobAction(job.id, onRunJob)}
                              title="Trigger job now (POST /api/jobs/{id}/run)"
                              className="px-2 py-1 rounded-md bg-white/[0.06] border border-white/10 text-[11px] font-mono text-[var(--ui-accent-soft)] hover:bg-white/12 cursor-pointer"
                            >
                              Run
                            </button>
                          )}
                          {isPaused
                            ? onResumeJob && (
                                <button
                                  type="button"
                                  disabled={busy}
                                  onClick={() => void runJobAction(job.id, onResumeJob)}
                                  title="Resume job"
                                  className="p-1.5 rounded-md bg-white/[0.06] border border-white/10 text-zinc-200 hover:text-white cursor-pointer"
                                >
                                  <Play className="w-3 h-3" />
                                </button>
                              )
                            : onPauseJob && (
                                <button
                                  type="button"
                                  disabled={busy}
                                  onClick={() => void runJobAction(job.id, onPauseJob)}
                                  title="Pause job"
                                  className="p-1.5 rounded-md bg-white/[0.06] border border-white/10 text-amber-200 hover:text-white cursor-pointer"
                                >
                                  <Pause className="w-3 h-3" />
                                </button>
                              )}
                          {onDeleteJob && (
                            <button
                              type="button"
                              disabled={busy}
                              onClick={() => void runJobAction(job.id, onDeleteJob)}
                              title="Delete job"
                              className="p-1.5 rounded-md bg-white/[0.06] border border-white/10 text-rose-300 hover:bg-rose-500/20 cursor-pointer"
                            >
                              <Trash2 className="w-3 h-3" />
                            </button>
                          )}
                        </div>
                      </div>
                    </div>
                  </StaggerItem>
                );
              })
            )}
          </div>
        )}

        {activeTab === 'runtime' && (
          <div className="divide-y divide-white/[0.07]">
            {/* Gateway Status & Route */}
            <div className="pb-4 space-y-3">
              <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
                <div className="flex items-center gap-2 min-w-0">
                  <StatusPulse state={isConnected ? 'live' : 'error'} size="sm" />
                  <span className="text-white font-semibold">Hermes Agent API Server</span>
                  <span aria-hidden="true" className="text-zinc-600">·</span>
                  <span
                    data-testid="ops-gateway-status"
                    className={`font-mono text-[11px] font-medium ${
                      isConnected ? 'text-[var(--ui-accent-soft)]' : 'text-rose-300'
                    }`}
                  >
                    {isConnected ? 'Connected' : 'Offline'}
                  </span>
                </div>
                <div className="flex items-center gap-2 shrink-0">
                  {!isConnected && gatewayStatus?.error && (
                    <span className="hidden sm:inline font-mono text-[11px] text-rose-300/80 truncate max-w-[240px]">
                      {gatewayStatus.error}
                    </span>
                  )}
                  {onRefreshGateway && (
                    <button
                      type="button"
                      onClick={() => void handleRefresh()}
                      aria-label="Probe Hermes Gateway"
                      title="Probe Hermes Gateway"
                      className="ui-tactile px-2.5 py-1 rounded-md bg-white/[0.05] hover:bg-white/[0.10] text-[11px] font-medium text-zinc-200 hover:text-white inline-flex items-center gap-1.5 cursor-pointer whitespace-nowrap"
                    >
                      <RefreshCw
                        className={`w-3 h-3 ${refreshing ? 'motion-safe:animate-spin motion-safe-only' : ''}`}
                      />
                      <span>{isConnected ? 'Refresh' : 'Retry'}</span>
                    </button>
                  )}
                </div>
              </div>

              {/* Clean unboxed 4-column telemetry row */}
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-x-4 gap-y-2 py-2.5 border-y border-white/[0.06] font-mono text-[11px]">
                <div className="min-w-0">
                  <div className="text-[10px] text-zinc-500">Endpoint</div>
                  <div className="text-zinc-200 truncate mt-0.5 tabular-nums" title={routeDisplay}>
                    {routeDisplay.replace(/^https?:\/\//, '')}
                  </div>
                </div>
                <div className="min-w-0">
                  <div className="text-[10px] text-zinc-500">Model</div>
                  <div className="text-[var(--ui-accent-soft)] truncate mt-0.5">
                    {gatewayStatus?.activeModel || 'hermes-agent'}
                  </div>
                </div>
                <div className="min-w-0">
                  <div className="text-[10px] text-zinc-500">Profile</div>
                  <div className="text-zinc-200 truncate mt-0.5">
                    @{gatewayStatus?.profile || 'default'}
                  </div>
                </div>
                <div className="min-w-0">
                  <div className="text-[10px] text-zinc-500">Transport</div>
                  <div className="text-zinc-200 truncate mt-0.5">
                    {gatewayStatus?.resolvedTransport || 'chat_completions'}
                  </div>
                </div>
              </div>

              {onChangeTransportMode && (
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="text-[11.5px] text-zinc-400">
                    Turn transport
                  </span>
                  <div className="inline-flex flex-wrap items-center gap-0.5 p-0.5 rounded-lg bg-white/[0.035]">
                    {(
                      [
                        ['auto', 'Auto'],
                        ['runs', '/v1/runs'],
                        ['session_stream', '/sessions/stream'],
                        ['chat_completions', '/v1/chat/completions'],
                      ] as const
                    ).map(([mode, label]) => (
                      <button
                        key={mode}
                        type="button"
                        onClick={() => onChangeTransportMode(mode)}
                        className={`px-2.5 py-1 rounded-md font-mono text-[10.5px] transition-colors cursor-pointer whitespace-nowrap ${
                          transportMode === mode
                            ? 'bg-[rgba(var(--ui-accent-rgb),0.16)] text-[var(--ui-accent-soft)] font-medium'
                            : 'text-zinc-400 hover:text-white'
                        }`}
                      >
                        {label}
                      </button>
                    ))}
                  </div>
                </div>
              )}

              {capabilities && (
                <div className="font-mono text-[10.5px] text-zinc-400 flex flex-wrap items-center gap-2">
                  <span>Capabilities:</span>
                  {capabilities.features?.run_submission && (
                    <span className="text-[var(--ui-accent-soft)]">runs</span>
                  )}
                  {capabilities.features?.run_approval && (
                    <span className="text-[var(--ui-accent-soft)]">approvals</span>
                  )}
                  {capabilities.features?.session_chat_stream && (
                    <span className="text-[var(--ui-accent-soft)]">session_stream</span>
                  )}
                  {capabilities.features?.reasoning_streaming && (
                    <span className="text-[var(--ui-accent-soft)]">reasoning</span>
                  )}
                  {capabilities.sessionKeyHeader && (
                    <span className="text-[var(--ui-accent-soft)]">{capabilities.sessionKeyHeader}</span>
                  )}
                </div>
              )}
            </div>

            {/* Parallel Profiles (~/.hermes/profiles/* & Gateway Routes) */}
            <div data-testid="ops-parallel-profiles" className="py-4 space-y-2.5">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="min-w-0">
                  <h3 className="text-xs font-semibold text-white">
                    Parallel Profiles
                  </h3>
                  <p className="text-[11px] text-zinc-400 mt-0.5">
                    Isolated memories and tools per profile
                  </p>
                </div>
                {onSaveProfile && (
                  <button
                    type="button"
                    data-testid="ops-add-profile-toggle"
                    onClick={() => {
                      setProfileError(null);
                      setShowAddProfileForm((v) => !v);
                    }}
                    className="ui-tactile px-2.5 py-1 rounded-md bg-white/[0.05] hover:bg-white/[0.10] text-[11px] font-medium text-zinc-200 hover:text-white inline-flex items-center gap-1 cursor-pointer shrink-0 whitespace-nowrap"
                  >
                    <Plus className="w-3 h-3" />
                    <span>Add Profile</span>
                  </button>
                )}
              </div>

              {profileError && (
                <div className="rounded-lg bg-rose-500/12 px-3 py-2 font-mono text-[11px] text-rose-200">
                  {profileError}
                </div>
              )}

              {showAddProfileForm && onSaveProfile && (
                <form
                  data-testid="ops-add-profile-form"
                  onSubmit={(e) => void handleSaveProfileSubmit(e)}
                  className="rounded-xl bg-black/35 border border-white/15 p-3 space-y-2.5"
                >
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                    <div className="space-y-1">
                      <label htmlFor="profile-id-input" className="block font-mono text-[10.5px] text-zinc-300">
                        Profile Slug
                      </label>
                      <input
                        id="profile-id-input"
                        data-testid="ops-profile-id-input"
                        type="text"
                        value={profileIdInput}
                        onChange={(e) => setProfileIdInput(e.target.value)}
                        placeholder="e.g. coder, research"
                        className="w-full rounded-lg bg-black/45 border border-white/12 px-2.5 py-1.5 font-mono text-xs text-white focus:outline-none focus:border-[var(--ui-accent)]"
                      />
                    </div>
                    <div className="space-y-1">
                      <label htmlFor="profile-mode-select" className="block font-mono text-[10.5px] text-zinc-300">
                        Gateway Mode
                      </label>
                      <select
                        id="profile-mode-select"
                        data-testid="ops-profile-mode-select"
                        value={profileModeInput}
                        onChange={(e) => setProfileModeInput(e.target.value as HermesProfileMode)}
                        className="w-full rounded-lg bg-black/45 border border-white/12 px-2.5 py-1.5 font-mono text-xs text-white focus:outline-none focus:border-[var(--ui-accent)]"
                      >
                        <option value="multiplexed">Multiplexed (/p/&lt;name&gt;)</option>
                        <option value="standalone">Standalone Gateway Port</option>
                      </select>
                    </div>
                  </div>

                  {profileModeInput === 'standalone' && (
                    <div className="space-y-1">
                      <label htmlFor="profile-baseurl-input" className="block font-mono text-[10.5px] text-zinc-300">
                        Standalone Gateway URL (hermes -p &lt;name&gt; gateway start)
                      </label>
                      <input
                        id="profile-baseurl-input"
                        data-testid="ops-profile-baseurl-input"
                        type="text"
                        value={profileBaseUrlInput}
                        onChange={(e) => setProfileBaseUrlInput(e.target.value)}
                        placeholder="http://127.0.0.1:8643"
                        className="w-full rounded-lg bg-black/45 border border-white/12 px-2.5 py-1.5 font-mono text-xs text-white focus:outline-none focus:border-[var(--ui-accent)]"
                      />
                    </div>
                  )}

                  <div className="space-y-1">
                    <label htmlFor="profile-apikey-input" className="block font-mono text-[10.5px] text-zinc-300">
                      Profile API Key (optional, stored server-side only)
                    </label>
                    <input
                      id="profile-apikey-input"
                      data-testid="ops-profile-apikey-input"
                      type="password"
                      value={profileApiKeyInput}
                      onChange={(e) => setProfileApiKeyInput(e.target.value)}
                      placeholder="Leave empty to inherit primary or env key"
                      className="w-full rounded-lg bg-black/45 border border-white/12 px-2.5 py-1.5 font-mono text-xs text-white focus:outline-none focus:border-[var(--ui-accent)]"
                    />
                  </div>

                  <div className="flex items-center justify-end gap-2">
                    <button
                      type="button"
                      onClick={() => {
                        setShowAddProfileForm(false);
                        setProfileError(null);
                      }}
                      className="px-3 py-1 rounded-md text-xs text-zinc-400 hover:text-white cursor-pointer"
                    >
                      Cancel
                    </button>
                    <button
                      type="submit"
                      data-testid="ops-profile-save-btn"
                      disabled={profileBusy}
                      className="ui-accent-btn px-3 py-1 rounded-md font-semibold text-xs hover:opacity-90 disabled:opacity-45 cursor-pointer"
                    >
                      {profileBusy ? 'Saving…' : 'Save Profile'}
                    </button>
                  </div>
                </form>
              )}

              <div className="divide-y divide-white/[0.06] border-t border-white/[0.06]">
                {(profiles.length > 0
                  ? profiles
                  : [
                      {
                        id: gatewayStatus?.profile || 'default',
                        label: gatewayStatus?.profile || 'default',
                        mode: 'multiplexed' as const,
                        source: 'primary' as const,
                        baseUrl: gatewayStatus?.gatewayUrl || 'http://127.0.0.1:8642',
                        routePrefix: gatewayStatus?.routePrefix || '',
                        hasApiKey: false,
                        connected: isConnected,
                        activeModel: gatewayStatus?.activeModel,
                      },
                    ]
                ).map((p) => {
                  const isDefault = p.id === selectedProfileId;
                  const count = sessionCountByProfile[p.id] ?? 0;
                  const routeLabel =
                    p.mode === 'standalone'
                      ? p.baseUrl
                      : `${p.baseUrl}${p.routePrefix || (p.id === 'default' ? '' : `/p/${p.id}`)}`;
                  return (
                    <div
                      key={p.id}
                      data-testid={`ops-profile-row-${p.id}`}
                      className="py-2.5 flex flex-wrap sm:flex-nowrap items-center justify-between gap-2 text-xs"
                    >
                      <div className="min-w-0 flex-1 space-y-0.5">
                        <div className="flex items-center gap-2 font-mono">
                          <StatusPulse state={p.connected ? 'live' : 'idle'} size="sm" />
                          <span className="text-white font-semibold truncate">@{p.id}</span>
                          <span className="text-[10.5px] text-zinc-500 shrink-0">
                            {p.mode === 'standalone' ? 'standalone' : p.routePrefix || '/'}
                          </span>
                          {p.activeModel && (
                            <span className="hidden sm:inline text-[10.5px] text-[var(--ui-accent-soft)] truncate">
                              · {p.activeModel}
                            </span>
                          )}
                          {count > 0 && (
                            <span className="text-[10.5px] text-zinc-400 shrink-0 tabular-nums">
                              · {count} {count === 1 ? 'lane' : 'lanes'}
                            </span>
                          )}
                        </div>
                        <div className="font-mono text-[10.5px] text-zinc-500 truncate">
                          {routeLabel} · {p.source}
                        </div>
                      </div>

                      <div className="flex items-center gap-1.5 shrink-0">
                        {onSelectDefaultProfile && (
                          <button
                            type="button"
                            onClick={() => onSelectDefaultProfile(p.id)}
                            className={`px-2 py-1 rounded-md font-mono text-[10.5px] transition-colors cursor-pointer ${
                              isDefault
                                ? 'bg-[rgba(var(--ui-accent-rgb),0.16)] text-[var(--ui-accent-soft)] font-medium'
                                : 'bg-white/[0.04] text-zinc-400 hover:text-white hover:bg-white/[0.08]'
                            }`}
                          >
                            {isDefault ? 'Default' : 'Use default'}
                          </button>
                        )}
                        {p.source === 'custom' && onDeleteProfile && (
                          <button
                            type="button"
                            disabled={profileBusy}
                            aria-label={`Delete profile ${p.id}`}
                            onClick={() => void handleRemoveProfile(p.id)}
                            className="p-1 rounded-md text-rose-300/80 hover:text-rose-200 hover:bg-rose-500/15 transition-colors cursor-pointer"
                          >
                            <Trash2 className="w-3.5 h-3.5" />
                          </button>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>

            {/* Toolsets from GET /v1/toolsets */}
            <div className="py-3 space-y-1.5">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <h3 className="text-xs font-semibold text-white">
                  Toolsets
                </h3>
                <span className="font-mono text-[10.5px] text-zinc-500 tabular-nums">
                  {toolsets.length} reported · GET /v1/toolsets
                </span>
              </div>
              {toolsets.length === 0 ? (
                <p className="font-mono text-[11.5px] text-zinc-400">
                  {isConnected ? '0 toolsets returned by gateway.' : 'Offline — start hermes gateway to inspect toolsets.'}
                </p>
              ) : (
                <div className="space-y-1.5">
                  {toolsets.map((ts) => {
                    const isOpen = expandedToolset === ts.name;
                    return (
                      <div
                        key={ts.name}
                        className="border-b border-white/[0.06] last:border-0 pb-1.5 last:pb-0 text-xs"
                      >
                        <button
                          type="button"
                          onClick={() => setExpandedToolset(isOpen ? null : ts.name)}
                          className="w-full text-left flex items-center justify-between gap-2 font-mono text-[11px] cursor-pointer hover:text-white min-w-0"
                        >
                          <span className={`truncate ${ts.enabled ? 'text-[var(--ui-accent-soft)] font-semibold' : 'text-zinc-500'}`}>
                            {ts.label}
                          </span>
                          <span className="text-zinc-400 shrink-0 whitespace-nowrap">
                            {ts.enabled ? (ts.configured === false ? 'unconfigured' : 'enabled') : 'disabled'} · {ts.tools.length} tools
                          </span>
                        </button>
                        {ts.description && (
                          <p className="text-zinc-400 text-[11px] mt-0.5 break-words">{ts.description}</p>
                        )}
                        {isOpen && ts.tools.length > 0 && (
                          <div className="mt-1 font-mono text-[10.5px] text-zinc-300 bg-black/30 rounded p-2 break-words">
                            {ts.tools.join(', ')}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
            </div>

            {/* Skills from GET /v1/skills */}
            <div className="py-3 space-y-1.5">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <h3 className="text-xs font-semibold text-white">
                  Skills
                </h3>
                <span className="font-mono text-[10.5px] text-zinc-500 tabular-nums">
                  {skills.length} loaded · GET /v1/skills
                </span>
              </div>
              {skills.length === 0 ? (
                <p className="font-mono text-[11.5px] text-zinc-400">
                  {isConnected ? '0 skills returned by gateway.' : 'Offline — start hermes gateway to inspect /v1/skills.'}
                </p>
              ) : (
                <div className="space-y-1.5">
                  {skills.map((skill) => (
                    <div
                      key={skill.name}
                      className="text-xs border-b border-white/[0.06] last:border-0 pb-1.5 last:pb-0"
                    >
                      <div className="flex items-center justify-between font-mono text-[11px]">
                        <span className="text-[var(--ui-accent-soft)] font-semibold">{skill.name}</span>
                        <span className="text-zinc-500 truncate max-w-[55%]">
                          {skill.source}
                        </span>
                      </div>
                      {skill.summary && (
                        <p className="text-zinc-400 mt-0.5">{skill.summary}</p>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>

            {/* Honest Memory Scoping Summary */}
            <div className="pt-3 space-y-1">
              <h3 className="text-xs font-semibold text-white">
                Memory &amp; Session Continuity
              </h3>
              <p className="font-mono text-[11px] text-zinc-400 leading-relaxed">
                Transcript continuity uses <span className="text-[var(--ui-accent-soft)]">X-Hermes-Session-Id</span>; isolated Honcho/long-term memory scoping sends{' '}
                <span className="text-[var(--ui-accent-soft)]">{gatewayStatus?.sessionKeyHeader || 'X-Hermes-Session-Key'}</span> per lane. Hermes API Server does not expose a separate /v1/memory endpoint.
              </p>
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
