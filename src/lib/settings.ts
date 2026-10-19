// Settings and text kept between runs, in the webview's local storage.
import type { Target } from "./typer";

export interface Settings {
  wpm: number;
  typoPercent: number;
  shiftEnter: boolean;
  codeMode: boolean;
  pauseOnClick: boolean;
  alwaysOnTop: boolean;
  keepText: boolean;
  rememberTarget: boolean;
  target: Target | null;
  text: string;
}

export const defaults: Settings = {
  wpm: 60,
  typoPercent: 2,
  shiftEnter: false,
  codeMode: false,
  pauseOnClick: true,
  alwaysOnTop: false,
  keepText: true,
  rememberTarget: true,
  target: null,
  text: "",
};

const KEY = "auto-typer.settings";

export function load(): Settings {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    return { ...defaults, ...saved };
  } catch {
    return defaults;
  }
}

export function save(s: Settings) {
  try {
    localStorage.setItem(KEY, JSON.stringify(s.keepText ? s : { ...s, text: "" }));
  } catch {
    // Storage unavailable: settings just won't persist.
  }
}
