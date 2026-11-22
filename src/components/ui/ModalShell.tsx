import { useEffect, type ReactNode } from 'react';

interface ModalShellProps {
  open: boolean;
  onClose: () => void;
  ariaLabel?: string;
  ariaLabelledBy?: string;
  testId?: string;
  maxWidthClass?: string;
  zIndexClass?: string;
  children: ReactNode;
}

/**
 * Reusable accessible modal dialog shell with backdrop blur, Escape dismissal,
 * and speed-scaled spring entrance choreography.
 */
export function ModalShell({
  open,
  onClose,
  ariaLabel,
  ariaLabelledBy,
  testId,
  maxWidthClass = 'w-[min(780px,96vw)]',
  zIndexClass = 'z-50',
  children,
}: ModalShellProps) {
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className={`ui-backdrop-enter fixed inset-0 ${zIndexClass} flex items-end sm:items-center justify-center p-0 sm:p-6 bg-[#05070B]/82`}
      role="presentation"
      onClick={onClose}
    >
      <section
        role="dialog"
        aria-modal="true"
        aria-label={ariaLabel}
        aria-labelledby={ariaLabelledBy}
        data-testid={testId}
        onClick={(e) => e.stopPropagation()}
        className={`ui-drawer-shell ui-modal-enter ${maxWidthClass} max-h-[90dvh] sm:max-h-[min(86vh,720px)] rounded-t-2xl sm:rounded-2xl border shadow-[0_24px_80px_rgba(0,0,0,0.74)] flex flex-col overflow-hidden text-[#ECF3F0]`}
      >
        {children}
      </section>
    </div>
  );
}
