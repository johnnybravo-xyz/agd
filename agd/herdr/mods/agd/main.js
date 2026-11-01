import { View, div } from "gpui-kit";
import { h_flex, v_flex } from "gpui-base";
import { exec, run, theme, watchFolder } from "herdr";

// agd's Herdr panel: today as the TUI's pane shows it. It only runs the
// `agd` binary and draws what agd prints (`agd agenda --status --json`:
// the counts, the overdue rows and today's, each with agd's own lead and
// title), so agd stays the one reader and writer of the Org files. It
// watches the roots agd names there, as `agd agenda --status --watch` does
// (exec cannot stream that), so a change from anywhere shows at once.

// agd's stderr as the sentence its TUI shows: the agd.error/v1 line under
// --json, else the line after `error:`.
function errorSentence(stderr) {
  const lines = stderr.trim().split("\n");
  const json = lines.find((line) => line.startsWith("{"));
  if (json) {
    try {
      return JSON.parse(json).message || stderr;
    } catch {
      return stderr;
    }
  }
  const error = lines.find((line) => /^error:/i.test(line));
  return (error || stderr).replace(/^error:\s*/i, "");
}

// Runs agd; resolves its stdout, or throws its error as a sentence.
async function agd(args) {
  let result;
  try {
    result = await exec("agd", args);
  } catch (error) {
    throw new Error(`cannot run agd (is it on PATH?): ${error.message ?? error}`);
  }
  if (result.code !== 0) {
    throw new Error(errorSentence(result.stderr || result.stdout) || `agd ${args[0]} failed`);
  }
  return result.stdout;
}

export default class AgdAgenda extends View {
  init(_props, cx) {
    this.status = null;
    this.message = null;
    this.undoable = false;
    this.error = null;
    this.busy = false;
    this.stale = false;
    this.watching = false;
    this.refresh(cx);
    cx.timer.every(60000, (cx) => this.refresh(cx));
  }

  // Runs one agd action at a time, then reads the agenda again; a refresh
  // asked for meanwhile runs after it.
  act(cx, action) {
    if (this.busy) {
      this.stale = true;
      return;
    }
    this.busy = true;
    cx.notify();
    cx.spawn(async (cx) => {
      try {
        await action();
        this.status = JSON.parse(await agd(["agenda", "--status", "--json"]));
        this.error = null;
        this.watch(this.status.roots, cx);
      } catch (error) {
        this.error = String(error.message ?? error);
      } finally {
        this.busy = false;
        cx.notify();
        if (this.stale) {
          this.stale = false;
          this.refresh(cx);
        }
      }
    });
  }

  // Re-reads the agenda whenever an Org file under agd's roots changes;
  // started once, with the roots of the first answer.
  watch(roots, cx) {
    if (this.watching) return;
    this.watching = true;
    for (const root of roots) {
      cx.spawn(async (cx) => {
        for (;;) {
          const changed = await watchFolder(root);
          if (changed.some((path) => path.endsWith(".org"))) this.refresh(cx);
        }
      });
    }
  }

  refresh(cx) {
    this.act(cx, async () => {});
  }

  // Done, as `x` in the TUI: the first done state of the task's own
  // workflow. Says agd's summary, which `u` in agd or Undo takes back.
  complete(entry, cx) {
    this.act(cx, async () => {
      const result = JSON.parse(await agd(["edit", entry.item.ref, "--done", "--json"]));
      this.message = [result.summary, ...result.notes].join("\n");
      this.undoable = true;
    });
  }

  undo(cx) {
    this.act(cx, async () => {
      this.message = (await agd(["undo"])).trim();
      this.undoable = false;
    });
  }

  button(id, label, action, t) {
    return div().id(id).h(27).px(9).flex().items_center().rounded(7)
      .border_1().border_color(t.border).text_size(11.5).text_color(t.muted)
      .cursor_pointer().hover((style) => style.bg(t.hover).text_color(t.foreground))
      .on_click(action).child(label);
  }

