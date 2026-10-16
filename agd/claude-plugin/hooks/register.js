// agd's Claude Code mod: /agd opens a pane with your agenda, /capture files a
// task without a Claude turn, and a band above the prompt shows the day's
// counts. It only runs the `agd` binary and draws what agd prints, so agd
// stays the one reader and writer of your Org files. No dependencies.

const PANE = 'agd'

const TODAY = 'today'

// The pane draws with the TUI's own palettes, which `agd agenda --views
// --json` prints by role (agd/src/tui/theme.rs is their one definition). An
// agd too old to print them gets Claude Code's theme colors for the same
// roles instead of a copy that could drift.
const THEME_KEYS = {
  accent: 'suggestion',
  overdue: 'error',
  due: 'warning',
  done: 'success',
  text: 'text',
  muted: 'inactive',
  yellow: 'warning',
  dim_red: 'error',
  pink: 'claude',
  teal: 'success',
  purple: 'permission',
  agents: ['claude', 'permission', 'suggestion'],
}

// agd's tools that change Org files; a call to one refreshes the band and
// the pane. Reads (agenda, query, describe) change nothing to show.
const WRITE_TOOL = /^mcp__(plugin_agd_)?agd__(capture|edit|plan|set_state)$/

// What the band and the pane draw; refresh() replaces them. `views` and
// `display` come from `agd agenda --views --json`, which reads config.toml:
// the saved views, and the TUI's theme, nerd_font, keyword colors and
// palettes.
// `current` is the open tab, a view's key or TODAY, starting on start_view.
// `showBand` is the plugin's show_band option; `refreshes` counts refresh()
// calls, so only the latest one started may replace what is drawn.
let status = ''
let sections = []
let problem = ''
let paneOpen = false
let views = []
let display = { theme: 'auto', nerd_font: false, keyword_colors: {}, palettes: null }
let palette = THEME_KEYS
let current = null
let showBand = true
let refreshes = 0
// When the pane last read agd, for which timed rows are past and which is
// next, and which day is today.
let now = new Date()

// Runs agd with an argument list (no shell). Resolves { stdout } or { error },
// the error the sentence agd's terminal shows, never its code.
async function agd($, args) {
  try {
    const result = await $.process.run(['agd', ...args], { timeoutMs: 15000 })
    if (result.exitCode !== 0) {
      const message = errorSentence(result.stderr.trim())
      return { error: message || 'agd ' + args[0] + ' exited with ' + result.exitCode }
    }
    return { stdout: result.stdout }
  } catch (error) {
    return { error: 'cannot run agd (is it installed and on PATH?): ' + error.message }
  }
}

// agd's stderr as a sentence: agd.error/v1's message under --json, else
// the text after `error:`.
function errorSentence(stderr) {
  if (stderr.startsWith('{')) {
    try {
      return String(JSON.parse(stderr).message || '')
    } catch {
      return stderr
    }
  }
  return stderr.replace(/^error:\s*/i, '')
}

// A saved view's sections, or today's agenda as one section.
async function readSections($, key) {
  if (key !== TODAY) {
    const view = await agd($, ['agenda', '--view', key, '--json'])
    return view.error ? { error: view.error } : parsed(view.stdout, (sections) => sections)
  }
  const day = await agd($, ['agenda', '--span', 'day', '--json'])
  if (day.error) return { error: day.error }
  return parsed(day.stdout, (entries) => [{ header: 'Today', entries, items: [], groups: [], color: 'blue' }])
}

// agd's JSON as { sections }, or { error } when it does not parse.
function parsed(stdout, toSections) {
  try {
    return { sections: toSections(JSON.parse(stdout)) }
  } catch (error) {
    return { error: 'agd printed JSON that does not parse: ' + error.message }
  }
}

// { views, display } from `agd agenda --views --json`; what is drawn now
// when agd fails, and no views when its JSON does not parse.
async function readDisplay($) {
  const listed = await agd($, ['agenda', '--views', '--json'])
  if (listed.error) return { views, display }
  try {
    const payload = JSON.parse(listed.stdout)
    return {
      views: payload.views || [],
      display: {
        theme: payload.theme || 'auto',
        nerd_font: payload.nerd_font === true,
        keyword_colors: payload.keyword_colors || {},
        palettes: payload.palettes || null,
      },
    }
  } catch {
    return { views: [], display }
  }
}

