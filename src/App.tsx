import { useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import * as settingsStore from "./lib/settings";
import {
  chars,
  normalize,
  onStatus,
  pauseTyping,
  resumeTyping,
  startTyping,
  stopTyping,
  updateTyping,
  type PauseReason,
  type Status,
} from "./lib/typer";
import "./App.css";

/** The part of the text being typed, as char positions in the whole text. */
interface Run {
  start: number;
  end: number;
}

const PAUSE_MESSAGES: Record<PauseReason, string> = {
  escape: "Paused: you pressed Esc.",
  click: "Paused: you clicked. Put the cursor back where typing should continue, then Resume.",
  focusLost: "Paused: you switched windows. Resume goes back to the same window.",
  requested: "Paused.",
  blocked:
    "Paused: the system blocked the key presses. The target app may be running as administrator, or the screen locked.",
};

function App() {
  const [s, setS] = useState(settingsStore.load);
  const [status, setStatus] = useState<Status>({ state: "idle" });
  const [run, setRun] = useState<Run | null>(null);
  const [error, setError] = useState<string | null>(null);
  const textArea = useRef<HTMLTextAreaElement>(null);

  const text = useMemo(() => normalize(s.text), [s.text]);
  const textChars = useMemo(() => chars(text), [text]);
  const set = (patch: Partial<settingsStore.Settings>) => setS((prev) => ({ ...prev, ...patch }));

  useEffect(() => settingsStore.save(s), [s]);

  useEffect(() => {
    getCurrentWindow().setAlwaysOnTop(s.alwaysOnTop).catch(() => {});
  }, [s.alwaysOnTop]);

  useEffect(() => {
    const unlisten = onStatus((st) => {
      setStatus(st);
      if (st.state === "done") {
        setS((prev) => ({ ...prev, target: prev.rememberTarget ? st.target : null }));
      }
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  const active = status.state === "waiting" || status.state === "typing" || status.state === "paused";
  const typoRate = s.typoPercent / 100;

  // Live changes to speed and typos apply from the next key.
  useEffect(() => {
    if (active) updateTyping(s.wpm, typoRate).catch(() => {});
  }, [s.wpm, typoRate]); // eslint-disable-line react-hooks/exhaustive-deps

  async function start() {
    setError(null);
    const el = textArea.current;
    // Highlighted text: type only that part.
    let startPos = 0;
    let endPos = textChars.length;
    if (el && el.selectionEnd > el.selectionStart) {
      startPos = chars(normalize(el.value.slice(0, el.selectionStart))).length;
      endPos = chars(normalize(el.value.slice(0, el.selectionEnd))).length;
    }
    const part = textChars.slice(startPos, endPos).join("");
    if (!part) {
      setError("Paste or type some text first.");
      return;
    }
    try {
      setRun({ start: startPos, end: endPos });
      setStatus({ state: "waiting" });
      await startTyping({
        text: part,
        config: { wpm: s.wpm, typoRate, shiftEnter: s.shiftEnter, codeMode: s.codeMode },
        pauseOnClick: s.pauseOnClick,
        remembered: s.rememberTarget ? s.target : null,
      });
    } catch (e) {
      setStatus({ state: "idle" });
      setError(String(e));
    }
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        if (status.state === "paused") void resumeTyping();
        else if (!active) void start();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const position =
    status.state === "typing" || status.state === "paused" || status.state === "stopped"
      ? status.position
      : status.state === "done" && run
        ? run.end - run.start
        : 0;
  const total = run ? run.end - run.start : 0;
  const measuredWpm = status.state === "typing" ? status.wpm : 0;
  const remaining = total - position;
  const etaSeconds = remaining > 0 ? (remaining / 5 / (measuredWpm > 5 ? measuredWpm : s.wpm)) * 60 : 0;

  return (
    <main className="app">
      <header className="header">
        <h1>Auto Typer</h1>
        <p className="status" role="status">
          {statusLine(status, error)}
        </p>
      </header>

      {run && status.state !== "idle" ? (
        <ProgressView chars={textChars} run={run} position={position} paused={status.state === "paused"} />
      ) : (
        <textarea
          ref={textArea}
          className="text"
          placeholder="Paste the text to type here. Highlight part of it to type only that part."
          value={s.text}
          onChange={(e) => set({ text: e.target.value })}
          spellCheck={false}
        />
      )}

      <section className="stats" aria-label="Progress">
        <progress max={Math.max(total, 1)} value={position} />
        <span>{total ? `${Math.round((position / total) * 100)}%` : "–"}</span>
        <span>{measuredWpm ? `${Math.round(measuredWpm)} wpm` : `${s.wpm} wpm target`}</span>
        <span>{active && etaSeconds ? `${formatTime(etaSeconds)} left` : formatTime(estimate(textChars.length, s.wpm))}</span>
      </section>

      <section className="controls">
        <label className="slider">
          <span>
            Speed <b>{s.wpm} wpm</b>
          </span>
          <input type="range" min={10} max={200} step={5} value={s.wpm} onChange={(e) => set({ wpm: +e.target.value })} />
        </label>
        <label className="slider">
          <span>
            Typos <b>{s.typoPercent}%</b>
          </span>
          <input
            type="range"
            min={0}
            max={10}
            step={0.5}
            value={s.typoPercent}
            onChange={(e) => set({ typoPercent: +e.target.value })}
          />
        </label>
      </section>

      <section className="options">
        <Toggle label="Shift+Enter for new lines" hint="Chat apps where Enter sends" checked={s.shiftEnter} onChange={(v) => set({ shiftEnter: v })} disabled={active} />
        <Toggle label="Code editor mode" hint="Keeps your indentation, drops auto-closed brackets" checked={s.codeMode} onChange={(v) => set({ codeMode: v })} disabled={active} />
        <Toggle label="Pause when I click" checked={s.pauseOnClick} onChange={(v) => set({ pauseOnClick: v })} disabled={active} />
        <Toggle label="Always on top" checked={s.alwaysOnTop} onChange={(v) => set({ alwaysOnTop: v })} />
        <Toggle label="Keep text after closing" hint="Turn off for anything private" checked={s.keepText} onChange={(v) => set({ keepText: v })} />
        <Toggle label="Remember target window" checked={s.rememberTarget} onChange={(v) => set({ rememberTarget: v, target: v ? s.target : null })} />
        {s.target && !active && (
          <button className="link" onClick={() => set({ target: null })}>
            Forget last window
          </button>
        )}
      </section>

      <footer className="actions">
        {!active && (
          <button className="primary" onClick={() => void start()}>
            Start typing <kbd>Ctrl+Enter</kbd>
          </button>
        )}
        {status.state === "typing" && (
          <button className="primary" onClick={() => void pauseTyping()}>
            Pause <kbd>Esc</kbd>
          </button>
        )}
        {status.state === "paused" && (
          <button className="primary resume" onClick={() => void resumeTyping()}>
            Resume
          </button>
        )}
        {active && <button onClick={() => void stopTyping()}>Stop</button>}
        {!active && run && (
          <button onClick={() => { setRun(null); setStatus({ state: "idle" }); }}>Edit text</button>
        )}
      </footer>
    </main>
  );
}

function ProgressView({ chars, run, position, paused }: { chars: string[]; run: Run; position: number; paused: boolean }) {
  const next = run.start + position;
  const done = next >= run.end;
  return (
    <div className="text progress-view" aria-live="off">
      <span className="outside">{chars.slice(0, run.start).join("")}</span>
      <span className="typed">{chars.slice(run.start, next).join("")}</span>
      {!done && <span className={paused ? "next paused" : "next"}>{chars[next] === "\n" ? "↵\n" : chars[next]}</span>}
      <span>{chars.slice(done ? next : next + 1, run.end).join("")}</span>
      <span className="outside">{chars.slice(run.end).join("")}</span>
    </div>
  );
}

function Toggle(props: { label: string; hint?: string; checked: boolean; disabled?: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="toggle" title={props.hint}>
      <input type="checkbox" checked={props.checked} disabled={props.disabled} onChange={(e) => props.onChange(e.target.checked)} />
      <span>{props.label}</span>
    </label>
  );
}

function statusLine(status: Status, error: string | null): string {
  if (error) return error;
  switch (status.state) {
    case "idle":
      return "Paste your text, then Start and click into the box where it should go.";
    case "waiting":
      return "Click into the window and box where the text should go…";
    case "typing":
      return "Typing… keep your hands off the keyboard. Esc pauses.";
    case "paused":
      return PAUSE_MESSAGES[status.reason];
    case "done":
      return "Done.";
    case "stopped":
      return "Stopped.";
    case "failed":
      return status.message;
  }
}

function estimate(chars: number, wpm: number): number {
  return (chars / 5 / wpm) * 60;
}

function formatTime(seconds: number): string {
  if (!seconds) return "";
  const m = Math.floor(seconds / 60);
  const sec = Math.round(seconds % 60);
  return m ? `${m}m ${sec.toString().padStart(2, "0")}s` : `${sec}s`;
}

export default App;
