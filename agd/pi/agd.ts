// agd's Pi extension: "/capture t buy milk due:fri" files a task through an
// agd capture template at once, without a model turn, as Claude Code's
// /capture does. `agd setup install pi` writes this file to
// ~/.pi/agent/extensions/agd.ts and `agd setup install omp` to
// ~/.omp/agent/extensions/agd.ts; oh-my-pi keeps Pi's extension API, and
// both load TypeScript without a build step.
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  pi.registerCommand("capture", {
    description: "File a task through an agd capture template: KEY text  due:fri on:+2d",
    handler: async (args, ctx) => {
      const match = args.trim().match(/^(\S+)\s+(.+)$/);
      if (!match) {
        ctx.ui.notify("usage: /capture KEY text, such as /capture t buy milk due:fri", "warning");
        return;
      }
      // After `--`, a text such as `-1 point` is text, not an option. Pi runs
      // agd without a shell, so the text is one argument as typed.
      const result = await pi.exec("agd", ["capture", "--json", "--", match[1], match[2]], {
        timeout: 15000,
      });
      if (result.code !== 0) {
        ctx.ui.notify(errorSentence(result.stderr) || "agd capture failed; is agd on your PATH?", "error");
        return;
      }
      // `summary` is the line agd's terminal shows:
      // `inbox.org:7  captured · buy milk · deadline Fri 23 Oct`.
      ctx.ui.notify(JSON.parse(result.stdout).summary, "info");
    },
  });
}

// With --json, agd writes its error as an agd.error/v1 line; its `message`
// is the sentence to show. Warnings may come before it.
function errorSentence(stderr: string): string {
  const json = stderr.split("\n").find((line) => line.startsWith("{"));
  if (!json) return stderr.trim().replace(/^error:\s*/i, "");
  try {
    return JSON.parse(json).message;
  } catch {
    return json;
  }
}