// agd's theme setting picks the palette; with `auto`, Claude Code's own theme
// does, since a pane has no terminal background to read.
async function readPalette($, display) {
  const name = display.theme === 'dark' || display.theme === 'light' ? display.theme : await claudePalette($)
  return display.palettes?.[name] || THEME_KEYS
}

// The palette Claude Code's theme asks for: ansi, light or dark.
async function claudePalette($) {
  try {
    const row = (await $.config.list()).find((config) => config.key === 'theme')
    const claudeTheme = String(row?.value ?? '')
    if (claudeTheme.includes('ansi')) return 'ansi'
    return claudeTheme.includes('light') ? 'light' : 'dark'
  } catch {
    return 'dark'
  }
}

// The open tab: `tab` while it still exists, else start_view, else Today.
function openTab(tab, views) {
  if (tab === TODAY || views.some((view) => view.key === tab)) return tab
  const start = views.find((view) => view.start)
  return start ? start.key : TODAY
}

// Reads agd again and redraws. Refreshes overlap (a timer, a write, a tab
// press), so each reads into its own values and applies them only when no
// later refresh has started: a slower, older one never draws over a newer.
async function refresh($) {
  refreshes += 1
  const mine = refreshes
  // The band's counts; with the band hidden, nothing reads them.
  const counts = showBand ? await agd($, ['agenda', '--status']) : { stdout: '' }
  const pane = paneOpen ? await readPane($) : null
  if (mine !== refreshes) return
  status = counts.error ? '' : counts.stdout.trim()
  if (pane) {
    views = pane.views
    display = pane.display
    palette = pane.palette
    current = pane.current
    problem = pane.problem
    sections = pane.sections
    now = pane.now
  }
  $.ui.invalidate('ui.render')
}

// Everything the pane draws, read fresh: the views and display settings,
// the palette, the open tab and its sections, and the time.
async function readPane($) {
  const shown = await readDisplay($)
  const tab = openTab(current, shown.views)
  const opened = await readSections($, tab)
  return {
    views: shown.views,
    display: shown.display,
    palette: await readPalette($, shown.display),
    current: tab,
    problem: opened.error || '',
    sections: opened.sections || [],
    now: await readNow($),
  }
}

// The time from Claude Code's clock, or the machine's when it has none.
async function readNow($) {
  try {
    return new Date(await $.clock.now())
  } catch {
    return new Date()
  }
}

// A refresh that runs once this dispatch has answered, so no hook waits on agd.
function refreshSoon($) {
  $.clock.after(0, () => {
    refresh($)
  })
}

async function showView($, key) {
  current = key
  await refresh($)
}

// A config color name (agd's COLOR_NAMES) in the palette.
function colorOf(name) {
  switch (name) {
    case 'orange':
      return palette.due
    case 'yellow':
      return palette.yellow
    case 'green':
      return palette.done
    case 'red':
      return palette.overdue
    case 'dim-red':
      return palette.dim_red
    case 'pink':
      return palette.pink
    case 'teal':
      return palette.teal
    case 'purple':
      return palette.purple
    case 'grey':
      return palette.muted
    default:
      return palette.accent
  }
}

// Text props with a color, leaving `color` out when the palette has none.
function colored(color, props = {}) {
  return color ? { ...props, color } : props
}

// Muted text: the palette's muted color, or the terminal's own foreground
// dimmed where the palette keeps it (16 colours), as the TUI draws it.
function muted(props = {}) {
  return palette.muted ? { ...props, color: palette.muted } : { ...props, dimColor: true }
}

// A button's props with its hotkey, when it has one; a hotkey prop must be a
// single digit or lowercase letter, never undefined.
function withHotkey(props, hotkey) {
  return hotkey ? { ...props, hotkey } : props
}

// The tab row: Today, then each saved view on its own key (`d: Dashboard`),
// as org-agenda's dispatcher keys; a view keyed `a` or `g` takes that key
// from Today or refresh.
function tabs($, Box, Button) {
  const keys = new Set(views.map((view) => view.key))
  const tab = (key, label, hotkey) =>
    Button(
      withHotkey(
        { key: 'view-' + key, label, plain: true, dimColor: current !== key, onPress: () => showView($, key) },
        /^[a-z0-9]$/.test(hotkey) ? hotkey : undefined,
      ),
    )
  return Box({
    flexDirection: 'row',
    flexWrap: 'wrap',
    columnGap: 3,
    children: [
      tab(TODAY, 'Today', keys.has('a') ? undefined : 'a'),
      ...views.map((view) => tab(view.key, view.desc, view.key)),
    ],
  })
}

