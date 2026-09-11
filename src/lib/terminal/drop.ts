// Files dragged onto a Terminal are pasted as shell-quoted paths, like Terminal.app and iTerm2, so
// e.g. Claude Code picks up a dropped image. `dragDropEnabled` is off (Tauri's handler swallows
// the sidebar's HTML5 drags on macOS), so drops arrive here as DOM Files without paths; the paths
// come from the drag pasteboard, and files that have none (or only an unreadable staging copy, as
// the screenshot thumbnail's) are saved to a temp dir first.
// See src-tauri/src/drop.rs.

import { dropPaths, saveDroppedFile } from "../ipc";

export function carriesFiles(e: DragEvent): boolean {
  return !!e.dataTransfer && Array.from(e.dataTransfer.types).includes("Files");
}

/**
 * Backslash-escape ASCII shell metacharacters, as Terminal.app does for dropped paths. Non-ASCII
 * (e.g. the U+202F before "AM" in screenshot names) is left alone: the shell doesn't split on it.
 */
export function shellEscape(path: string): string {
  return path.replace(/[ \t\n\r!"#$&'()*,;<=>?[\\\]^`{|}~]/g, "\\$&");
}

const basename = (p: string) => p.slice(p.lastIndexOf("/") + 1);

/**
 * Paths for the dropped `files`, in order. Call right after the `drop` event, while the drag
 * pasteboard still holds this drop. Pasteboard paths are matched by name so a stale pasteboard
 * never supplies a wrong path; unmatched files are saved and their temp path used.
 */
export async function resolveDroppedPaths(files: File[]): Promise<string[]> {
  const onPasteboard = await dropPaths().catch(() => [] as string[]);
  if (files.length === 0) return onPasteboard;
  const paths = await Promise.all(
    files.map(async (file) => {
      const i = onPasteboard.findIndex((p) => basename(p) === file.name);
      if (i !== -1) return onPasteboard.splice(i, 1)[0];
      return saveDroppedFile(file).catch((err) => {
        console.error("[terminal] could not save dropped file", file.name, err);
        return null;
      });
    }),
  );
  return paths.filter((p): p is string => !!p);
}

/**
 * Stop a file drop anywhere outside a Terminal from navigating the webview to the file (WebKit's
 * default). Terminal panes accept the drop first and mark the event handled.
 */
export function initDropGuard(): () => void {
  const onDragOver = (e: DragEvent) => {
    if (e.defaultPrevented || !carriesFiles(e)) return;
    e.preventDefault();
    e.dataTransfer!.dropEffect = "none";
  };
  const onDrop = (e: DragEvent) => {
    if (carriesFiles(e)) e.preventDefault();
  };
  window.addEventListener("dragover", onDragOver);
  window.addEventListener("drop", onDrop);
  return () => {
    window.removeEventListener("dragover", onDragOver);
    window.removeEventListener("drop", onDrop);
  };
}
