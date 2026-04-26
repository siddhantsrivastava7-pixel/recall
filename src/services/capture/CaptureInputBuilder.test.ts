import { describe, expect, it } from "vitest";

import { buildQuickCaptureInput } from "@/services/capture/CaptureInputBuilder";

describe("CaptureInputBuilder", () => {
  it("builds a quick capture payload with optional fields collapsed to null", () => {
    const input = buildQuickCaptureInput(
      {
        title: "  Launch notes  ",
        content: "  Keep this snippet exactly as pasted.  ",
        note: "   ",
        projectId: "",
      },
      {
        sourceApp: "Chrome",
        sourceWindow: "Pricing doc",
      },
    );

    expect(input).toEqual({
      sourceType: "manual",
      title: "Launch notes",
      content: "  Keep this snippet exactly as pasted.  ",
      note: null,
      projectId: null,
      sourceApp: "Chrome",
      sourceWindow: "Pricing doc",
    });
  });

  it("supports clipboard-style capture with inferred optional metadata handled later by the backend", () => {
    const input = buildQuickCaptureInput(
      {
        title: "",
        content: "  https://example.com/docs/render  ",
        note: " grabbed from clipboard ",
        projectId: "system-inbox",
      },
      {
        sourceApp: "Brave",
        sourceWindow: "Render Docs",
      },
    );

    expect(input.sourceType).toBe("manual");
    expect(input.title).toBeNull();
    expect(input.content).toBe("  https://example.com/docs/render  ");
    expect(input.note).toBe("grabbed from clipboard");
    expect(input.projectId).toBe("system-inbox");
    expect(input.sourceApp).toBe("Brave");
    expect(input.sourceWindow).toBe("Render Docs");
  });
});