  // An uppercase header with its count, as the TUI's pane heads a section.
  header(label, count, color, t) {
    return h_flex().gap(7).px(4).pt(6).text_size(11).font_semibold()
      .child(div().text_color(count > 0 ? color : t.dim).child(label))
      .child(div().text_color(t.dim).child(String(count)));
  }

  // One row as the TUI's pane draws it: agd's lead, the title, and at the
  // right the priority, ↻ for a repeating task or ✓ for done.
  row(entry, index, t) {
    const { item, occurrence, display } = entry;
    const late = occurrence.late;
    const due = occurrence.type === "deadline" || occurrence.type === "upcoming-deadline";
    const leadColor = late ? t.blocked : due ? t.working : item.done ? t.dim : t.accent;
    const planning = item.planning ?? {};
    const repeats = [planning.scheduled, planning.deadline].some((timestamp) => timestamp?.repeater);
    const mark = item.done ? "✓" : item.priority ?? (repeats ? "↻" : "");
    let title = div().flex_1().min_w(0).truncate().child(display.title);
    title = item.done ? title.line_through().text_color(t.dim) : title.text_color(t.foreground);
    return h_flex().id(`agenda-${index}`).w_full().min_h(36).flex_none().px(10).gap(9)
      .items_center().rounded(8).bg(t.surface)
      .child(div().w(44).flex_none().text_size(11.5).text_color(leadColor).child(display.lead))
      .child(title)
      .when(Boolean(mark), (el) => el.child(div().flex_none().text_size(11).text_color(item.done ? t.done : t.muted).child(mark)))
      .when(Boolean(item.todo) && !item.done, (el) => el.child(this.button(
        `agd-done-${index}`, "Done", (_event, cx) => this.complete(entry, cx), t,
      )));
  }

  render() {
    const t = theme();
    const status = this.status;
    const overdue = status?.overdue_entries ?? [];
    const today = status?.today_entries ?? [];
    const rows = [];
    if (status) {
      if (overdue.length > 0) {
        rows.push(this.header("OVERDUE", status.overdue, t.blocked, t));
        overdue.forEach((entry, index) => rows.push(this.row(entry, `o${index}`, t)));
      }
      rows.push(this.header("TODAY", status.today, t.accent, t));
      today.forEach((entry, index) => rows.push(this.row(entry, `t${index}`, t)));
      if (overdue.length + today.length === 0) {
        rows.push(div().p(10).text_color(t.dim).child("All clear for today."));
      }
    }
    return v_flex().id("agd-agenda").size_full().min_h(0).rounded(10).overflow_hidden()
      .bg(t.background).border_1().border_color(t.border).text_color(t.foreground).text_size(13)
      .child(h_flex().h(44).flex_none().px(12).gap(7).items_center().border_b(1).border_color(t.border)
        .child(div().font_semibold().child("agd"))
        .when(this.busy, (el) => el.child(div().text_size(10.5).text_color(t.dim).child("Updating…")))
        .child(div().flex_1())
        .child(this.button("agd-refresh", "Refresh", (_event, cx) => this.refresh(cx), t))
        .child(this.button("agd-open", "Open", () => run("terminal.open", { argv: ["agd"], name: "agd" }), t))
        .child(this.button("agd-capture", "+", () => run("terminal.open", { argv: ["agd", "capture", "-i"], name: "Capture" }), t)))
      .child(v_flex().id("agd-list").flex_1().min_h(0).overflow_y_scrollbar().p(7).gap(5).children(rows))
      .when(Boolean(this.message), (el) => el.child(h_flex().flex_none().px(12).py(8).gap(9).items_center()
        .border_t(1).border_color(t.border).text_size(11)
        .child(div().flex_1().min_w(0).text_color(t.muted).child(this.message))
        .when(this.undoable, (el) => el.child(this.button("agd-undo", "Undo", (_event, cx) => this.undo(cx), t)))))
      .when(Boolean(this.error), (el) => el.child(div().flex_none().px(12).py(8).border_t(1)
        .border_color(t.border).text_size(11).text_color(t.blocked).child(this.error)));
  }
}
