import { browser, $ } from "@wdio/globals";

/** The window's whole visible text, which is how these specs assert on state. */
export const bodyText = async () => (await $("body")).getText();

/**
 * Click by visible text in one DOM step.
 *
 * The rail swaps the whole surface, so a handle found a moment earlier can be unmounted before
 * the click lands. Querying and clicking in the same script is stable, and the click is still a
 * real click on the real button, so React handles it exactly as it would a mouse click.
 */
async function clickByText(needle: string): Promise<boolean> {
  return (await browser.execute((text: string) => {
    const button = Array.from(document.querySelectorAll("button")).find((candidate) =>
      (candidate.textContent ?? "").includes(text),
    );
    if (!button) return false;
    button.click();
    return true;
  }, needle)) as boolean;
}

export async function openRail(label: string) {
  const rail = await $('nav[aria-label="Primary"]');
  await rail.waitForDisplayed({ timeout: 40_000 });
  const opened = (await browser.execute((name: string) => {
    const button = document.querySelector<HTMLButtonElement>(
      `nav[aria-label="Primary"] button[aria-label="${name}"]`,
    );
    if (!button) return false;
    button.click();
    return true;
  }, label)) as boolean;
  if (!opened) throw new Error(`the rail has no ${label} button`);
}

/**
 * Start a session from whichever affordance the surface shows: a project with no session offers
 * "Start a session", one that already has sessions offers the "+ session" button.
 */
export async function startSession() {
  await openRail("Session");
  if (!(await clickByText("Start a session")) && !(await clickByText("+ session"))) {
    throw new Error("the session surface offered no way to start a session");
  }
  const composer = await $('textarea[placeholder*="Ask"]');
  await composer.waitForDisplayed({ timeout: 30_000 });
}

export async function sendPrompt(text: string) {
  const composer = await $('textarea[placeholder*="Ask"]');
  await composer.waitForDisplayed({ timeout: 20_000 });
  await composer.setValue(text);
  if (!(await clickByText("Send"))) throw new Error("the composer has no Send button");
}

/** The approval card, whether or not the current turn asked anything. */
export async function approvalCard() {
  return $('section[aria-label="Approval required"]');
}

export async function waitForText(needle: string, timeout = 40_000) {
  await browser.waitUntil(async () => (await bodyText()).includes(needle), {
    timeout,
    timeoutMsg: `the window never showed ${JSON.stringify(needle)}`,
  });
}

/**
 * Approve every card the turn raises: a turn that runs a command and then writes a file asks
 * once for each, and each answer resumes the same turn.
 */
export async function approveEveryCard(limit = 4) {
  for (let seen = 0; seen < limit; seen += 1) {
    const card = await approvalCard();
    // `sendPrompt` returns before the coordinator has necessarily raised its first card. Wait for
    // either the next card or the end of the turn; checking once races a fast scripted reply.
    let raised = false;
    await browser.waitUntil(
      async () => {
        if (await card.isExisting()) {
          raised = true;
          return true;
        }
        return (await bodyText()).includes("Done.");
      },
      { timeout: 30_000, interval: 150, timeoutMsg: "the turn raised no card and did not finish" },
    );
    if (!raised) return seen;
    await card.waitForDisplayed({ timeout: 20_000 });
    await (await card.$("button=Allow once")).click();
    await browser.pause(400);
  }
  return limit;
}
