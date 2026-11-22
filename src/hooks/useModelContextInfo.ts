import { useCallback, useEffect, useMemo, useState } from 'react';
import type { TerminalEntry } from '../types/hermes';
import {
  computeContextWindowUsage,
  resolveModelInfoSpec,
  type ContextWindowUsageReport,
  type ModelInfoResponse,
  type ModelInfoSpec,
} from '../utils/modelContextEngine';

interface UseModelContextInfoParams {
  activeModelId?: string;
  activeProvider?: string;
  activeProfile?: string;
  activeChannel?: string;
  entries: TerminalEntry[];
  skillsCount?: number;
  toolsetsCount?: number;
}

export interface UseModelContextInfoResult {
  modelInfo: ModelInfoResponse | null;
  inspectedModel: ModelInfoSpec;
  availableModels: ModelInfoSpec[];
  selectedModelId: string | null;
  setSelectedModelId: (modelId: string | null) => void;
  usage: ContextWindowUsageReport;
  loading: boolean;
  refreshing: boolean;
  refresh: (force?: boolean) => Promise<void>;
}

export function useModelContextInfo({
  activeModelId,
  activeProvider,
  activeProfile,
  activeChannel,
  entries,
  skillsCount = 0,
  toolsetsCount = 0,
}: UseModelContextInfoParams): UseModelContextInfoResult {
  const [modelInfo, setModelInfo] = useState<ModelInfoResponse | null>(null);
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);

  const fetchModelInfo = useCallback(
    async (force = false) => {
      if (force) setRefreshing(true);
      const params = new URLSearchParams();
      if (activeProfile) params.set('profile', activeProfile);
      if (activeModelId && activeModelId !== 'hermes-agent') {
        params.set('model', activeModelId);
      }
      if (activeChannel) params.set('channelId', activeChannel);
      if (force) params.set('refresh', '1');

      try {
        const res = await fetch(`/api/hermes/model-info?${params.toString()}`);
        if (res.ok) {
          const data = (await res.json()) as ModelInfoResponse;
          if (data && data.activeModel) {
            setModelInfo(data);
          }
        }
      } catch {
        // Retain last fetched specs or fallback cleanly
      } finally {
        setLoading(false);
        setRefreshing(false);
      }
    },
    [activeModelId, activeProfile, activeChannel]
  );

  // Fetch on mount & when active model / profile / channel / turn count changes
  useEffect(() => {
    void fetchModelInfo(false);
  }, [fetchModelInfo, entries.length]);

  const fallbackModel = useMemo(
    () =>
      resolveModelInfoSpec({
        modelId: activeModelId || 'hermes-agent',
        providerHint: activeProvider,
      }),
    [activeModelId, activeProvider]
  );

  const availableModels = useMemo(() => {
    if (modelInfo?.availableModels && modelInfo.availableModels.length > 0) {
      return modelInfo.availableModels;
    }
    return [fallbackModel];
  }, [modelInfo, fallbackModel]);

  const inspectedModel = useMemo(() => {
    if (selectedModelId) {
      const found = availableModels.find((m) => m.id === selectedModelId);
      if (found) return found;
    }
    return modelInfo?.activeModel ?? fallbackModel;
  }, [selectedModelId, availableModels, modelInfo, fallbackModel]);

  const usage = useMemo(
    () =>
      computeContextWindowUsage({
        model: inspectedModel,
        entries,
        skillsCount,
        toolsetsCount,
        compactionThresholdRatio: modelInfo?.compactionThresholdRatio ?? 0.8,
        // Only apply live provider usage when viewing the actual active model (not a what-if comparison model)
        lastReportedUsage:
          !selectedModelId || selectedModelId === modelInfo?.activeModel?.id
            ? modelInfo?.lastReportedUsage ?? null
            : null,
      }),
    [inspectedModel, entries, skillsCount, toolsetsCount, modelInfo, selectedModelId]
  );

  return {
    modelInfo,
    inspectedModel,
    availableModels,
    selectedModelId,
    setSelectedModelId,
    usage,
    loading,
    refreshing,
    refresh: fetchModelInfo,
  };
}
