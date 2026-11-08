import { browser, expect } from "@wdio/globals";

/**
 * Item 1's second spec: a renamed IPC command must fail by name.
 *
 * Tauri answers an unknown command with the name it was given, so invoking one that does not
 * exist is the same failure a rename produces — and the assertion is on the message, not on a
 * timeout, so a silently missing bridge cannot pass.
 */
describe("the IPC bridge", () => {
  it("fails a renamed command by name", async () => {
    const result = await browser.execute(async () => {
      const bridge = (window as unknown as {
        __TAURI_INTERNALS__?: { invoke: (command: string, args?: unknown) => Promise<unknown> };
      }).__TAURI_INTERNALS__;
      if (!bridge) return { bridged: false, message: "no __TAURI_INTERNALS__" };
      try {
        await bridge.invoke("turn_send_renamed", {});
        return { bridged: true, message: "the renamed command resolved" };
      } catch (error) {
        return { bridged: true, message: String(error) };
      }
    });

    expect(result.bridged).toBe(true);
    expect(result.message).toContain("turn_send_renamed");
  });
});