// A pill as the TUI's theme.pill draws it: the label on the surface color,
// rounded with Nerd Font caps when nerd_font is on; without a surface (16
// colours) the hue reversed, so it keeps its fill on any row.
function pill(Text, text, color) {
  if (!palette.surface) {
    return [Text({ color, inverse: true, bold: true, children: [' ' + text + ' '] })]
  }
  const chip = { color, backgroundColor: palette.surface, bold: true }
  if (!display.nerd_font) return [Text({ ...chip, children: [' ' + text + ' '] })]
  return [
    Text({ color: palette.surface, children: [''] }),
    Text({ ...chip, children: [text] }),
    Text({ color: palette.surface, children: [''] }),
  ]
}

// The lead's color, as the TUI's: overdue in red, deadlines amber; null for
// the rest, which are muted.
function leadColor(occurrence) {
  if (occurrence.type === 'past-scheduled') return palette.overdue
  if (occurrence.type === 'deadline' && occurrence.extra.endsWith('ago:')) return palette.overdue
  if (occurrence.type === 'deadline' || occurrence.type === 'upcoming-deadline') return palette.due
  return null
}

// Whether an agenda row is overdue: a reminder past its date or a missed
// deadline.
function isOverdue(occurrence) {
  return occurrence.type === 'past-scheduled' || (occurrence.type === 'deadline' && occurrence.extra.endsWith('ago:'))
}

// The first number in org's leader: 12 in `Sched.12x:`, 3 in `In   3 d.:`.
function leaderDays(extra) {
  const digits = extra.match(/\d+/)
  return digits ? Number(digits[0]) : 0
}

// The clock time a row's lead shows (`16:30`, the start of a range), as
// minutes since midnight; null when its lead is a badge or countdown.
function leadMinutes(occurrence) {
  if (isOverdue(occurrence) || occurrence.type === 'deadline' || occurrence.type === 'upcoming-deadline') return null
  const time = (occurrence.time || '').match(/^(\d{1,2}):(\d{2})/)
  return time ? Number(time[1]) * 60 + Number(time[2]) : null
}

// The six-cell lead that always says when, as the TUI's lead_cell: an
// overdue badge (`12d`, `-2d`, right-aligned), a countdown (`in 3d`), `due`
// on a deadline's day, or the time: muted once past or done, bold on the
// next timed task, accent later.
function leadCell(Text, occurrence, done, clock) {
  if (occurrence.type === 'past-scheduled') return pill(Text, (leaderDays(occurrence.extra) + 'd').padStart(4), palette.overdue)
  if (occurrence.type === 'deadline' && occurrence.extra.endsWith('ago:')) {
    return pill(Text, ('-' + leaderDays(occurrence.extra) + 'd').padStart(4), palette.overdue)
  }
  if (occurrence.type === 'upcoming-deadline') {
    return [Text(colored(palette.due, { children: [('in ' + leaderDays(occurrence.extra) + 'd').padEnd(6)] }))]
  }
  if (occurrence.type === 'deadline') return [Text(colored(palette.due, { bold: true, children: ['due   '] }))]
  if (!occurrence.time) return [Text({ children: ['      '] })]
  const time = { children: [occurrence.time.padEnd(6)] }
  if (done || clock === 'past') return [Text(muted(time))]
  if (clock === 'next') return [Text(colored(palette.accent, { ...time, bold: true }))]
  return [Text(colored(palette.accent, time))]
}

