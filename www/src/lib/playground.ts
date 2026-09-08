/**
 * /try contract: homepage sample, host limits, and copy that must not
 * claim `xo run` or native LLVM in the browser.
 */

import { homePage } from "../docs/site";

/** Sum buffer on /try. Same program as the homepage `sum.echo` figure. */
export function playgroundSumSource(): string {
  return homePage.sample.endsWith("\n") ? homePage.sample : `${homePage.sample}\n`;
}

/** Host services the playground refuses. Keep this list on the page. */
export const PLAYGROUND_HOST_LIMITS = ["filesystem", "net", "process", "tasks"] as const;
