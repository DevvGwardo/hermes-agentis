import { useEffect, useMemo, useRef, useState } from 'react';
import {
  CornerDownLeft,
  Home,
  LayoutGrid,
  ListChecks,
  MessageSquare,
  PanelLeft,
  Plus,
  RefreshCw,
  Search,
  Settings,
  Terminal,
  Volume2,
  VolumeX,
  X,
} from 'lucide-react';
import {
  filterQuickJumpItems,
  stepQuickJumpIndex,
  type QuickJumpCategory,
  type QuickJumpItem,
} from '../utils/navigationModel';
import { ModalShell } from './ui';

interface QuickJumpModalProps {
  open: boolean;
  onClose: () => void;
  items: QuickJumpItem[];
  onExecute: (item: QuickJumpItem) => void;
  isMuted: boolean;
}

const CATEGORY_LABELS: Record<QuickJumpCategory, string> = {
  views: 'Workspace views',
  sessions: 'Sessions',
  actions: 'Quick actions',
};

const CATEGORY_ORDER: QuickJumpCategory[] = ['views', 'sessions', 'actions'];

function renderItemIcon(item: QuickJumpItem, isMuted: boolean) {
  if (item.action.type === 'view') {
    switch (item.action.view) {
      case 'home':
        return <Home className="w-3.5 h-3.5 text-[var(--ui-accent-soft)] shrink-0" />;
      case 'canvas':
        return <LayoutGrid className="w-3.5 h-3.5 text-[var(--ui-accent-soft)] shrink-0" />;
      case 'voice':
        return <MessageSquare className="w-3.5 h-3.5 text-[var(--ui-accent-soft)] shrink-0" />;
      case 'queue':
        return <ListChecks className="w-3.5 h-3.5 text-[var(--ui-accent-soft)] shrink-0" />;
      case 'cli':
        return <Terminal className="w-3.5 h-3.5 text-[var(--ui-accent-soft)] shrink-0" />;
    }
  }
  if (item.action.type === 'session') {
    return <MessageSquare className="w-3.5 h-3.5 text-zinc-400 shrink-0" />;
  }
  switch (item.action.command) {
    case 'new-session':
      return <Plus className="w-3.5 h-3.5 text-[var(--ui-accent-soft)] shrink-0" />;
    case 'toggle-sidebar':
      return <PanelLeft className="w-3.5 h-3.5 text-zinc-300 shrink-0" />;
    case 'open-settings':
      return <Settings className="w-3.5 h-3.5 text-zinc-300 shrink-0" />;
    case 'toggle-mute':
      return isMuted ? (
        <Volume2 className="w-3.5 h-3.5 text-zinc-300 shrink-0" />
      ) : (
        <VolumeX className="w-3.5 h-3.5 text-zinc-300 shrink-0" />
      );
    case 'retry-gateway':
      return <RefreshCw className="w-3.5 h-3.5 text-amber-300 shrink-0" />;
  }
}

