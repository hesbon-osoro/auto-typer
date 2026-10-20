import { afterEach, describe, expect, it } from "vitest";
import { defaults, load, save } from "./settings";

afterEach(() => localStorage.clear());

describe("settings", () => {
  it("falls back to defaults when nothing is saved", () => {
    expect(load()).toEqual(defaults);
  });

  it("round-trips saved settings", () => {
    save({ ...defaults, wpm: 95, codeMode: true, text: "hello" });
    expect(load()).toMatchObject({ wpm: 95, codeMode: true, text: "hello" });
  });

  it("drops the text when keepText is off", () => {
    save({ ...defaults, keepText: false, text: "private" });
    expect(load().text).toBe("");
  });

  it("ignores corrupt storage", () => {
    localStorage.setItem("auto-typer.settings", "{not json");
    expect(load()).toEqual(defaults);
  });
});