// One task as the TUI's view rows draw it: for an agenda entry the category
// and org-agenda's lead (`10:00......`, `In 2 d.:`, `Sched. 2x:`), else an
// indent; then the keyword in its color, the title, the priority chip and
// the tags, muted.
function taskRow(Text, item, occurrence) {
  const parts = []
  if (occurrence) {
    parts.push(Text(muted({ children: [(item.category + ':').padEnd(12)] })))
    if (occurrence.time) parts.push(Text({ children: [occurrence.time + '...... '] }))
    if (occurrence.extra) {
      const lead = { children: [occurrence.extra.padEnd(11) + ' '] }
      const color = leadColor(occurrence)
      parts.push(Text(color ? { ...lead, color } : muted(lead)))
    }
  } else {
    parts.push(Text({ children: ['       '] }))
  }
  if (item.todo) {
    const name = display.keyword_colors[item.todo] || (item.done ? 'green' : 'blue')
    const bold = !item.done && name !== 'orange'
    parts.push(Text(colored(colorOf(name), { bold, children: [item.todo + ' '] })))
  }
  parts.push(Text({ ...titleStyle(item, false), children: [displayTitle(item.title)] }))
  if (item.priority) parts.push(Text({ children: [' '] }), ...pill(Text, item.priority, palette.due))
  if (item.tags.length > 0) parts.push(Text(muted({ children: ['  #' + item.tags.join(' #')] })))
  return Text({ wrap: 'truncate-end', children: parts })
}

// A title's style: muted and struck through when done, as in the TUI, bold
// on the next timed task.
function titleStyle(item, next) {
  if (item.done) return muted({ strikethrough: true })
  return colored(palette.text, next ? { bold: true } : {})
}

// The title as agd's lists show it: active timestamps dropped, and
// `[[target][description]]` read as its description, `[[target]]` as the target.
function displayTitle(title) {
  return title
    .replace(/[ \t]*<\d{4}-\d{2}-\d{2}[^>\n]*>/g, '')
    .replace(/\[\[([^\]]*)\](?:\[([^\]]*)\])?\]/g, (_, target, description) => description || target)
    .trim()
}

// Rows for a block or a group: agenda entries under their day, list items
// as they come.
function rows(Text, entries, items) {
  const lines = []
  let day = ''
  for (const entry of entries) {
    if (entry.occurrence.date !== day) {
      day = entry.occurrence.date
      // Today's date stands out in accent, underlined, as in the TUI.
      const style = day === isoDate(now) ? colored(palette.accent, { underline: true }) : colored(palette.text)
      lines.push(Text({ ...style, bold: true, children: [dayLabel(day)] }))
    }
    lines.push(taskRow(Text, entry.item, entry.occurrence))
  }
  for (const item of items) lines.push(taskRow(Text, item, null))
  return lines
}

const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday']
const MONTHS = [
  'January',
  'February',
  'March',
  'April',
  'May',
  'June',
  'July',
  'August',
  'September',
  'October',
  'November',
  'December',
]

// 2026-10-15 as org-agenda and the TUI's day lists head it,
// `Thursday   15 October 2026`, in UTC so the date never shifts.
function dayLabel(date) {
  const parsed = new Date(date + 'T00:00:00Z')
  const weekday = WEEKDAYS[parsed.getUTCDay()].padEnd(10)
  const day = String(parsed.getUTCDate()).padStart(2)
  return weekday + ' ' + day + ' ' + MONTHS[parsed.getUTCMonth()] + ' ' + parsed.getUTCFullYear()
}

// A Date as `2026-10-16`, in local time as agd reads today.
function isoDate(date) {
  const pad = (number) => String(number).padStart(2, '0')
  return date.getFullYear() + '-' + pad(date.getMonth() + 1) + '-' + pad(date.getDate())
}

// An uppercase header in its color with a muted row count, as in the TUI;
// an empty section is muted.
function header(Text, text, count, color) {
  const label = text.toUpperCase()
  if (count === 0) return Text(muted({ bold: true, children: [label] }))
  return Text({
    children: [
      Text(colored(colorOf(color), { bold: true, children: [label] })),
      Text(muted({ children: ['  ' + count] })),
    ],
  })
}

function sectionCount(section) {
  const own = (section.entries || []).length + (section.items || []).length
  return (section.groups || []).reduce(
    (sum, group) => sum + group.entries.length + group.items.length,
    own,
  )
}

// One section's lines: its header, its rows, then each group's.
function sectionLines(Text, section) {
  const lines = [header(Text, section.header || 'Agenda', sectionCount(section), section.color)]
  lines.push(...rows(Text, section.entries || [], section.items || []))
  for (const group of section.groups || []) {
    const count = group.entries.length + group.items.length
    lines.push(header(Text, ' ' + group.name, count, group.color))
    lines.push(...rows(Text, group.entries, group.items))
  }
  return lines
}