export function QuickJumpModal({
  open,
  onClose,
  items,
  onExecute,
  isMuted,
}: QuickJumpModalProps) {
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const filtered = useMemo(() => filterQuickJumpItems(items, query), [items, query]);

  useEffect(() => {
    if (!open) {
      setQuery('');
      setSelectedIndex(0);
      return;
    }
    setSelectedIndex(0);
    const id = window.setTimeout(() => {
      inputRef.current?.focus();
    }, 15);
    return () => window.clearTimeout(id);
  }, [open]);

  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  const grouped = useMemo(() => {
    const groups: Array<{ category: QuickJumpCategory; entries: Array<{ item: QuickJumpItem; flatIndex: number }> }> = [];
    for (const category of CATEGORY_ORDER) {
      const entries: Array<{ item: QuickJumpItem; flatIndex: number }> = [];
      filtered.forEach((item, flatIndex) => {
        if (item.category === category) {
          entries.push({ item, flatIndex });
        }
      });
      if (entries.length > 0) {
        groups.push({ category, entries });
      }
    }
    return groups;
  }, [filtered]);

  const handleSelect = (item: QuickJumpItem) => {
    onClose();
    onExecute(item);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSelectedIndex((prev) => stepQuickJumpIndex(prev, filtered.length, 'next'));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSelectedIndex((prev) => stepQuickJumpIndex(prev, filtered.length, 'prev'));
    } else if (e.key === 'Enter' && filtered.length > 0) {
      e.preventDefault();
      const target = filtered[Math.min(selectedIndex, filtered.length - 1)];
      if (target) handleSelect(target);
    }
  };

  return (
    <ModalShell
      open={open}
      onClose={onClose}
      ariaLabel="Quick Jump Navigation"
      testId="quick-jump-modal"
      maxWidthClass="w-[min(560px,94vw)]"
    >
      {/* Search Bar */}
      <div className="flex items-center gap-2.5 px-4 py-3 border-b border-white/10 shrink-0">
        <Search className="w-4 h-4 text-zinc-400 shrink-0" aria-hidden="true" />
        <label htmlFor="quick-jump-search" className="sr-only">
          Jump to a view, session, or action
        </label>
        <input
          id="quick-jump-search"
          ref={inputRef}
          data-testid="quick-jump-input"
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Jump to view, session, or action…"
          className="flex-1 min-w-0 bg-transparent text-sm text-white placeholder:text-zinc-500 focus:outline-none"
        />
        {query && (
          <button
            type="button"
            onClick={() => setQuery('')}
            aria-label="Clear quick jump search"
            className="p-1 rounded-md text-zinc-400 hover:text-white hover:bg-white/[0.06] cursor-pointer"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        )}
        <button
          type="button"
          onClick={onClose}
          aria-label="Close Quick Jump"
          className="font-mono text-[10.5px] text-zinc-400 hover:text-white px-1.5 py-0.5 rounded border border-white/10 bg-white/[0.03] cursor-pointer shrink-0"
        >
          Esc
        </button>
      </div>

      {/* Results List */}
      <div
        role="listbox"
        aria-label="Navigation destinations"
        className="flex-1 min-h-0 max-h-[min(60vh,420px)] overflow-y-auto cyber-scroll p-2 space-y-3"
      >
        {filtered.length === 0 ? (
          <div className="py-10 px-4 text-center space-y-1">
            <p className="text-xs font-medium text-zinc-200">No matching destinations</p>
            <p className="text-[11px] text-zinc-500">
              Try &ldquo;canvas&rdquo;, &ldquo;cli&rdquo;, &ldquo;settings&rdquo;, or a session name.
            </p>
          </div>
        ) : (
          grouped.map((group) => (
            <div key={group.category} className="space-y-0.5">
              <div className="px-2.5 py-1 text-[11px] font-medium text-zinc-400">
                {CATEGORY_LABELS[group.category]}
              </div>
              {group.entries.map(({ item, flatIndex }) => {
                const isSelected = flatIndex === selectedIndex;
                return (
                  <button
                    key={item.id}
                    type="button"
                    role="option"
                    aria-selected={isSelected}
                    data-testid={`quick-jump-item-${item.id}`}
                    onMouseEnter={() => setSelectedIndex(flatIndex)}
                    onClick={() => handleSelect(item)}
                    className={`w-full text-left px-2.5 py-2 rounded-xl flex items-center justify-between gap-3 transition-colors cursor-pointer ${
                      isSelected
                        ? 'bg-[rgba(var(--ui-accent-rgb),0.16)] text-white'
                        : 'text-zinc-200 hover:bg-white/[0.04]'
                    }`}
                  >
                    <div className="flex items-center gap-2.5 min-w-0 flex-1">
                      <span className="w-7 h-7 rounded-lg bg-white/[0.04] border border-white/[0.08] flex items-center justify-center shrink-0">
                        {renderItemIcon(item, isMuted)}
                      </span>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2 min-w-0">
                          <span className="text-xs font-medium text-white truncate">
                            {item.title}
                          </span>
                          {item.active && (
                            <span className="font-mono text-[10px] text-[var(--ui-accent-soft)] shrink-0">
                              current
                            </span>
                          )}
                          {item.badge && (
                            <span className="font-mono text-[10px] text-amber-300/90 shrink-0 tabular-nums">
                              · {item.badge}
                            </span>
                          )}
                        </div>
                        <p className="text-[11px] text-zinc-400 truncate mt-0.5">
                          {item.subtitle}
                        </p>
                      </div>
                    </div>

                    <div className="flex items-center gap-1.5 shrink-0">
                      {item.shortcut && (
                        <kbd className="font-mono text-[10px] text-zinc-400 px-1.5 py-0.5 rounded border border-white/10 bg-white/[0.03] whitespace-nowrap">
                          {item.shortcut}
                        </kbd>
                      )}
                      {isSelected && (
                        <CornerDownLeft className="w-3 h-3 text-[var(--ui-accent-soft)]" aria-hidden="true" />
                      )}
                    </div>
                  </button>
                );
              })}
            </div>
          ))
        )}
      </div>

      {/* Concise Keyboard Guide Footer */}
      <div className="px-4 py-2 border-t border-white/[0.08] bg-black/20 flex items-center justify-between gap-2 font-mono text-[10.5px] text-zinc-400 shrink-0">
        <span>↑↓ navigate · Enter open</span>
        <span className="hidden sm:inline">Alt+1–5 switch views · ⌘B sidebar</span>
      </div>
    </ModalShell>
  );
}
