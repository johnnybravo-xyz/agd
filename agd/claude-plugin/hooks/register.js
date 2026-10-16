// agd's Claude Code mod: /agd opens a pane with your agenda, /capture files a
// task without a Claude turn, and a band above the prompt shows the day's
// counts. It only runs the `agd` binary and draws what agd prints, so agd
// stays the one reader and writer of your Org files. No dependencies.

const PANE = 'agd'

const TODAY = 'today'

// agd's TUI palette, as agd/src/tui/theme.rs defines it; keep the two in step.
const PALETTES = {
  dark: {
    accent: '#6ea8fe',
    overdue: '#f07178',
    due: '#e5b567',
    done: '#7ec68f',
    text: '#dee2e8',
    muted: '#808896',
    surface: '#20252e',
    yellow: '#ecd678',
    dimRed: '#b07076',
    pink: '#e879b0',
    teal: '#5ec4a0',
    purple: '#b094f5',
  },
  light: {
    accent: '#2563c4',
    overdue: '#c53030',
    due: '#b06f00',
    done: '#248748',
    text: '#1f2329',
    muted: '#6c727e',
    surface: '#eef0f3',
    yellow: '#967a00',
    dimRed: '#b87878',
    pink: '#c83c82',
    teal: '#148c64',
    purple: '#6e50c8',
  },
  // The TUI's 16-color palette, for Claude Code's ANSI themes; text keeps
  // the terminal's own foreground.
  ansi: {
    accent: 'blueBright',
    overdue: 'red',
    due: 'yellow',
    done: 'green',
    text: undefined,
    muted: 'gray',
    surface: undefined,
    yellow: 'yellowBright',
    dimRed: 'red',
    pink: 'magenta',
    teal: 'cyan',
    purple: 'magentaBright',
  },
}

// agd's tools that change Org files; a call to one refreshes the band and
// the pane. Reads (agenda, query, describe) change nothing to show.
const WRITE_TOOL = /^mcp__(plugin_agd_)?agd__(capture|edit|plan|set_state)$/

// What the band and the pane draw; refresh() replaces them. `views` and
// `display` come from `agd agenda --views --json`, which reads config.toml:
// the saved views, and the TUI's theme, nerd_font and keyword colors.
// `current` is the open tab, a view's key or TODAY, starting on start_view.
// `showBand` is the plugin's show_band option; `refreshes` counts refresh()
// calls, so only the latest one started may replace what is drawn.
let status = ''
let sections = []
let problem = ''
let paneOpen = false
let views = []
let display = { theme: 'auto', nerd_font: false, keyword_colors: {} }
let palette = PALETTES.dark
let current = null
let showBand = true
let refreshes = 0

// Runs agd with an argument list (no shell). Resolves { stdout } or { error }.
async function agd($, args) {
  try {
    const result = await $.process.run(['agd', ...args], { timeoutMs: 15000 })
    if (result.exitCode !== 0) {
      const message = result.stderr.trim().replace(/^Error:\s*/, '')
      return { error: message || 'agd ' + args[0] + ' exited with ' + result.exitCode }
    }
    return { stdout: result.stdout }
  } catch (error) {
    return { error: 'cannot run agd (is it installed and on PATH?): ' + error.message }
  }
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
      },
    }
  } catch {
    return { views: [], display }
  }
}

// agd's theme setting picks the palette; with `auto`, Claude Code's own theme
// does, since a pane has no terminal background to read.
async function readPalette($, theme) {
  if (theme === 'dark' || theme === 'light') return PALETTES[theme]
  try {
    const row = (await $.config.list()).find((config) => config.key === 'theme')
    const claudeTheme = String(row?.value ?? '')
    if (claudeTheme.includes('ansi')) return PALETTES.ansi
    return claudeTheme.includes('light') ? PALETTES.light : PALETTES.dark
  } catch {
    return PALETTES.dark
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
  }
  $.ui.invalidate('ui.render')
}