// Today as the TUI's pane draws it, compact enough for a 40-column pane:
// overdue rows first, most days late first, then timed rows by time,
// untimed ones, and done rows last; each row the marker (`▸` on the next
// timed task), the lead, the title, and at the right the priority, ↻ for
// a repeating task or ✓ for done.
function todayLines(Box, Text, entries) {
  const minutes = now.getHours() * 60 + now.getMinutes()
  const late = entries.filter((entry) => isOverdue(entry.occurrence))
  late.sort((left, right) => leaderDays(right.occurrence.extra) - leaderDays(left.occurrence.extra))
  const rest = entries.filter((entry) => !isOverdue(entry.occurrence))
  const order = (entry) => {
    const time = leadMinutes(entry.occurrence)
    return [entry.item.done ? 1 : 0, time === null ? 1 : 0, time ?? 0]
  }
  rest.sort((left, right) => {
    const [a, b] = [order(left), order(right)]
    return a[0] - b[0] || a[1] - b[1] || a[2] - b[2]
  })
  const next = rest.find((entry) => !entry.item.done && (leadMinutes(entry.occurrence) ?? -1) >= minutes)
  const open = entries.filter((entry) => !entry.item.done).length
  const lines = [header(Text, 'Today', open, 'blue')]
  for (const entry of [...late, ...rest]) {
    const time = leadMinutes(entry.occurrence)
    const clock = entry === next ? 'next' : time !== null && time < minutes ? 'past' : 'later'
    lines.push(compactRow(Box, Text, entry, clock))
  }
  if (entries.length === 0) lines.push(Text(muted({ children: ['All clear for today.'] })))
  return lines
}

// One compact row: what is left of the right mark's room goes to the
// title, which ends in "…" when it does not fit.
function compactRow(Box, Text, entry, clock) {
  const { item, occurrence } = entry
  const marker = clock === 'next' ? Text(colored(palette.accent, { bold: true, children: ['▸ '] })) : Text({ children: ['  '] })
  const left = Text({
    wrap: 'truncate-end',
    children: [
      marker,
      ...leadCell(Text, occurrence, item.done, clock),
      Text({ children: [' '] }),
      Text({ ...titleStyle(item, clock === 'next'), children: [displayTitle(item.title)] }),
    ],
  })
  const planning = item.planning || {}
  const repeats = [planning.scheduled, planning.deadline].some((timestamp) => timestamp && timestamp.repeater)
  let right = null
  if (item.done) right = [Text(colored(palette.done, { bold: true, children: ['✓'] }))]
  else if (item.priority) right = pill(Text, item.priority, palette.due)
  else if (repeats) right = [Text(muted({ children: ['↻'] }))]
  const children = [Box({ flexGrow: 1, flexShrink: 1, children: [left] })]
  if (right) children.push(Box({ flexDirection: 'row', flexShrink: 0, marginLeft: 1, children: right }))
  return Box({ flexDirection: 'row', children })
}

function paneBody($, e) {
  const { Box, Text, Button } = $.ui.resolve(e)
  const blank = () => Text({ children: [' '] })
  const today = current === TODAY
  const body = problem
    ? [Text(colored(palette.overdue, { children: [problem] }))]
    : today
      ? todayLines(Box, Text, sections.flatMap((section) => section.entries || []))
      : sections.flatMap((section, index) => [...(index > 0 ? [blank()] : []), ...sectionLines(Text, section)])
  const footer = Box({
    flexDirection: 'row',
    columnGap: 3,
    children: [
      Button(
        withHotkey(
          { key: 'refresh', label: 'refresh', plain: true, onPress: () => refresh($) },
          views.some((view) => view.key === 'g') ? undefined : 'g',
        ),
      ),
      Text(muted({ children: ['esc close · ctrl-x x close'] })),
    ],
  })
  return Box({
    flexDirection: 'column',
    width: e.props.bodyColumns,
    children: [tabs($, Box, Button), blank(), ...body, blank(), footer],
  })
}

