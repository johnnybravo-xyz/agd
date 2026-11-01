// agd's Pi extension: "/capture t buy milk due:fri" files a task through an
// agd capture template at once, without a model turn, as Claude Code's
// /capture does. `agd setup install pi` writes this file to
// ~/.pi/agent/extensions/agd.ts and `agd setup install omp` to
// ~/.omp/agent/extensions/agd.ts; oh-my-pi keeps Pi's extension API, and
// both load TypeScript without a build step.
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  pi.registerCommand("capture", {
    description: "File a task through an agd capture template: [KEY] text  due:fri on:+2d",
    handler: async (args, ctx) => {
      const text = args.trim();
      if (!text) {
        ctx.ui.notify("usage: /capture [KEY] text, such as /capture t buy milk due:fri", "warning");
        return;
      }
      // agd picks the template as on a terminal: the first word when it
      // names one, else the first template. After `--`, a text such as
      // `-1 point` is text, not an option; Pi runs agd without a shell.
      const result = await pi.exec("agd", ["capture", "--json", "--", text], { timeout: 15000 });
      if (result.code !== 0) {
        ctx.ui.notify(errorSentence(result.stderr) || "agd capture failed; is agd on your PATH?", "error");
        return;
      }
      // agd's own `summary` and `notes`, as every surface says them.
      const captured = JSON.parse(result.stdout);
      ctx.ui.notify([`${captured.summary} · agd undo`, ...captured.notes].join("\n"), "info");
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
