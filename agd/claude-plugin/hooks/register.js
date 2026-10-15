// agd's Claude Code mod: /agd opens a pane with your agenda, /capture files a
// task without a Claude turn, and a band above the prompt shows the day's
// counts. It only runs the `agd` binary and draws what agd prints, so agd
// stays the one reader and writer of your Org files. No dependencies.

const PANE = 'agd'

const TODAY = 'today'

// What the band and the pane draw; refresh() replaces them. `views` are the
// saved views from `agd agenda --views`; `current` is the open tab, a view's
// key or TODAY, starting on start_view.
let status = ''
let sections = []
let problem = ''
let paneOpen = false
let views = []
let current = null

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
  return parsed(day.stdout, (entries) => [{ header: 'Today', entries, items: [], groups: [] }])
}

async function readViews($) {
  const listed = await agd($, ['agenda', '--views', '--json'])
  if (listed.error) return []
  try {
    return JSON.parse(listed.stdout)
  } catch {
    return []
  }
}

// agd's JSON as { sections }, or { error } when it does not parse.
function parsed(stdout, toSections) {
  try {
    return { sections: toSections(JSON.parse(stdout)) }
  } catch (error) {
    return { error: 'agd printed JSON that does not parse: ' + error.message }
  }
}

async function refresh($) {
  const counts = await agd($, ['agenda', '--status'])
  status = counts.error ? '' : counts.stdout.trim()
  if (paneOpen) {
    views = await readViews($)
    if (current === null || (current !== TODAY && !views.some((view) => view.key === current))) {
      const start = views.find((view) => view.start)
      current = start ? start.key : TODAY
    }
    const read = await readSections($, current)
    problem = read.error || ''
    sections = read.sections || []
  }
  $.ui.invalidate('ui.render')
}

async function showView($, key) {
  current = key
  await refresh($)
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

// A config color name (agd's COLOR_NAMES) as Text props.
function colorProps(name) {
  switch (name) {
    case 'grey':
      return { dimColor: true }
    // dimColor would turn red grey in Claude Code, so a muted red instead,
    // the TUI's own dim-red.
    case 'dim-red':
      return { color: '#b07076' }
    case 'teal':
      return { color: 'cyan' }
    case 'purple':
    case 'pink':
      return { color: 'magenta' }
    case 'orange':
    case 'yellow':
    case 'green':
    case 'red':
    case 'blue':
      return { color: name }
    default:
      return { color: 'blue' }
  }
}

// One task: the agenda lead (time, "In 2 d.:", "Sched. 2x:") when there is
// one, the keyword, priority, title and tags.
function rowText(item, lead) {
  const parts = []
  if (lead !== undefined) parts.push(lead.padEnd(11))
  if (item.todo) parts.push(item.todo)
  if (item.priority) parts.push('[#' + item.priority + ']')
  // Like agd's own lists, drop active timestamps from the title.
  parts.push(item.title.replace(/<\d{4}-\d{2}-\d{2}[^>]*>/g, '').trim())
  if (item.tags.length > 0) parts.push(':' + item.tags.join(':') + ':')
  return parts.join(' ')
}

function entryLead(entry) {
  const occurrence = entry.occurrence
  return occurrence.time ? occurrence.time : occurrence.extra || ''
}

// Rows for a block or a group: agenda entries under their day, list items
// as they come.
function rows(Text, entries, items) {
  const lines = []
  let day = ''
  for (const entry of entries) {
    if (entry.occurrence.date !== day) {
      day = entry.occurrence.date
      lines.push(Text({ bold: true, children: [dayLabel(day)] }))
    }
    lines.push(textRow(Text, entry.item, rowText(entry.item, entryLead(entry))))
  }
  for (const item of items) lines.push(textRow(Text, item, rowText(item)))
  return lines
}

function textRow(Text, item, text) {
  return Text({ wrap: 'truncate-end', dimColor: item.done, children: ['  ' + text] })
}

const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

// 2026-10-15 as "Thu 15 Oct", in UTC so the date never shifts.
function dayLabel(date) {
  const parsed = new Date(date + 'T00:00:00Z')
  return WEEKDAYS[parsed.getUTCDay()] + ' ' + parsed.getUTCDate() + ' ' + MONTHS[parsed.getUTCMonth()]
}

// An uppercase header with its row count; empty sections are dimmed.
function header(Text, text, count, color) {
  const style = count === 0 ? { dimColor: true } : colorProps(color)
  const label = text.toUpperCase() + (count > 0 ? '  ' + count : '')
  return Text({ bold: true, ...style, children: [label] })
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
    ? [Text({ color: 'red', children: [problem] })]
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
      Text({ dimColor: true, children: ['esc close · ctrl-x x close'] }),
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

export function register(on) {
  on('session.start', async ($, e, next) => {
    await refresh($)
    $.clock.every(60000, () => refresh($))
    await registerCommands($)
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

  // After Claude uses an agd tool, show what it changed.
  on('tool.call', async ($, e, next) => {
    const result = await next(e)
    if (/^mcp__(plugin_agd_)?agd__/.test(e.tool)) await refresh($)
    return result
  }).catch((_, e, next) => next(e))

  on('ui.render', { component: 'Pane' }, async ($, e, next) => {
    if (e.requestId !== PANE) return next(e)
    return paneBody($, e)
  })

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    // Stay out of the way of Claude Code's own survey in the band.
    if (!status || e.props.hasSurvey) return next(e)
    const { Box, Text } = $.ui.resolve(e)
    const theirs = await next(e)
    const line = Text({ dimColor: true, wrap: 'truncate-end', children: ['agd · ' + status] })
    return theirs ? Box({ flexDirection: 'column', children: [theirs, line] }) : line
  })
}
