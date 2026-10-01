// A modal dialog, wrapping the native <dialog> element.
import { useEffect, useRef, type ReactNode } from 'react';

/** A modal dialog (the native `<dialog>`): open while `open` is true. */
export function Dialog({ open, onClose, children, wide = false }: { open: boolean; onClose: () => void; children: ReactNode; wide?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) d.showModal();
    if (!open && d.open) d.close();
  }, [open]);
  return (
    <dialog ref={ref} onClose={onClose} style={wide ? { width: 'min(720px, 94vw)' } : undefined}>
      {children}
    </dialog>
  );
}
