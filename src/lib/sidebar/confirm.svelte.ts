// A single, app-wide in-app confirm dialog (never window.confirm/alert). One <ConfirmDialog/>
// mounted in +page.svelte renders whatever request is current; requestConfirm() resolves once
// the user answers. requestChoice() adds a third, alternative button (Handoff's "Type git push
// first"); `detail` may hold several lines. requestNotice() only informs: one button, no Cancel.

export type Choice = "confirm" | "alternative" | "cancel";

interface ConfirmRequest {
  message: string;
  detail?: string;
  confirmLabel?: string;
  /** A third button between Cancel and the confirmation; none when unset. */
  alternativeLabel?: string;
  /** Only informs: no Cancel button (`requestNotice`). */
  notice?: boolean;
  danger?: boolean;
  resolve: (choice: Choice) => void;
}

export const confirmDialog = $state<{ current: ConfirmRequest | null }>({ current: null });

export function requestChoice(opts: {
  message: string;
  detail?: string;
  confirmLabel?: string;
  alternativeLabel?: string;
  danger?: boolean;
}): Promise<Choice> {
  return new Promise((resolve) => {
    confirmDialog.current = { ...opts, resolve };
  });
}

export async function requestConfirm(opts: {
  message: string;
  detail?: string;
  confirmLabel?: string;
  danger?: boolean;
}): Promise<boolean> {
  return (await requestChoice(opts)) === "confirm";
}

export function answerConfirm(choice: Choice | boolean): void {
  const c: Choice = choice === true ? "confirm" : choice === false ? "cancel" : choice;
  confirmDialog.current?.resolve(c);
  confirmDialog.current = null;
}

/** Tell the user something that needs no decision: one button, resolved when dismissed. */
export async function requestNotice(opts: { message: string; detail?: string; label?: string }): Promise<void> {
  await new Promise<Choice>((resolve) => {
    confirmDialog.current = { message: opts.message, detail: opts.detail, confirmLabel: opts.label ?? "OK", notice: true, resolve };
  });
}
