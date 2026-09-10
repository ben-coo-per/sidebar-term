// A single, app-wide in-app confirm dialog (never window.confirm/alert). One <ConfirmDialog/>
// mounted in +page.svelte renders whatever request is current; requestConfirm() resolves once
// the user answers.

interface ConfirmRequest {
  message: string;
  detail?: string;
  confirmLabel?: string;
  danger?: boolean;
  resolve: (ok: boolean) => void;
}

export const confirmDialog = $state<{ current: ConfirmRequest | null }>({ current: null });

export function requestConfirm(opts: {
  message: string;
  detail?: string;
  confirmLabel?: string;
  danger?: boolean;
}): Promise<boolean> {
  return new Promise((resolve) => {
    confirmDialog.current = { ...opts, resolve };
  });
}

export function answerConfirm(ok: boolean): void {
  confirmDialog.current?.resolve(ok);
  confirmDialog.current = null;
}
