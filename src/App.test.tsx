import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import App from "./App";

afterEach(() => {
  cleanup();
  localStorage.clear();
});

// jsdom has no __TAURI_INTERNALS__, just like a normal browser tab on the dev server.
describe("App outside the Tauri webview", () => {
  it("renders instead of crashing", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "Auto Typer" })).toBeTruthy();
    expect(screen.getByRole("textbox")).toBeTruthy();
  });

  it("explains that typing needs the desktop app and disables Start", () => {
    render(<App />);
    expect(screen.getByRole("status").textContent).toMatch(/desktop app/i);
    const start = screen.getByRole("button", { name: /start typing/i }) as HTMLButtonElement;
    expect(start.disabled).toBe(true);
  });
});