// "/capture t buy milk due:fri" runs `agd capture t "buy milk due:fri"`.
// It says what it wrote as agd's TUI and terminal do:
// `inbox.org:7  captured · buy milk · deadline Fri 23 Oct`.
async function capture($, args) {
  const match = args.trim().match(/^(\S+)\s+(.+)$/)
  if (!match) return { text: 'usage: /capture KEY text, such as /capture t buy milk due:fri' }
  const result = await agd($, ['capture', match[1], match[2], '--json'])
  // Claude Code prefixes the plugin's name, so the text starts with agd's own.
  if (result.error) return { text: result.error }
  const today = await readNow($)
  await refresh($)
  try {
    return { text: capturedLine(JSON.parse(result.stdout).item, today) }
  } catch {
    return { text: 'captured' }
  }
}

// agd's words::captured: the file and line, `captured`, the title, then its
// dates (`scheduled Sat 17 Oct`, `deadline Fri 23 Oct`).
function capturedLine(item, today) {
  let line = item.file.split('/').pop() + ':' + item.line + '  captured · ' + item.title
  const planning = item.planning || {}
  for (const [label, timestamp] of [
    ['scheduled', planning.scheduled],
    ['deadline', planning.deadline],
  ]) {
    if (timestamp) line += ' · ' + label + ' ' + when(timestamp.date, timestamp.time, today)
  }
  return line
}

const SHORT_WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']
const SHORT_MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

// agd's words::when: `Fri 16 Oct`, with the year when it is not `today`'s,
// and the time after it.
function when(date, time, today) {
  const parsed = new Date(date + 'T00:00:00Z')
  if (Number.isNaN(parsed.getTime())) return date
  let text = SHORT_WEEKDAYS[parsed.getUTCDay()] + ' ' + parsed.getUTCDate() + ' ' + SHORT_MONTHS[parsed.getUTCMonth()]
  if (parsed.getUTCFullYear() !== today.getFullYear()) text += ' ' + parsed.getUTCFullYear()
  return time ? text + ' ' + time : text
}

// Literal names, so `claude plugin validate` sees each command.run hook
// answer only its own command. A name another plugin took is logged, not fatal.
async function registerCommands($) {
  try {
    await $.command.register({ name: 'agd', description: 'Open or close your agd agenda pane', immediate: true })
  } catch (error) {
    $.ui.log('/agd not added: ' + error.message)
  }
  try {
    await $.command.register({
      name: 'capture',
      description: 'File a task through an agd capture template',
      argumentHint: 'KEY text  due:fri on:+2d +tag #A',
      immediate: true,
    })
  } catch (error) {
    $.ui.log('/capture not added: ' + error.message)
  }
}

// `options` holds plugin.json's userConfig values; a change to one loads
// the module again with the new values.
export function register(on, options) {
  showBand = options?.show_band !== false

  // The session waits on this hook, so agd is read after it answers.
  on('session.start', async ($, e, next) => {
    await registerCommands($)
    $.clock.every(60000, () => refresh($))
    refreshSoon($)
    return next(e)
  })

  on('command.run', { command: 'agd' }, async ($) => {
    if (paneOpen) {
      await $.ui.close({ id: PANE })
      paneOpen = false
      return {}
    }
    paneOpen = true
    await refresh($)
    await $.ui.open({ id: PANE, title: 'agd', focus: true, closeOnEscape: true })
    return {}
  })

  on('command.run', { command: 'capture' }, async ($, e) => capture($, e.args))

  // Observers: a failure here must never stop a close or a tool call, so
  // each .catch passes the event on as it was.
  on('ui.close', async (_, e, next) => {
    if (e.id === PANE) paneOpen = false
    return next(e)
  }).catch((_, e, next) => next(e))

  // After Claude changes an agd item, show what it changed: the result goes
  // back to Claude at once, and the band and pane catch up after.
  on('tool.call', async ($, e, next) => {
    const result = await next(e)
    if (WRITE_TOOL.test(e.tool) && !result.deny && !result.isError) refreshSoon($)
    return result
  }).catch((_, e, next) => next(e))

  on('ui.render', { component: 'Pane' }, async ($, e, next) => {
    if (e.requestId !== PANE) return next(e)
    return paneBody($, e)
  })

  if (!showBand) return
  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    // Stay out of the way of Claude Code's own survey in the band.
    if (!status || e.props.hasSurvey) return next(e)
    const { Box, Text } = $.ui.resolve(e)
    const theirs = await next(e)
    const line = Text({ dimColor: true, wrap: 'truncate-end', children: ['agd · ' + status] })
    return theirs ? Box({ flexDirection: 'column', children: [theirs, line] }) : line
  })
}
