// Bridge to the Rust typist: commands out, status events in.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * True inside the Tauri webview. False in a normal browser tab on the dev server (or in tests),
 * where every Tauri API throws, some of them synchronously before a promise exists.
 */
export const inDesktopApp = (): boolean => isTauri();

export interface PlanConfig {
  wpm: number;
  typoRate: number;
  shiftEnter: boolean;
  codeMode: boolean;
}

export interface Target {
  id: number;
  pid: number;
}

export type PauseReason = "escape" | "click" | "focusLost" | "requested" | "blocked";

export type Status =
  | { state: "idle" }
  | { state: "waiting" }
  | { state: "typing"; position: number; total: number; wpm: number }
  | { state: "paused"; position: number; reason: PauseReason }
  | { state: "done"; target: Target }
  | { state: "stopped"; position: number }
  | { state: "failed"; message: string };

export function startTyping(request: {
  text: string;
  config: PlanConfig;
  pauseOnClick: boolean;
  remembered: Target | null;
}): Promise<void> {
  return invoke("start_typing", { request });
}

export const pauseTyping = () => invoke<void>("pause_typing");
export const resumeTyping = () => invoke<void>("resume_typing");
export const stopTyping = () => invoke<void>("stop_typing");
export const updateTyping = (wpm: number, typoRate: number) =>
  invoke<void>("update_typing", { wpm, typoRate });

export function onStatus(handler: (s: Status) => void): Promise<UnlistenFn> {
  return listen<Status>("typer-status", (e) => handler(e.payload));
}

/** Same line endings and char positions as the Rust plan (Unicode scalar values). */
export function normalize(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

export function chars(text: string): string[] {
  return Array.from(text);
}
