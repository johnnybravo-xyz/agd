import { View, div } from "gpui-kit";
import { h_flex, v_flex } from "gpui-base";
import { exec, run, theme } from "herdr";

function timing(entry) {
  const occurrence = entry.occurrence ?? {};
  if (occurrence.late) {
    const days = Math.max(1, Number(occurrence.days) || 1);
    return `${days}d overdue`;
  }
  if (occurrence.time) {
    return occurrence.end_time
      ? `${occurrence.time}–${occurrence.end_time}`
      : occurrence.time;
  }
  const extra = String(occurrence.extra ?? "").trim().replace(/:$/, "");
  if (extra) return extra;
  return {
    deadline: "Due today",
    "upcoming-deadline": "Due soon",
    scheduled: "Scheduled",
    "past-scheduled": "Overdue",
    timestamp: "Today",
    block: "Today",
    todo: "Today",
  }[occurrence.type] ?? "Today";
}

export default class AgdAgenda extends View {
  init(_props, cx) {
    this.entries = [];
    this.error = null;
    this.loading = false;
    this.refresh(cx);
    cx.timer.every(60000, (cx) => this.refresh(cx));
  }

  refresh(cx) {
    if (this.loading) return;
    this.loading = true;
    cx.spawn(async (cx) => {
      try {
        const result = await exec("agd", ["agenda", "--span", "day", "--json"]);
        if (result.code !== 0) {
          throw new Error((result.stderr || result.stdout).trim() || "agd agenda failed");
        }
        const entries = JSON.parse(result.stdout);
        if (!Array.isArray(entries)) throw new Error("agd returned an unexpected agenda response");
        this.entries = entries.filter((entry) => !entry.item?.done);
        this.error = null;
      } catch (error) {
        this.error = String(error).replace(/^Error:\s*/, "");
      } finally {
        this.loading = false;
        cx.notify();
      }
    });
  }

  button(id, label, action, t) {
    return div().id(id).h(27).px(9).flex().items_center().rounded(7)
      .border_1().border_color(t.border).text_size(11.5).text_color(t.muted)
      .cursor_pointer().hover((style) => style.bg(t.hover).text_color(t.foreground))
      .on_click(action).child(label);
  }

  row(entry, index, t) {
    const item = entry.item ?? {};
    const late = Boolean(entry.occurrence?.late);
    const tags = Array.isArray(item.tags) ? item.tags : [];
    const detail = [item.category, ...tags.map((tag) => `#${tag}`)].filter(Boolean).join(" · ");
    return h_flex().id(`agenda-${index}`).w_full().min_h(52).flex_none().px(10).py(8).gap(9)
      .rounded(8).bg(t.surface)
      .child(div().mt(5).size(7).flex_none().rounded(4).bg(late ? t.blocked : t.accent))
      .child(v_flex().flex_1().min_w(0).gap(3)
        .child(div().truncate().text_color(t.foreground).child(item.title ?? "Untitled task"))
        .child(h_flex().gap(7).text_size(11).text_color(t.dim)
          .child(div().flex_none().text_color(late ? t.blocked : t.muted).child(timing(entry)))
          .when(Boolean(detail), (el) => el.child(div().truncate().child(detail)))))
      .when(Boolean(item.priority), (el) => el.child(div().flex_none().text_size(10.5).text_color(t.muted).child(`!${item.priority}`)));
  }

  render() {
    const t = theme();
    const overdue = this.entries.filter((entry) => entry.occurrence?.late).length;
    return v_flex().id("agd-agenda").size_full().min_h(0).rounded(10).overflow_hidden()
      .bg(t.background).border_1().border_color(t.border).text_color(t.foreground).text_size(13)
      .child(h_flex().h(44).flex_none().px(12).gap(7).items_center().border_b(1).border_color(t.border)
        .child(div().font_semibold().child("Agenda"))
        .when(this.loading, (el) => el.child(div().text_size(10.5).text_color(t.dim).child("Updating…")))
        .child(div().flex_1())
        .child(this.button("agd-refresh", "Refresh", (_event, cx) => { this.refresh(cx); cx.notify(); }, t))
        .child(this.button("agd-open", "Open", () => run("terminal.open", { argv: ["agd"], name: "Agenda" }), t))
        .child(this.button("agd-capture", "+", () => run("terminal.open", { argv: ["agd", "capture", "-i"], name: "Capture" }), t)))
      .child(h_flex().h(34).flex_none().px(12).gap(10).items_center().border_b(1).border_color(t.border).text_size(11.5)
        .child(div().text_color(t.muted).child(`${this.entries.length} ${this.entries.length === 1 ? "item" : "items"}`))
        .when(overdue > 0, (el) => el.child(div().text_color(t.blocked).child(`${overdue} overdue`))))
      .child(v_flex().id("agd-list").flex_1().min_h(0).overflow_y_scrollbar().p(7).gap(5)
        .children(this.entries.map((entry, index) => this.row(entry, index, t)))
        .when(this.entries.length === 0, (el) => el.child(div().p(10).text_color(t.dim).child(this.loading ? "Loading today’s agenda…" : "Nothing on today’s agenda"))))
      .when(Boolean(this.error), (el) => el.child(div().flex_none().px(12).py(8).border_t(1).border_color(t.border).text_size(11).text_color(t.blocked).child(this.error)));
  }
}