// Everything the pane draws, read fresh: the views and display settings,
// the palette, the open tab and its sections.
async function readPane($) {
  const shown = await readDisplay($)
  const tab = openTab(current, shown.views)
  const opened = await readSections($, tab)
  return {
    views: shown.views,
    display: shown.display,
    palette: await readPalette($, shown.display.theme),
    current: tab,
    problem: opened.error || '',
    sections: opened.sections || [],
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
      return palette.dimRed
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

// The priority as the TUI's chip: amber on the surface color, rounded with
// Nerd Font caps when nerd_font is on; black on amber without a surface.
function priorityChip(Text, priority) {
  if (!palette.surface) {
    return [Text({ color: 'black', backgroundColor: palette.due, bold: true, children: [' ' + priority + ' '] })]
  }
  const chip = { color: palette.due, backgroundColor: palette.surface, bold: true }
  if (!display.nerd_font) return [Text({ ...chip, children: [' ' + priority + ' '] })]
  return [
    Text({ color: palette.surface, children: ['\ue0b6'] }),
    Text({ ...chip, children: [priority] }),
    Text({ color: palette.surface, children: ['\ue0b4'] }),
  ]
}

// The lead's color, as the TUI's: overdue in red, deadlines amber, the rest muted.
function leadColor(occurrence) {
  if (occurrence.type === 'past-scheduled') return palette.overdue
  if (occurrence.type === 'deadline' && occurrence.extra.endsWith('ago:')) return palette.overdue
  if (occurrence.type === 'deadline' || occurrence.type === 'upcoming-deadline') return palette.due
  return palette.muted
}

// One task as the TUI's view rows draw it: for an agenda entry the category
// and org-agenda's lead (`10:00......`, `In 2 d.:`, `Sched. 2x:`), else an
// indent; then the keyword in its color, the title, the priority chip and
// the tags.
function taskRow(Text, item, occurrence) {
  const parts = []
  if (occurrence) {
    parts.push(Text(colored(palette.muted, { children: [(item.category + ':').padEnd(12)] })))
    if (occurrence.time) parts.push(Text({ children: [occurrence.time + '...... '] }))
    if (occurrence.extra) parts.push(Text(colored(leadColor(occurrence), { children: [occurrence.extra.padEnd(11) + ' '] })))
  } else {
    parts.push(Text({ children: ['       '] }))
  }
  if (item.todo) {
    const name = display.keyword_colors[item.todo] || (item.done ? 'green' : 'blue')
    const bold = !item.done && name !== 'orange'
    parts.push(Text(colored(colorOf(name), { bold, children: [item.todo + ' '] })))
  }
  // A done title is muted and struck through, as in the TUI.
  const title = item.done ? colored(palette.muted, { strikethrough: true }) : colored(palette.text)
  parts.push(Text({ ...title, children: [displayTitle(item.title)] }))
  if (item.priority) parts.push(Text({ children: [' '] }), ...priorityChip(Text, item.priority))
  if (item.tags.length > 0) parts.push(Text(colored(palette.accent, { children: ['  #' + item.tags.join(' #')] })))
  return Text({ wrap: 'truncate-end', children: parts })
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
      lines.push(Text(colored(palette.text, { bold: true, children: [dayLabel(day)] })))
    }
    lines.push(taskRow(Text, entry.item, entry.occurrence))
  }
  for (const item of items) lines.push(taskRow(Text, item, null))
  return lines
}

const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

// 2026-10-15 as "Thu 15 Oct", in UTC so the date never shifts.
function dayLabel(date) {
  const parsed = new Date(date + 'T00:00:00Z')
  return WEEKDAYS[parsed.getUTCDay()] + ' ' + parsed.getUTCDate() + ' ' + MONTHS[parsed.getUTCMonth()]
}

// An uppercase header in its color with a muted row count, as in the TUI;
// an empty section is muted.
function header(Text, text, count, color) {
  const label = text.toUpperCase()
  if (count === 0) return Text(colored(palette.muted, { bold: true, children: [label] }))
  return Text({
    children: [
      Text(colored(colorOf(color), { bold: true, children: [label] })),
      Text(colored(palette.muted, { children: ['  ' + count] })),
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

function paneBody($, e) {
  const { Box, Text, Button } = $.ui.resolve(e)
  const blank = () => Text({ children: [' '] })
  const body = problem
    ? [Text(colored(palette.overdue, { children: [problem] }))]
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
      Text(colored(palette.muted, { children: ['esc close · ctrl-x x close'] })),
    ],
  })
  return Box({
    flexDirection: 'column',
    width: e.props.bodyColumns,
    children: [tabs($, Box, Button), blank(), ...body, blank(), footer],
  })
}

// "/capture t buy milk due:fri" runs `agd capture t "buy milk due:fri"`.
async function capture($, args) {
  const match = args.trim().match(/^(\S+)\s+(.+)$/)
  if (!match) return { text: 'usage: /capture KEY text, such as /capture t buy milk due:fri' }
  const result = await agd($, ['capture', match[1], match[2]])
  // Claude Code prefixes the plugin's name, so the text starts with agd's own.
  if (result.error) return { text: result.error }
  await refresh($)
  // agd prints the new item's ref, "<file>:<line>#sha256:…".
  const location = result.stdout.trim().split('#')[0].split('/').pop()
  return { text: 'captured to ' + location }
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
